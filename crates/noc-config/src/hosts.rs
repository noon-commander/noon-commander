use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

use crate::ConfigError;
use crate::write::{write_atomic, write_new};

/// Commented template of `hosts.toml` written by `noc config init`; it holds no hosts.
pub const DEFAULT_HOSTS: &str = include_str!("../assets/default-hosts.toml");

/// Settings from `hosts.toml`, keyed by host name: one table per host.
///
/// Unknown keys and types are errors, so a typo does not silently fall back to a default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Hosts {
    pub hosts: BTreeMap<String, HostConfig>,
}

/// One host table. `type` picks the variant, which decides what else the table may hold.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum HostConfig {
    /// `type = "sftp"`: decorations for a Host alias from `ssh_config`. The table never
    /// defines the host; how to reach it stays in `ssh_config`.
    Sftp(SftpHost),
}

/// The settings of an SFTP host.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SftpHost {
    /// Name shown for the host instead of its alias.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Remote directory opened on connect, instead of the remote home directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_dir: Option<String>,
    /// Where the other panel goes when the host is opened: a local path, `/…` or `~/…`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_dir: Option<String>,
    /// Whether opening the host again returns to the last directory shown on it in this
    /// session, before `start_dir`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub remember_dir: bool,
}

impl HostConfig {
    /// The value of `type`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Sftp(_) => "sftp",
        }
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Sftp(host) => host.label.as_deref(),
        }
    }

    pub fn start_dir(&self) -> Option<&str> {
        match self {
            Self::Sftp(host) => host.start_dir.as_deref(),
        }
    }

    pub fn other_dir(&self) -> Option<&str> {
        match self {
            Self::Sftp(host) => host.other_dir.as_deref(),
        }
    }

    pub fn remember_dir(&self) -> bool {
        match self {
            Self::Sftp(host) => host.remember_dir,
        }
    }

    /// Whether every setting has its default, which leaves nothing to write.
    pub fn is_default(&self) -> bool {
        match self {
            Self::Sftp(host) => *host == SftpHost::default(),
        }
    }
}

/// The local path that `other_dir` names: an absolute path, or `~` or `~/…` under `home`.
/// `None` for anything else, which later versions may give a meaning, such as `host:path`.
pub fn local_dir(text: &str, home: &Path) -> Option<PathBuf> {
    if text.starts_with('/') {
        return Some(PathBuf::from(text));
    }
    match text.strip_prefix('~')? {
        "" => Some(home.to_path_buf()),
        rest => rest.strip_prefix('/').map(|rest| home.join(rest)),
    }
}

impl Hosts {
    /// Loads `hosts.toml` from `path`; a missing file yields no hosts.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match read(path)? {
            Some(text) => Self::from_toml(&text, path),
            None => Ok(Self::default()),
        }
    }

    /// Parses TOML text and checks the values; `origin` is only used in errors.
    pub fn from_toml(text: &str, origin: &Path) -> Result<Self, ConfigError> {
        let hosts: Self = toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: origin.to_path_buf(),
            source: Box::new(source),
        })?;
        for (name, host) in &hosts.hosts {
            if let Some(dir) = host.other_dir()
                && local_dir(dir, Path::new("/")).is_none()
            {
                return Err(ConfigError::InvalidHost {
                    path: origin.to_path_buf(),
                    host: name.clone(),
                    reason: format!("`other_dir = {dir:?}` must start with / or ~/"),
                });
            }
        }
        Ok(hosts)
    }

    pub fn get(&self, name: &str) -> Option<&HostConfig> {
        self.hosts.get(name)
    }
}

