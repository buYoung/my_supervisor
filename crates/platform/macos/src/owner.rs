//! Private filesystem ownership primitives for the user-scoped daemon root.
//!
//! The held descriptor is deliberately owned by this platform crate so the
//! daemon composition layer cannot accidentally replace it with a process-local
//! mutex or release it before its authoritative runtime stops.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A non-blocking advisory write lock held for the lifetime of an owner.
pub struct OwnerLock {
    file: File,
}

impl OwnerLock {
    /// Claim `path` without following a replacement symlink. Contention is
    /// reported as a normal error; a dead owner releases this kernel lock.
    pub fn acquire(path: &Path) -> io::Result<Self> {
        validate_existing_components(path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "owner lock has no parent")
        })?)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        validate_private_file(&file)?;
        let mut lock = libc::flock {
            l_start: 0,
            l_len: 0,
            l_pid: 0,
            l_type: libc::F_WRLCK as libc::c_short,
            l_whence: libc::SEEK_SET as libc::c_short,
        };
        // SAFETY: `lock` is initialized and the descriptor remains owned by
        // `file` for the resulting lock guard lifetime.
        let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &mut lock) };
        if result == -1 {
            let error = io::Error::last_os_error();
            return Err(
                if matches!(error.raw_os_error(), Some(code) if code == libc::EACCES || code == libc::EAGAIN)
                {
                    io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "daemon data root is already owned",
                    )
                } else {
                    error
                },
            );
        }
        Ok(Self { file })
    }
}

impl Drop for OwnerLock {
    fn drop(&mut self) {
        let mut lock = libc::flock {
            l_start: 0,
            l_len: 0,
            l_pid: 0,
            l_type: libc::F_UNLCK as libc::c_short,
            l_whence: libc::SEEK_SET as libc::c_short,
        };
        // SAFETY: this only releases the advisory lock held by `self.file`.
        let _ = unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_SETLK, &mut lock) };
    }
}

/// Create or tighten a user-owned, non-group/world-accessible directory.
pub fn ensure_private_directory(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700))?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    validate_existing_components(path)?;
    let mut metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "state directory must be user-owned and mode 0700-equivalent",
        ));
    }
    if metadata.mode() & 0o077 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        metadata = fs::symlink_metadata(path)?;
        if metadata.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "state directory must be user-owned and mode 0700-equivalent",
            ));
        }
    }
    Ok(())
}

/// Read a regular user-only file without following its final component.
pub fn read_private_file(path: &Path) -> io::Result<Vec<u8>> {
    validate_existing_components(path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "private file has no parent")
    })?)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    validate_private_file(&file)?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    Ok(contents)
}

/// Replace a user-only file atomically after syncing both file and parent.
pub fn write_private_file_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "private file has no parent"))?;
    validate_existing_components(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "private file has no name"))?;
    let temporary = parent.join(format!(
        ".{}.{}.{}",
        name.to_string_lossy(),
        std::process::id(),
        nonce
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(contents)?;
        file.sync_all()?;
        validate_private_file(&file)?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn validate_existing_components(path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "symlinked state path component is not allowed: {}",
                    current.display()
                ),
            ));
        }
        if metadata.uid() != unsafe { libc::geteuid() }
            && metadata.uid() != 0
            && current != Path::new("/")
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "state path component has another owner: {}",
                    current.display()
                ),
            ));
        }
        if current != Path::new("/")
            && metadata.mode() & 0o022 != 0
            && metadata.mode() & 0o1000 == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "state path component is writable by another user: {}",
                    current.display()
                ),
            ));
        }
    }
    Ok(())
}

fn validate_private_file(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "state file must be a regular user-only file",
        ));
    }
    Ok(())
}
