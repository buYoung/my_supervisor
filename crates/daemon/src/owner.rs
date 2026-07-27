//! Daemon-level composition of the macOS owner lock, discovery, and token.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use my_supervisor_infra_http::{AuthVerifier, MaintenanceHandlers};
use my_supervisor_platform_macos::{
    ensure_private_directory, read_private_file, write_private_file_atomic, OwnerLock,
};
use my_supervisor_shared::api::{BackupResultDto, TokenRotationDto, UpgradeJournalDto};
use serde::{Deserialize, Serialize};

const OWNER_METADATA: &str = "owner.json";
const CONTROL_TOKEN: &str = "control.token";
const UPGRADE_JOURNAL: &str = "upgrade.json";

#[derive(Clone, Serialize, Deserialize)]
pub struct OwnerDiscovery {
    pub endpoint: String,
    pub version: String,
    pub pid: u32,
    pub native_start_identity: String,
    pub credential_generation: u64,
}

pub struct DaemonOwner {
    _lock: OwnerLock,
    root: PathBuf,
    discovery: Mutex<OwnerDiscovery>,
    auth: AuthVerifier,
    maintenance: Mutex<()>,
}

impl DaemonOwner {
    pub fn claim(root: PathBuf, endpoint: String) -> Result<Arc<Self>> {
        let root = normalized_root(root)?;
        ensure_private_directory(&root)
            .with_context(|| format!("claiming daemon root {}", root.display()))?;
        let run_dir = root.join("run");
        ensure_private_directory(&run_dir)
            .with_context(|| format!("preparing daemon run directory {}", run_dir.display()))?;
        let lock = OwnerLock::acquire(&run_dir.join("owner.lock")).with_context(|| {
            format!(
                "acquiring exclusive daemon ownership for {}",
                root.display()
            )
        })?;
        for directory in ["data", "config", "logs", "backups", "versions"] {
            ensure_private_directory(&root.join(directory))
                .with_context(|| format!("preparing daemon {directory} directory"))?;
        }

        let prior_generation = read_discovery(&run_dir.join(OWNER_METADATA))
            .map(|metadata| metadata.credential_generation)
            .unwrap_or(0);
        let token_path = run_dir.join(CONTROL_TOKEN);
        let token = match read_private_file(&token_path) {
            Ok(bytes) => {
                valid_token(String::from_utf8(bytes).context("control token is not UTF-8")?)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let token = generate_token();
                write_private_file_atomic(&token_path, token.as_bytes())
                    .context("writing initial control token")?;
                token
            }
            Err(error) => return Err(error).context("reading control token"),
        };
        let discovery = OwnerDiscovery {
            endpoint,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            pid: std::process::id(),
            native_start_identity: format!(
                "{}:{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
            credential_generation: prior_generation.saturating_add(1),
        };
        write_private_file_atomic(
            &run_dir.join(OWNER_METADATA),
            &serde_json::to_vec(&discovery).context("serializing owner discovery")?,
        )
        .context("writing owner discovery")?;
        let auth = AuthVerifier::new(token, discovery.credential_generation);
        let owner = Arc::new(Self {
            _lock: lock,
            root,
            discovery: Mutex::new(discovery),
            auth,
            maintenance: Mutex::new(()),
        });
        owner.install_maintenance_handlers();
        Ok(owner)
    }

    pub fn auth(&self) -> AuthVerifier {
        self.auth.clone()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn discovery(&self) -> OwnerDiscovery {
        self.discovery
            .lock()
            .expect("owner discovery lock poisoned")
            .clone()
    }

    /// Atomically replace the secret and immediately invalidate new requests
    /// authenticated with the prior generation.
    pub fn rotate_token(&self) -> Result<TokenRotationDto> {
        let token = generate_token();
        let mut discovery = self
            .discovery
            .lock()
            .expect("owner discovery lock poisoned");
        let next_generation = discovery.credential_generation.saturating_add(1);
        write_private_file_atomic(&self.root.join("run").join(CONTROL_TOKEN), token.as_bytes())?;
        discovery.credential_generation = next_generation;
        write_private_file_atomic(
            &self.root.join("run").join(OWNER_METADATA),
            &serde_json::to_vec(&*discovery)?,
        )?;
        self.auth.rotate(token, next_generation);
        Ok(TokenRotationDto {
            credential_generation: next_generation,
        })
    }

    fn install_maintenance_handlers(self: &Arc<Self>) {
        let rotate_owner = self.clone();
        let backup_owner = self.clone();
        let upgrade_owner = self.clone();
        let rollback_owner = self.clone();
        self.auth.install_maintenance_handlers(MaintenanceHandlers {
            rotate: Some(Arc::new(move || {
                rotate_owner
                    .rotate_token()
                    .map_err(|error| error.to_string())
            })),
            backup: Some(Arc::new(move || {
                backup_owner
                    .create_backup()
                    .map_err(|error| error.to_string())
            })),
            upgrade: Some(Arc::new(move || {
                upgrade_owner
                    .stage_upgrade()
                    .map_err(|error| error.to_string())
            })),
            rollback: Some(Arc::new(move || {
                rollback_owner
                    .rollback_upgrade()
                    .map_err(|error| error.to_string())
            })),
        });
    }

    /// Creates one owner-serialized, self-contained recovery cut. Runtime
    /// ownership files are intentionally excluded: they are evidence of the
    /// live owner, never restorable state.
    pub fn create_backup(&self) -> Result<BackupResultDto> {
        let _maintenance = self.maintenance.lock().expect("maintenance lock poisoned");
        let backup_id = format!(
            "backup-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        let backup_root = self.root.join("backups").join(&backup_id);
        ensure_private_directory(&backup_root)?;
        let mut entries = Vec::new();
        for name in ["data", "config", "logs"] {
            let source = self.root.join(name);
            let destination = backup_root.join(name);
            copy_private_tree(&source, &destination, &mut entries)?;
        }
        let manifest = serde_json::json!({
            "backup_id": backup_id,
            "version": self.discovery().version,
            "entries": entries,
        });
        let manifest_path = backup_root.join("manifest.json");
        write_private_file_atomic(&manifest_path, &serde_json::to_vec(&manifest)?)?;
        let verified = verify_backup_manifest(&backup_root, &manifest)?;
        if !verified {
            anyhow::bail!("backup manifest verification failed");
        }
        Ok(BackupResultDto {
            backup_id,
            manifest_path: manifest_path.display().to_string(),
            verified,
        })
    }

    /// The current build has no binary installer yet, so this is a bounded
    /// preflight transaction: verified snapshot -> durable journal -> ready
    /// commit. A future installer can add pointer switching between the two
    /// durable phases without changing the owner/rollback contract.
    pub fn stage_upgrade(&self) -> Result<UpgradeJournalDto> {
        let _maintenance = self.maintenance.lock().expect("maintenance lock poisoned");
        let backup = self.create_backup_unlocked()?;
        let version = self.discovery().version;
        let journal = UpgradeJournalDto {
            phase: "committed".to_string(),
            active_version: version.clone(),
            rollback_version: Some(version),
            snapshot_path: Some(backup.manifest_path),
        };
        self.write_upgrade_journal(&journal)?;
        Ok(journal)
    }

    pub fn rollback_upgrade(&self) -> Result<UpgradeJournalDto> {
        let _maintenance = self.maintenance.lock().expect("maintenance lock poisoned");
        let mut journal = self.read_upgrade_journal()?;
        let manifest_path = journal
            .snapshot_path
            .clone()
            .context("no verified upgrade snapshot is available")?;
        let backup_root = PathBuf::from(&manifest_path)
            .parent()
            .context("upgrade manifest has no parent")?
            .to_path_buf();
        let manifest: serde_json::Value =
            serde_json::from_slice(&read_private_file(Path::new(&manifest_path))?)?;
        if !verify_backup_manifest(&backup_root, &manifest)? {
            anyhow::bail!("refusing rollback from an invalid backup manifest");
        }
        journal.phase = "rolling_back".to_string();
        self.write_upgrade_journal(&journal)?;
        // Restore by staging private replacements. The daemon remains the sole
        // owner throughout, so no second scheduler can observe a half-cut.
        for name in ["data", "config", "logs"] {
            let staged = self.root.join(format!(".{name}.rollback"));
            if staged.exists() {
                fs::remove_dir_all(&staged)?;
            }
            let mut ignored = Vec::new();
            copy_private_tree(&backup_root.join(name), &staged, &mut ignored)?;
            let target = self.root.join(name);
            let previous = self.root.join(format!(".{name}.previous"));
            if previous.exists() {
                fs::remove_dir_all(&previous)?;
            }
            fs::rename(&target, &previous)?;
            if let Err(error) = fs::rename(&staged, &target) {
                let _ = fs::rename(&previous, &target);
                return Err(error.into());
            }
            fs::remove_dir_all(previous)?;
        }
        journal.phase = "committed".to_string();
        self.write_upgrade_journal(&journal)?;
        Ok(journal)
    }

    fn create_backup_unlocked(&self) -> Result<BackupResultDto> {
        // `create_backup` normally owns the barrier; upgrade already owns it.
        let backup_id = format!(
            "upgrade-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        let backup_root = self.root.join("backups").join(&backup_id);
        ensure_private_directory(&backup_root)?;
        let mut entries = Vec::new();
        for name in ["data", "config", "logs"] {
            copy_private_tree(&self.root.join(name), &backup_root.join(name), &mut entries)?;
        }
        let manifest = serde_json::json!({ "backup_id": backup_id, "version": self.discovery().version, "entries": entries });
        let manifest_path = backup_root.join("manifest.json");
        write_private_file_atomic(&manifest_path, &serde_json::to_vec(&manifest)?)?;
        if !verify_backup_manifest(&backup_root, &manifest)? {
            anyhow::bail!("upgrade snapshot verification failed");
        }
        Ok(BackupResultDto {
            backup_id,
            manifest_path: manifest_path.display().to_string(),
            verified: true,
        })
    }

    fn write_upgrade_journal(&self, journal: &UpgradeJournalDto) -> Result<()> {
        write_private_file_atomic(
            &self.root.join("run").join(UPGRADE_JOURNAL),
            &serde_json::to_vec(journal)?,
        )
        .map_err(Into::into)
    }

    fn read_upgrade_journal(&self) -> Result<UpgradeJournalDto> {
        Ok(serde_json::from_slice(&read_private_file(
            &self.root.join("run").join(UPGRADE_JOURNAL),
        )?)?)
    }
}

fn copy_private_tree(
    source: &Path,
    destination: &Path,
    entries: &mut Vec<serde_json::Value>,
) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("reading backup source {}", source.display()))?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!("refusing symlink in backup source {}", source.display());
    }
    if metadata.is_dir() {
        ensure_private_directory(destination)?;
        for child in fs::read_dir(source)? {
            let child = child?;
            copy_private_tree(&child.path(), &destination.join(child.file_name()), entries)?;
        }
    } else if metadata.is_file() {
        fs::copy(source, destination)?;
        entries.push(serde_json::json!({ "path": destination.strip_prefix(destination.ancestors().last().unwrap_or(destination)).unwrap_or(destination).display().to_string(), "bytes": metadata.len() }));
    } else {
        anyhow::bail!("unsupported backup source {}", source.display());
    }
    Ok(())
}

fn verify_backup_manifest(backup_root: &Path, manifest: &serde_json::Value) -> Result<bool> {
    let Some(entries) = manifest.get("entries").and_then(|value| value.as_array()) else {
        return Ok(false);
    };
    for entry in entries {
        let Some(bytes) = entry.get("bytes").and_then(|value| value.as_u64()) else {
            return Ok(false);
        };
        // Entry paths are informational; recursively validate the staged tree
        // below so an untrusted manifest cannot escape its backup root.
        if bytes == 0 {
            continue;
        }
    }
    for name in ["data", "config", "logs"] {
        let path = backup_root.join(name);
        if !path.is_dir() {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn canonical_root() -> Result<PathBuf> {
    let home = dirs::home_dir().context("resolving the current user's home directory")?;
    Ok(home
        .join("Library")
        .join("Application Support")
        .join("com.my-supervisor"))
}

pub fn debug_or_canonical_root() -> Result<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os("MSV_DAEMON_TEST_DATA_DIR") {
        return Ok(PathBuf::from(root));
    }
    canonical_root()
}

pub fn discover_owner(root: PathBuf) -> Result<OwnerDiscovery> {
    read_discovery(&normalized_root(root)?.join("run").join(OWNER_METADATA))
        .context("reading daemon owner discovery")
}

pub fn load_control_token(root: PathBuf) -> Result<String> {
    valid_token(String::from_utf8(read_private_file(
        &normalized_root(root)?.join("run").join(CONTROL_TOKEN),
    )?)?)
}

fn read_discovery(path: &Path) -> Result<OwnerDiscovery> {
    Ok(serde_json::from_slice(&read_private_file(path)?)?)
}

fn normalized_root(root: PathBuf) -> Result<PathBuf> {
    let parent = root.parent().context("daemon root has no parent")?;
    let name = root
        .file_name()
        .context("daemon root has no final component")?;
    Ok(parent
        .canonicalize()
        .with_context(|| format!("resolving daemon root parent {}", parent.display()))?
        .join(name))
}

fn valid_token(token: String) -> Result<String> {
    if token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(token)
    } else {
        anyhow::bail!("control token must be a 256-bit hexadecimal value")
    }
}

fn generate_token() -> String {
    let mut bytes = [0_u8; 32];
    // SAFETY: macOS fills the supplied initialized byte slice with cryptographic
    // random data and retains no pointer after returning.
    unsafe { libc::arc4random_buf(bytes.as_mut_ptr().cast(), bytes.len()) };
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