/// Writes the table of the host `name` in the file at `path`, or removes it if `host` is
/// `None` or all defaults. Comments, formatting, and the other tables stay as they are; the
/// file is read again first, so changes made meanwhile are kept. A file that is not valid
/// TOML is left alone. The file is replaced atomically, through a symbolic link if it is one.
///
/// Blocking: call it from a blocking thread.
pub fn save_host(path: &Path, name: &str, host: Option<&HostConfig>) -> Result<(), ConfigError> {
    let text = read(path)?.unwrap_or_default();
    let mut document: DocumentMut = text.parse().map_err(|source| ConfigError::Edit {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;
    match host.filter(|host| !host.is_default()) {
        None => {
            document.remove(name);
        }
        Some(host) => {
            let present = document.get(name).is_some_and(Item::is_table_like);
            if !present {
                // Comments at the end of the file stay above the new table, not below it.
                let mut table = Table::new();
                let trailing = document.trailing().as_str().unwrap_or_default().to_owned();
                if !trailing.trim().is_empty() {
                    table
                        .decor_mut()
                        .set_prefix(format!("{}\n\n", trailing.trim_end()));
                    document.set_trailing("");
                }
                document.insert(name, Item::Table(table));
            }
            if let Some(table) = document.get_mut(name).and_then(Item::as_table_like_mut) {
                fill(table, host);
            }
        }
    }
    write_atomic(path, document.to_string().as_bytes())
}

/// Sets the keys of `host` in `table`: changed values keep the comments around them, and
/// values that did not change keep their formatting too.
fn fill(table: &mut dyn TableLike, host: &HostConfig) {
    set(table, "type", Some(Value::from(host.kind())));
    match host {
        HostConfig::Sftp(sftp) => {
            let text = |value: &Option<String>| value.as_deref().map(Value::from);
            set(table, "label", text(&sftp.label));
            set(table, "start_dir", text(&sftp.start_dir));
            set(table, "other_dir", text(&sftp.other_dir));
            set(
                table,
                "remember_dir",
                sftp.remember_dir.then(|| Value::from(true)),
            );
        }
    }
}

fn set(table: &mut dyn TableLike, key: &str, value: Option<Value>) {
    let Some(mut value) = value else {
        table.remove(key);
        return;
    };
    if let Some(old) = table.get(key).and_then(Item::as_value) {
        let same = match (old, &value) {
            (Value::String(old), Value::String(new)) => old.value() == new.value(),
            (Value::Boolean(old), Value::Boolean(new)) => old.value() == new.value(),
            _ => false,
        };
        if same {
            return;
        }
        *value.decor_mut() = old.decor().clone();
    }
    table.insert(key, Item::Value(value));
}

/// The text of the file at `path`, or `None` if there is none.
fn read(path: &Path) -> Result<Option<String>, ConfigError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Writes [`DEFAULT_HOSTS`] to `path`, creating parent directories.
///
/// Fails with [`ConfigError::AlreadyExists`] if `path` exists, unless `force` is set.
pub fn write_default_hosts(path: &Path, force: bool) -> Result<(), ConfigError> {
    write_new(path, DEFAULT_HOSTS, force)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};

    use super::*;

    const ORIGIN: &str = "/cfg/noc/hosts.toml";

    fn parse(text: &str) -> Hosts {
        Hosts::from_toml(text, Path::new(ORIGIN)).unwrap()
    }

    fn sftp(host: SftpHost) -> HostConfig {
        HostConfig::Sftp(host)
    }

    fn assert_parse_error(text: &str) {
        match Hosts::from_toml(text, Path::new(ORIGIN)) {
            Err(ConfigError::Parse { path, .. }) => assert_eq!(path, Path::new(ORIGIN)),
            other => panic!("expected a parse error for {text:?}, got {other:?}"),
        }
    }

    #[test]
    fn parses_typed_tables() {
        let hosts = parse(
            "[\"prod-web\"]\ntype = \"sftp\"\nlabel = \"Prod\"\nstart_dir = \"/var/www\"\n\
             other_dir = \"~/site\"\nremember_dir = true\n\n\
             [\"web.example.com\"]\ntype = \"sftp\"\n",
        );
        assert_eq!(
            hosts.get("prod-web"),
            Some(&sftp(SftpHost {
                label: Some("Prod".to_owned()),
                start_dir: Some("/var/www".to_owned()),
                other_dir: Some("~/site".to_owned()),
                remember_dir: true,
            }))
        );
        assert_eq!(
            hosts.get("web.example.com"),
            Some(&sftp(SftpHost::default()))
        );
        assert_eq!(parse(""), Hosts::default());
        assert_eq!(parse(DEFAULT_HOSTS), Hosts::default());
    }

    #[test]
    fn the_template_example_is_valid() {
        let uncommented: Vec<&str> = DEFAULT_HOSTS
            .lines()
            .skip_while(|line| *line != "# [\"prod-web\"]")
            .filter_map(|line| line.strip_prefix("# "))
            .collect();
        let hosts = parse(&uncommented.join("\n"));
        let host = hosts.get("prod-web").unwrap();
        assert_eq!(host.label(), Some("Prod"));
        assert_eq!(host.other_dir(), Some("~/projects/site"));
        assert!(host.remember_dir());
    }

    #[test]
    fn rejects_what_the_schema_does_not_know() {
        for text in [
            "[web]\nlabel = \"Web\"",
            "[web]\ntype = \"ftp\"",
            "[web]\ntype = \"sftp\"\nlable = \"Web\"",
            "[web]\ntype = \"sftp\"\nargs = [\"-v\"]",
            "[web]\ntype = \"sftp\"\nremember_dir = \"yes\"",
            "web = \"Web\"",
        ] {
            assert_parse_error(text);
        }
    }

    #[test]
    fn other_dir_is_a_local_path_for_now() {
        for dir in ["/srv", "~", "~/site"] {
            parse(&format!("[web]\ntype = \"sftp\"\nother_dir = \"{dir}\""));
        }
        for dir in ["site", "web:/srv", "~user/site", ""] {
            let text = format!("[web]\ntype = \"sftp\"\nother_dir = \"{dir}\"");
            match Hosts::from_toml(&text, Path::new(ORIGIN)) {
                Err(ConfigError::InvalidHost { host, .. }) => assert_eq!(host, "web"),
                other => panic!("expected an invalid host for {dir:?}, got {other:?}"),
            }
        }
        let home = Path::new("/home/u");
        assert_eq!(local_dir("~", home), Some(PathBuf::from("/home/u")));
        assert_eq!(local_dir("~/a", home), Some(PathBuf::from("/home/u/a")));
        assert_eq!(local_dir("/a", home), Some(PathBuf::from("/a")));
        assert_eq!(local_dir("a", home), None);
    }

    #[test]
    fn a_missing_file_holds_no_hosts() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            Hosts::load(&tmp.path().join("hosts.toml")).unwrap(),
            Hosts::default()
        );
    }

    fn saved(text: &str, name: &str, host: Option<&HostConfig>) -> String {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hosts.toml");
        fs::write(&path, text).unwrap();
        save_host(&path, name, host).unwrap();
        fs::read_to_string(&path).unwrap()
    }

    #[test]
    fn saving_keeps_comments_and_other_tables() {
        let text = "# My hosts\n\n\
                    [db]\ntype = \"sftp\" # the database\nlabel = \"DB\"\n\n\
                    # The web server.\n\
                    [\"web.example.com\"]\ntype = \"sftp\"\nlabel = \"Web\" # shown\nstart_dir = '/srv'\n";
        let host = sftp(SftpHost {
            label: Some("Site".to_owned()),
            start_dir: Some("/srv".to_owned()),
            other_dir: Some("~/site".to_owned()),
            remember_dir: true,
        });
        assert_eq!(
            saved(text, "web.example.com", Some(&host)),
            "# My hosts\n\n\
             [db]\ntype = \"sftp\" # the database\nlabel = \"DB\"\n\n\
             # The web server.\n\
             [\"web.example.com\"]\ntype = \"sftp\"\nlabel = \"Site\" # shown\nstart_dir = '/srv'\n\
             other_dir = \"~/site\"\nremember_dir = true\n"
        );
    }

    #[test]
    fn saving_adds_a_quoted_table_and_reads_back() {
        let host = sftp(SftpHost {
            label: Some("Web".to_owned()),
            ..SftpHost::default()
        });
        let text = saved("# Hosts\n", "10.0.0.5", Some(&host));
        assert_eq!(
            text,
            "# Hosts\n\n[\"10.0.0.5\"]\ntype = \"sftp\"\nlabel = \"Web\"\n"
        );
        assert_eq!(parse(&text).get("10.0.0.5"), Some(&host));
    }

    #[test]
    fn defaults_leave_the_file() {
        let text = "[db]\ntype = \"sftp\"\n\n[web]\ntype = \"sftp\"\nlabel = \"Web\"\nremember_dir = true\n";
        let default = sftp(SftpHost::default());
        assert_eq!(
            saved(text, "web", Some(&default)),
            "[db]\ntype = \"sftp\"\n"
        );
        assert_eq!(saved(text, "web", None), "[db]\ntype = \"sftp\"\n");
        let label_only = sftp(SftpHost {
            label: Some("Web".to_owned()),
            ..SftpHost::default()
        });
        assert_eq!(
            saved(text, "web", Some(&label_only)),
            "[db]\ntype = \"sftp\"\n\n[web]\ntype = \"sftp\"\nlabel = \"Web\"\n"
        );
    }

    #[test]
    fn saving_creates_the_file_and_its_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config/noc/hosts.toml");
        let host = sftp(SftpHost {
            start_dir: Some("/srv".to_owned()),
            ..SftpHost::default()
        });
        save_host(&path, "web", Some(&host)).unwrap();
        assert_eq!(Hosts::load(&path).unwrap().get("web"), Some(&host));
    }

    #[test]
    fn saving_leaves_invalid_toml_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hosts.toml");
        fs::write(&path, "[web\n").unwrap();
        let host = sftp(SftpHost::default());
        assert!(matches!(
            save_host(&path, "db", Some(&host)),
            Err(ConfigError::Edit { .. })
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "[web\n");
    }

    #[test]
    fn saving_keeps_the_mode_and_follows_a_link() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("dotfiles/hosts.toml");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, "").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        let link = tmp.path().join("hosts.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let host = sftp(SftpHost {
            label: Some("Web".to_owned()),
            ..SftpHost::default()
        });
        save_host(&link, "web", Some(&host)).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(Hosts::load(&real).unwrap().get("web"), Some(&host));
        let mode = fs::metadata(&real).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
        let names: Vec<_> = fs::read_dir(real.parent().unwrap()).unwrap().collect();
        assert_eq!(names.len(), 1, "no temporary file is left");
    }
}
