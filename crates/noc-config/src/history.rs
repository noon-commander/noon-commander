use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ConfigError;
use crate::hosts::read;
use crate::write::write_atomic;

/// The top of `history.toml`, before the commands.
const HEADER: &str = "\
# Commands typed on the command line of Noon Commander (! and :), oldest first, each with the
# host it ran on. Noon Commander writes this file again after each command; comments are not
# kept.

";

/// `history.toml`: the commands of the command line, oldest first (ADR 0019).
///
/// Unknown keys and types are errors, so a typo does not silently fall back to a default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    #[serde(default, rename = "command", skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<HistoryEntry>,
}

/// One `[[command]]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    /// The command as typed; it may span lines.
    pub command: String,
    /// The alias of the host it ran on; none for this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Where it ran, as the panel showed it: a local path, or a path on the host, empty for its
    /// home directory.
    #[serde(default)]
    pub dir: String,
    /// When it ran, in seconds since the Unix epoch.
    #[serde(default)]
    pub time: i64,
}

impl HistoryEntry {
    /// Whether this is `command` on `host`: the same entry, wherever and whenever it ran.
    pub fn is(&self, host: Option<&str>, command: &str) -> bool {
        self.host.as_deref() == host && self.command == command
    }
}

impl History {
    /// Loads `history.toml` from `path`; a missing file yields no commands.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match read(path)? {
            Some(text) => Self::from_toml(&text, path),
            None => Ok(Self::default()),
        }
    }

    /// Parses TOML text; `origin` is only used in errors.
    pub fn from_toml(text: &str, origin: &Path) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: origin.to_path_buf(),
            source: Box::new(source),
        })
    }

    /// The text of `history.toml`: the header, then the commands.
    fn to_text(&self) -> Result<String, toml::ser::Error> {
        Ok(format!("{HEADER}{}", toml::to_string(self)?))
    }
}

/// Adds `entry` to the file at `path` as the newest command, in place of the same command on
/// the same host, and keeps the newest `size` commands. Returns the commands as the file now
/// holds them.
///
/// Like [`remove_command`], it reads the file again first, so commands that another Noon
/// Commander added meanwhile are kept; a file that is not valid is left alone. The file is
/// written whole each time, so comments in it are not kept. It is replaced atomically, through
/// a symbolic link if it is one, and is new with mode 0600, since commands may hold secrets.
///
/// Blocking: call it from a blocking thread.
pub fn add_command(path: &Path, entry: &HistoryEntry, size: usize) -> Result<History, ConfigError> {
    let mut history = History::load(path)?;
    history
        .commands
        .retain(|kept| !kept.is(entry.host.as_deref(), &entry.command));
    history.commands.push(entry.clone());
    let excess = history.commands.len().saturating_sub(size);
    history.commands.drain(..excess);
    write(path, &history)?;
    Ok(history)
}

/// Removes `command` on `host` from the file at `path`; a missing one is not an error. Returns
/// the commands as the file now holds them. See [`add_command`].
///
/// Blocking: call it from a blocking thread.
pub fn remove_command(
    path: &Path,
    host: Option<&str>,
    command: &str,
) -> Result<History, ConfigError> {
    let mut history = History::load(path)?;
    let count = history.commands.len();
    history.commands.retain(|kept| !kept.is(host, command));
    if history.commands.len() != count {
        write(path, &history)?;
    }
    Ok(history)
}

fn write(path: &Path, history: &History) -> Result<(), ConfigError> {
    let text = history.to_text().map_err(|error| ConfigError::Write {
        path: path.to_path_buf(),
        source: io::Error::other(error),
    })?;
    write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn entry(command: &str, host: Option<&str>, time: i64) -> HistoryEntry {
        HistoryEntry {
            command: command.to_owned(),
            host: host.map(str::to_owned),
            dir: "/srv".to_owned(),
            time,
        }
    }

    fn commands(history: &History) -> Vec<(&str, Option<&str>)> {
        history
            .commands
            .iter()
            .map(|entry| (entry.command.as_str(), entry.host.as_deref()))
            .collect()
    }

    #[test]
    fn a_missing_file_has_no_commands() {
        let dir = tempfile::tempdir().unwrap();
        let history = History::load(&dir.path().join("history.toml")).unwrap();
        assert_eq!(history, History::default());
    }

    #[test]
    fn adds_newest_last_once_per_host_and_keeps_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state/history.toml");
        add_command(&path, &entry("make", None, 1), 3).unwrap();
        add_command(&path, &entry("make", Some("web"), 2), 3).unwrap();
        add_command(
            &path,
            &entry("for f in *; do\n  echo \"$f\"\ndone", None, 3),
            3,
        )
        .unwrap();
        let history = add_command(&path, &entry("make", None, 4), 3).unwrap();
        assert_eq!(
            commands(&history),
            [
                ("make", Some("web")),
                ("for f in *; do\n  echo \"$f\"\ndone", None),
                ("make", None)
            ],
            "the same command on another host is another entry"
        );
        let history = add_command(&path, &entry("ls", None, 5), 3).unwrap();
        assert_eq!(
            commands(&history)[0],
            ("for f in *; do\n  echo \"$f\"\ndone", None)
        );
        assert_eq!(History::load(&path).unwrap(), history, "it reads back");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# Commands typed"), "{text}");
    }

    #[test]
    fn removes_a_command_of_one_host() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.toml");
        add_command(&path, &entry("make", None, 1), 10).unwrap();
        add_command(&path, &entry("make", Some("web"), 2), 10).unwrap();
        let history = remove_command(&path, Some("web"), "make").unwrap();
        assert_eq!(commands(&history), [("make", None)]);
        assert_eq!(remove_command(&path, Some("db"), "make").unwrap(), history);
    }

    #[test]
    fn an_invalid_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.toml");
        std::fs::write(&path, "[[command]]\ncmd = \"ls\"\n").unwrap();
        assert!(matches!(
            add_command(&path, &entry("ls", None, 1), 10),
            Err(ConfigError::Parse { .. })
        ));
        assert!(std::fs::read_to_string(&path).unwrap().contains("cmd ="));
    }
}
