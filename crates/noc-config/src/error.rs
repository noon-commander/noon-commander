use std::path::PathBuf;

/// Errors from resolving directories, loading the configuration, and writing files.
///
/// Messages name the path only; the underlying error is available as the
/// [`source`](std::error::Error::source).
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// No absolute home directory is known.
    #[error("cannot determine the home directory")]
    NoHomeDir,
    /// A file or directory exists but cannot be read.
    #[error("cannot read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The configuration is not valid TOML or does not match the schema.
    #[error("invalid config {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
    /// `config.toml` has a `[hosts]` table, which moved to `hosts.toml`.
    #[error(
        "{path}: [hosts] moved to hosts.toml in the same directory; move each \
         [hosts.\"alias\"] table there as [\"alias\"] with type = \"sftp\", without `args`"
    )]
    HostsMoved { path: PathBuf },
    /// A host table in `hosts.toml` has a value that its type does not allow.
    #[error("invalid host {host:?} in {path}: {reason}")]
    InvalidHost {
        path: PathBuf,
        host: String,
        reason: String,
    },
    /// `hosts.toml` is not valid TOML, so it is not changed.
    #[error("cannot edit {path}")]
    Edit {
        path: PathBuf,
        #[source]
        source: Box<toml_edit::TomlError>,
    },
    /// [`write_default_config`](crate::write_default_config) found an existing file and was
    /// not asked to overwrite it.
    #[error("{path} already exists; use --force to overwrite it")]
    AlreadyExists { path: PathBuf },
    /// A file or directory cannot be created or written.
    #[error("cannot write {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The runtime directory is not a private directory of the current user.
    #[error("unsafe runtime directory {path}: {reason}")]
    UnsafeRuntimeDir { path: PathBuf, reason: String },
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;
    use std::io;
    use std::path::PathBuf;

    use super::ConfigError;

    fn path() -> PathBuf {
        PathBuf::from("/cfg/noc/config.toml")
    }

    #[test]
    fn messages_leave_the_cause_to_the_source_chain() {
        let toml_error = toml::from_str::<toml::Table>("key = ").unwrap_err();
        let cases = [
            (
                ConfigError::Read {
                    path: path(),
                    source: io::Error::from(io::ErrorKind::PermissionDenied),
                },
                "cannot read /cfg/noc/config.toml",
            ),
            (
                ConfigError::Parse {
                    path: path(),
                    source: Box::new(toml_error),
                },
                "invalid config /cfg/noc/config.toml",
            ),
            (
                ConfigError::Write {
                    path: path(),
                    source: io::Error::from(io::ErrorKind::PermissionDenied),
                },
                "cannot write /cfg/noc/config.toml",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
            assert!(error.source().is_some(), "{error:?}");
        }
    }

    #[test]
    fn messages_without_a_source() {
        let cases = [
            (
                ConfigError::NoHomeDir,
                "cannot determine the home directory",
            ),
            (
                ConfigError::AlreadyExists { path: path() },
                "/cfg/noc/config.toml already exists; use --force to overwrite it",
            ),
            (
                ConfigError::UnsafeRuntimeDir {
                    path: PathBuf::from("/tmp/noc-501"),
                    reason: "it is a symbolic link".to_owned(),
                },
                "unsafe runtime directory /tmp/noc-501: it is a symbolic link",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
            assert!(error.source().is_none(), "{error:?}");
        }
    }
}
