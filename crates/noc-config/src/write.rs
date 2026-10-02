use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use crate::ConfigError;

/// Writes `contents` to a new file at `path`, creating parent directories.
///
/// Fails with [`ConfigError::AlreadyExists`] if `path` exists, unless `force` is set.
pub(crate) fn write_new(path: &Path, contents: &str, force: bool) -> Result<(), ConfigError> {
    create_parent(path)?;
    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        // Checks and creates in one step, so a file that appears meanwhile is not clobbered.
        options.create_new(true);
    }
    let write_error = |source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    };
    let mut file = options.open(path).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            ConfigError::AlreadyExists {
                path: path.to_path_buf(),
            }
        } else {
            write_error(error)
        }
    })?;
    file.write_all(contents.as_bytes()).map_err(write_error)
}

/// Replaces the file at `path` with `contents` atomically: through a temporary file in the
/// same directory, which takes the old file's permissions, then a rename. A symbolic link is
/// followed, so the file it points to is replaced and the link stays. Creates parent
/// directories if the file is new.
pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), ConfigError> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    create_parent(&target)?;
    let write_error = |source| ConfigError::Write {
        path: target.clone(),
        source,
    };
    let mode = fs::metadata(&target).map_or(0o600, |metadata| metadata.permissions().mode());
    let temporary = temporary_path(&target);
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(contents)?;
            // The mode on creation is masked by the umask; the old file's mode is not.
            file.set_permissions(fs::Permissions::from_mode(mode & 0o7777))?;
            file.sync_all()
        });
    let result = written.and_then(|()| fs::rename(&temporary, &target));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(write_error)
}

/// A hidden name next to `path` that no other writer uses.
fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    path.with_file_name(format!(".{name}.{}.{nanos:x}.tmp", std::process::id()))
}

fn create_parent(path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}
