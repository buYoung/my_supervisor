use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const REQUIRED_SIDECARS: [&str; 4] = ["msv-daemon", "msv", "msv-log-proxy", "msv-group-reaper"];
const SECURITY_CONTRACT_VERSION: &str = "MSV_SECURITY_CONTRACT_V1";

fn main() {
    emit_security_contract();
    prepare_release_sidecars();
    tauri_build::build();
}

/// Bind the precise Tauri security inputs and the pre-build source manifest to
/// the desktop executable. This is build evidence only: an unsigned Mach-O
/// does not gain hardened-runtime or embedded-entitlement guarantees from it.
fn emit_security_contract() {
    let desktop_dir = PathBuf::from(required_env("CARGO_MANIFEST_DIR"));
    let config_path = desktop_dir.join("tauri.conf.json");
    println!("cargo:rerun-if-changed={}", config_path.display());

    let config_bytes = fs::read(&config_path)
        .unwrap_or_else(|error| panic!("reading {} failed: {error}", config_path.display()));
    let config: serde_json::Value = serde_json::from_slice(&config_bytes)
        .unwrap_or_else(|error| panic!("parsing {} failed: {error}", config_path.display()));
    let csp = config
        .pointer("/app/security/csp")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("{} must define app.security.csp", config_path.display()));
    let hardened_runtime = config
        .pointer("/bundle/macOS/hardenedRuntime")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_else(|| {
            panic!(
                "{} must define bundle.macOS.hardenedRuntime",
                config_path.display()
            )
        });
    let entitlement_relative_path = config
        .pointer("/bundle/macOS/entitlements")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| {
            panic!(
                "{} must define bundle.macOS.entitlements",
                config_path.display()
            )
        });
    let entitlement_path = desktop_dir.join(entitlement_relative_path);
    println!("cargo:rerun-if-changed={}", entitlement_path.display());
    let entitlement_bytes = fs::read(&entitlement_path).unwrap_or_else(|error| {
        panic!(
            "reading entitlement input {} failed: {error}",
            entitlement_path.display()
        )
    });

    let provenance_digest = source_provenance_digest(&desktop_dir);
    let marker = format!(
        "{SECURITY_CONTRACT_VERSION}|csp_hex={}|hardened_runtime={hardened_runtime}|entitlement_path={entitlement_relative_path}|entitlement_sha256={}|entitlement_hex={}|source_provenance_sha256={provenance_digest}",
        hex_encode(csp.as_bytes()),
        file_sha256(&entitlement_path),
        hex_encode(&entitlement_bytes),
    );
    println!("cargo:rustc-env=MSV_SECURITY_CONTRACT_MARKER={marker}");
}

fn source_provenance_digest(desktop_dir: &Path) -> String {
    let workspace_root = desktop_dir
        .parent()
        .and_then(Path::parent)
        .expect("desktop crate is nested under the workspace root");
    let target_dir = cargo_target_dir(workspace_root, desktop_dir);
    let manifest_path = target_dir.join("source-manifest.tsv");
    if manifest_path.exists() {
        println!("cargo:rerun-if-changed={}", manifest_path.display());
        let manifest_bytes = fs::read(&manifest_path).unwrap_or_else(|error| {
            panic!(
                "reading source provenance manifest {} failed: {error}",
                manifest_path.display()
            )
        });
        let manifest = std::str::from_utf8(&manifest_bytes).unwrap_or_else(|error| {
            panic!(
                "source provenance manifest {} is not UTF-8: {error}",
                manifest_path.display()
            )
        });
        let digest = manifest
            .trim_end_matches('\n')
            .rsplit_once("\nmanifest_sha256\t")
            .map(|(_, digest)| digest)
            .unwrap_or_else(|| {
                panic!(
                    "source provenance manifest {} is missing its digest footer",
                    manifest_path.display()
                )
            });
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            panic!(
                "source provenance manifest {} has an invalid digest footer",
                manifest_path.display()
            );
        }
        digest.to_ascii_lowercase()
    } else if required_env("PROFILE") == "release" {
        panic!(
            "release desktop builds require {}. Run scripts/release/capture-source-provenance.sh create <CARGO_TARGET_DIR>/source-manifest.tsv before building.",
            manifest_path.display()
        );
    } else {
        "unbound-debug-build".to_string()
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn file_sha256(path: &Path) -> String {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .unwrap_or_else(|error| panic!("running shasum for {} failed: {error}", path.display()));
    if !output.status.success() {
        panic!(
            "shasum for {} failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let digest = String::from_utf8(output.stdout)
        .unwrap_or_else(|error| {
            panic!(
                "shasum output for {} was not UTF-8: {error}",
                path.display()
            )
        })
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("shasum did not return a digest for {}", path.display()))
        .to_ascii_lowercase();
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        panic!("shasum returned an invalid digest for {}", path.display());
    }
    digest
}

/// Tauri expects sidecars next to `tauri.conf.json`, suffixed with the exact
/// target triple. The release preparation script stages the server-release
/// artifacts there; this build step verifies that exact set instead of racing
/// Cargo output writes or compiling a mismatched helper profile.
fn prepare_release_sidecars() {
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");
    println!("cargo:rerun-if-env-changed=HOST");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=PROFILE");

    let target = required_env("TARGET");
    let profile = required_env("PROFILE");
    let helper_artifact_dir = cargo_artifact_dir(&target, &required_env("HOST"), &profile);
    let desktop_dir = PathBuf::from(required_env("CARGO_MANIFEST_DIR"));
    let sidecar_dir = desktop_dir.join("binaries");
    fs::create_dir_all(&sidecar_dir).expect("creating the Tauri sidecar directory");

    for sidecar_name in REQUIRED_SIDECARS {
        let source = helper_artifact_dir.join(sidecar_name);
        println!("cargo:rerun-if-changed={}", source.display());
        let sidecar = sidecar_dir.join(format!("{sidecar_name}-{target}"));
        if profile == "release" {
            validate_executable(&source, sidecar_name);
            validate_executable(&sidecar, sidecar_name);
            let source_bytes = fs::read(&source).unwrap_or_else(|error| {
                panic!(
                    "reading release helper {} failed: {error}",
                    source.display()
                )
            });
            let sidecar_bytes = fs::read(&sidecar).unwrap_or_else(|error| {
                panic!(
                    "reading prepared sidecar {} failed: {error}",
                    sidecar.display()
                )
            });
            if source_bytes != sidecar_bytes {
                panic!(
                    "prepared sidecar {sidecar_name} does not match {}. Run `scripts/release/prepare-macos-sidecars.sh` after `cargo msv-release` before `cargo tauri build`.",
                    source.display()
                );
            }
        } else if source.exists() {
            validate_executable(&source, sidecar_name);
            fs::copy(&source, &sidecar).unwrap_or_else(|error| {
                panic!(
                    "copying {sidecar_name} from {} to {} failed: {error}",
                    source.display(),
                    sidecar.display()
                )
            });
        } else {
            write_development_placeholder(&sidecar, sidecar_name);
        }
        validate_executable(&sidecar, sidecar_name);
    }
}

/// `cargo check` and a first `cargo tauri dev` do not compile dependency bin
/// targets.  Keep Tauri's config validation deterministic in that case, while
/// ensuring an attempted detached launch fails with an explicit instruction.
/// Release builds never use this path: they require the real same-profile
/// helper artifact above.
fn write_development_placeholder(path: &Path, helper_name: &str) {
    fs::write(
        path,
        format!(
            "#!/bin/sh\necho '{helper_name} is unavailable; run cargo build -p my-supervisor-platform-macos before starting detached processes' >&2\nexit 127\n"
        ),
    )
    .unwrap_or_else(|error| panic!("writing development placeholder {} failed: {error}", path.display()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap_or_else(|error| {
            panic!(
                "marking development placeholder {} executable failed: {error}",
                path.display()
            )
        });
    }
}

fn cargo_artifact_dir(target: &str, host: &str, profile: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(required_env("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("desktop crate is nested under the workspace root");
    let target_dir = cargo_target_dir(workspace_root, &manifest_dir);
    let target_dir = if target == host {
        target_dir
    } else {
        target_dir.join(target)
    };
    target_dir.join(profile)
}

/// Cargo preserves a relative `CARGO_TARGET_DIR`. Root release commands use a
/// workspace-relative value, while the documented Tauri command runs from the
/// desktop crate with `../../target/...`; accept the existing target location
/// in either invocation without changing the artifact contract.
fn cargo_target_dir(workspace_root: &Path, manifest_dir: &Path) -> PathBuf {
    let Some(target_dir) = env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) else {
        return workspace_root.join("target");
    };
    if target_dir.is_absolute() {
        return target_dir;
    }
    let workspace_relative = workspace_root.join(&target_dir);
    let manifest_relative = manifest_dir.join(&target_dir);
    if workspace_relative.exists() || !manifest_relative.exists() {
        workspace_relative
    } else {
        manifest_relative
    }
}

fn validate_executable(path: &Path, helper_name: &str) {
    let metadata = fs::metadata(path).unwrap_or_else(|error| {
        panic!(
            "required release helper {helper_name} is missing at {}: {error}. Run `cargo msv-release` and `scripts/release/prepare-macos-sidecars.sh` before `cargo tauri build`.",
            path.display()
        )
    });
    if !metadata.is_file() {
        panic!(
            "required release helper {helper_name} at {} is not a regular file",
            path.display()
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if metadata.permissions().mode() & 0o111 == 0 {
            panic!(
                "required release helper {helper_name} at {} is not executable",
                path.display()
            );
        }
    }
}

fn required_env(name: &str) -> String {
    env::var(name)
        .unwrap_or_else(|_| panic!("Cargo did not provide required build variable {name}"))
}
