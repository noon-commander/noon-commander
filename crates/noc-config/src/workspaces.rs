use std::collections::HashSet;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ConfigError;
use crate::hosts::read;
use crate::write::write_atomic;

/// The top of `workspaces.toml`, before the workspaces.
const HEADER: &str = "\
# Workspaces of Noon Commander: F9 → Workspace saves the tabs of both panels.
# Noon Commander writes this file again on each change; comments are not kept.

";

/// `workspaces.toml`: saved layouts of the tabs of both panels, in the order they were saved.
///
/// Unknown keys and types are errors, so a typo does not silently fall back to a default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workspaces {
    #[serde(default, rename = "workspace", skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<Workspace>,
}

/// One `[[workspace]]`: the tabs of both panels.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    /// Unique and not blank.
    pub name: String,
    /// The side whose panel has the keys.
    #[serde(default)]
    pub active: PanelSide,
    /// At least one tab.
    pub left: Vec<SavedTab>,
    /// At least one tab.
    pub right: Vec<SavedTab>,
}

/// A side of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelSide {
    #[default]
    Left,
    Right,
}

/// One tab of a panel: `[[workspace.left]]` or `[[workspace.right]]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SavedTab {
    /// TOML key `location`.
    #[serde(rename = "location")]
    pub place: Place,
    #[serde(default, skip_serializing_if = "is_default")]
    pub sort: SortBy,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub descending: bool,
    /// The name of the entry under the cursor, or the alias of the host under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// The tab that shows on its side; at most one per side, else the first shows.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub current: bool,
}

/// What a panel is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SortBy {
    #[default]
    Name,
    Extension,
    Time,
    Size,
}

/// Where a tab is, written as one string: `root` (the virtual root), `sftp` (the list of
/// hosts), a local directory as an absolute path, `~`, or `~/…`, or `host:path` on a host (an
/// empty path is the host's start directory).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub enum Place {
    Root,
    Sftp,
    /// As written: `/…`, `~`, or `~/…`.
    Local(String),
    Remote {
        host: String,
        path: String,
    },
}

impl Place {
    /// Parses the text form; `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "root" => return Some(Self::Root),
            "sftp" => return Some(Self::Sftp),
            _ => {}
        }
        if text.starts_with(['/', '~']) {
            return crate::local_dir(text, Path::new("/"))
                .is_some()
                .then(|| Self::Local(text.to_owned()));
        }
        let (host, path) = text.split_once(':')?;
        (!host.is_empty() && !host.contains('/')).then(|| Self::Remote {
            host: host.to_owned(),
            path: path.to_owned(),
        })
    }

    /// A local directory: `~` for `home`, `~/…` below it, else the absolute path. `None` if
    /// `path` is not absolute or not UTF-8.
    pub fn local(path: &Path, home: &Path) -> Option<Self> {
        if !path.is_absolute() {
            return None;
        }
        let text = path.to_str()?;
        if home.is_absolute()
            && let Ok(rest) = path.strip_prefix(home)
        {
            return Some(Self::Local(match rest.to_str()? {
                "" => "~".to_owned(),
                rest => format!("~/{rest}"),
            }));
        }
        Some(Self::Local(text.to_owned()))
    }

    /// The directory of a `Local` place, with `~` under `home`; `None` for the others.
    pub fn local_dir(&self, home: &Path) -> Option<PathBuf> {
        match self {
            Self::Local(text) => crate::local_dir(text, home),
            Self::Root | Self::Sftp | Self::Remote { .. } => None,
        }
    }
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root => f.write_str("root"),
            Self::Sftp => f.write_str("sftp"),
            Self::Local(path) => f.write_str(path),
            Self::Remote { host, path } => write!(f, "{host}:{path}"),
        }
    }
}

impl TryFrom<String> for Place {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text).ok_or_else(|| {
            format!("`{text}` is not `root`, `sftp`, a path that starts with / or ~/, or host:path")
        })
    }
}

impl From<Place> for String {
    fn from(place: Place) -> Self {
        place.to_string()
    }
}

impl Workspaces {
    /// Loads `workspaces.toml` from `path`; a missing file yields no workspaces.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match read(path)? {
            Some(text) => Self::from_toml(&text, path),
            None => Ok(Self::default()),
        }
    }

    /// Parses TOML text and checks the values; `origin` is only used in errors.
    pub fn from_toml(text: &str, origin: &Path) -> Result<Self, ConfigError> {
        let workspaces: Self = toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: origin.to_path_buf(),
            source: Box::new(source),
        })?;
        workspaces.check(origin)?;
        Ok(workspaces)
    }

    pub fn get(&self, name: &str) -> Option<&Workspace> {
        self.workspaces
            .iter()
            .find(|workspace| workspace.name == name)
    }

    /// Checks the rules that the schema cannot express; `origin` is only used in errors.
    fn check(&self, origin: &Path) -> Result<(), ConfigError> {
        let mut names = HashSet::new();
        for workspace in &self.workspaces {
            let invalid = |reason: String| ConfigError::InvalidWorkspace {
                path: origin.to_path_buf(),
                workspace: workspace.name.clone(),
                reason,
            };
            if workspace.name.trim().is_empty() {
                return Err(invalid("the name is blank".to_owned()));
            }
            if !names.insert(workspace.name.as_str()) {
                return Err(invalid("another workspace has the same name".to_owned()));
            }
            for (side, tabs) in [("left", &workspace.left), ("right", &workspace.right)] {
                if tabs.is_empty() {
                    return Err(invalid(format!("`{side}` has no tabs")));
                }
                if tabs.iter().filter(|tab| tab.current).count() > 1 {
                    return Err(invalid(format!("`{side}` has more than one current tab")));
                }
                // A place built in code, not parsed, may have no text form that reads back.
                if let Some(tab) = tabs
                    .iter()
                    .find(|tab| Place::parse(&tab.place.to_string()).as_ref() != Some(&tab.place))
                {
                    return Err(invalid(format!(
                        "{:?} cannot be written as a location",
                        tab.place
                    )));
                }
            }
        }
        Ok(())
    }

    /// The text of `workspaces.toml`: the header, then the workspaces.
    fn to_text(&self) -> Result<String, toml::ser::Error> {
        Ok(format!("{HEADER}{}", toml::to_string(self)?))
    }
}

/// Saves `workspace` in the file at `path`: replaces the one with the same name in place, else
/// appends it. Returns all workspaces as the file now holds them.
///
/// Like [`remove_workspace`] and [`rename_workspace`], it reads the file again first, so
/// changes made meanwhile are kept; a file that is not valid, or that the change would make
/// invalid, is left alone. The file is data, not settings: it is written whole each time, so
/// comments in it are not kept. It is replaced atomically, through a symbolic link if it is
/// one.
///
/// Blocking: call it from a blocking thread.
pub fn save_workspace(path: &Path, workspace: &Workspace) -> Result<Workspaces, ConfigError> {
    let mut workspaces = Workspaces::load(path)?;
    match workspaces
        .workspaces
        .iter_mut()
        .find(|saved| saved.name == workspace.name)
    {
        Some(saved) => saved.clone_from(workspace),
        None => workspaces.workspaces.push(workspace.clone()),
    }
    write(path, &workspaces)?;
    Ok(workspaces)
}

/// Removes the workspace `name` from the file at `path`; a missing one is not an error.
/// Returns all workspaces as the file now holds them. See [`save_workspace`].
///
/// Blocking: call it from a blocking thread.
pub fn remove_workspace(path: &Path, name: &str) -> Result<Workspaces, ConfigError> {
    let mut workspaces = Workspaces::load(path)?;
    let count = workspaces.workspaces.len();
    workspaces
        .workspaces
        .retain(|workspace| workspace.name != name);
    if workspaces.workspaces.len() != count {
        write(path, &workspaces)?;
    }
    Ok(workspaces)
}

/// Renames the workspace `from` to `to` in the file at `path`, in its place; another workspace
/// named `to` is replaced (removed). A missing `from` is not an error. Returns all workspaces
/// as the file now holds them. See [`save_workspace`].
///
/// Blocking: call it from a blocking thread.
pub fn rename_workspace(path: &Path, from: &str, to: &str) -> Result<Workspaces, ConfigError> {
    let mut workspaces = Workspaces::load(path)?;
    if from == to || workspaces.get(from).is_none() {
        return Ok(workspaces);
    }
    workspaces
        .workspaces
        .retain(|workspace| workspace.name != to);
    for workspace in &mut workspaces.workspaces {
        if workspace.name == from {
            to.clone_into(&mut workspace.name);
        }
    }
    write(path, &workspaces)?;
    Ok(workspaces)
}

/// Checks `workspaces` and writes them to the file at `path`.
fn write(path: &Path, workspaces: &Workspaces) -> Result<(), ConfigError> {
    workspaces.check(path)?;
    let text = workspaces.to_text().map_err(|error| ConfigError::Write {
        path: path.to_path_buf(),
        source: io::Error::other(error),
    })?;
    write_atomic(path, text.as_bytes())
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::ffi::OsStrExt as _;
    use std::path::{Path, PathBuf};

    use super::*;

    const ORIGIN: &str = "/data/noc/workspaces.toml";

    const EXAMPLE: &str = "\
[[workspace]]
name = \"noon\"
active = \"right\"

[[workspace.left]]
location = \"~/src/noon\"
cursor = \"Cargo.toml\"
current = true

[[workspace.left]]
location = \"web:/var/www\"
sort = \"time\"
descending = true

[[workspace.right]]
location = \"root\"
";

    fn parse(text: &str) -> Workspaces {
        Workspaces::from_toml(text, Path::new(ORIGIN)).unwrap()
    }

    fn place(text: &str) -> Place {
        Place::parse(text).unwrap()
    }

    fn tab(location: &str) -> SavedTab {
        SavedTab {
            place: place(location),
            sort: SortBy::Name,
            descending: false,
            cursor: None,
            current: false,
        }
    }

    fn named(name: &str) -> Workspace {
        Workspace {
            name: name.to_owned(),
            active: PanelSide::Left,
            left: vec![tab("~")],
            right: vec![tab("sftp")],
        }
    }

    fn example() -> Workspace {
        Workspace {
            name: "noon".to_owned(),
            active: PanelSide::Right,
            left: vec![
                SavedTab {
                    cursor: Some("Cargo.toml".to_owned()),
                    current: true,
                    ..tab("~/src/noon")
                },
                SavedTab {
                    sort: SortBy::Time,
                    descending: true,
                    ..tab("web:/var/www")
                },
            ],
            right: vec![tab("root")],
        }
    }

    fn names(workspaces: &Workspaces) -> Vec<&str> {
        workspaces
            .workspaces
            .iter()
            .map(|workspace| workspace.name.as_str())
            .collect()
    }

    fn assert_invalid(text: &str, name: &str, reason: &str) {
        match Workspaces::from_toml(text, Path::new(ORIGIN)) {
            Err(ConfigError::InvalidWorkspace {
                path,
                workspace,
                reason: actual,
            }) => {
                assert_eq!(path, Path::new(ORIGIN));
                assert_eq!(workspace, name, "{text:?}");
                assert!(actual.contains(reason), "{actual:?} for {text:?}");
            }
            other => panic!("expected an invalid workspace for {text:?}, got {other:?}"),
        }
    }

    fn assert_parse_error(text: &str) {
        match Workspaces::from_toml(text, Path::new(ORIGIN)) {
            Err(ConfigError::Parse { path, .. }) => assert_eq!(path, Path::new(ORIGIN)),
            other => panic!("expected a parse error for {text:?}, got {other:?}"),
        }
    }

    #[test]
    fn the_example_round_trips() {
        let workspaces = parse(EXAMPLE);
        assert_eq!(
            workspaces,
            Workspaces {
                workspaces: vec![example()]
            }
        );
        assert_eq!(workspaces.get("noon"), Some(&example()));
        assert_eq!(workspaces.get("other"), None);
        let text = workspaces.to_text().unwrap();
        assert_eq!(text, format!("{HEADER}{EXAMPLE}"));
        assert_eq!(parse(&text), workspaces);
    }

    #[test]
    fn defaults_are_left_out() {
        let workspaces = parse(
            "[[workspace]]\nname = \"a\"\n\
             [[workspace.left]]\nlocation = \"/\"\n\
             [[workspace.right]]\nlocation = \"sftp\"\n",
        );
        let workspace = Workspace {
            left: vec![tab("/")],
            ..named("a")
        };
        assert_eq!(workspaces.get("a"), Some(&workspace));
        assert_eq!(
            workspaces.to_text().unwrap(),
            format!(
                "{HEADER}[[workspace]]\nname = \"a\"\nactive = \"left\"\n\n\
                 [[workspace.left]]\nlocation = \"/\"\n\n\
                 [[workspace.right]]\nlocation = \"sftp\"\n"
            )
        );
        assert_eq!(parse(""), Workspaces::default());
        assert_eq!(Workspaces::default().to_text().unwrap(), HEADER);
        assert_eq!(parse(HEADER), Workspaces::default());
    }

    #[test]
    fn places_have_a_text_form() {
        let remote = |host: &str, path: &str| Place::Remote {
            host: host.to_owned(),
            path: path.to_owned(),
        };
        let local = |path: &str| Place::Local(path.to_owned());
        for (text, expected) in [
            ("root", Place::Root),
            ("sftp", Place::Sftp),
            ("/", local("/")),
            ("/var/www", local("/var/www")),
            ("/a b:c", local("/a b:c")),
            ("~", local("~")),
            ("~/", local("~/")),
            ("~/src/noon", local("~/src/noon")),
            ("web:/var/www", remote("web", "/var/www")),
            ("web:", remote("web", "")),
            ("web:a:b", remote("web", "a:b")),
            (
                "u@web.example.com:~/site",
                remote("u@web.example.com", "~/site"),
            ),
            ("root:/srv", remote("root", "/srv")),
            ("sftp:", remote("sftp", "")),
        ] {
            assert_eq!(Place::parse(text), Some(expected.clone()), "{text:?}");
            assert_eq!(expected.to_string(), text);
            assert_eq!(String::from(expected), text);
        }
        for text in [
            "", "relative", "Root", "SFTP", ":x", ":", "~user", "~user/x", "~:x", "a/b:c", "./a:b",
        ] {
            assert_eq!(Place::parse(text), None, "{text:?}");
            assert_eq!(
                Place::try_from(text.to_owned()),
                Err(format!(
                    "`{text}` is not `root`, `sftp`, a path that starts with / or ~/, or host:path"
                ))
            );
        }
        assert_eq!(Place::try_from("web:x".to_owned()), Ok(remote("web", "x")));
    }

    #[test]
    fn local_places_use_a_tilde_under_home() {
        let home = Path::new("/home/u");
        let local = |path: &str| Place::local(Path::new(path), home);
        assert_eq!(local("/home/u"), Some(place("~")));
        assert_eq!(local("/home/u/"), Some(place("~")));
        assert_eq!(local("/home/u/src/noon"), Some(place("~/src/noon")));
        assert_eq!(local("/home/user"), Some(place("/home/user")));
        assert_eq!(local("/etc"), Some(place("/etc")));
        assert_eq!(local("/"), Some(place("/")));
        assert_eq!(local("src"), None);
        assert_eq!(local(""), None);
        let not_utf8 = Path::new(OsStr::from_bytes(b"/caf\xe9"));
        assert_eq!(Place::local(not_utf8, home), None);
        assert_eq!(
            Place::local(Path::new("/home/u/x"), Path::new("relative")),
            Some(place("/home/u/x"))
        );
        for path in ["/home/u", "/home/u/src/noon", "/etc"] {
            let dir = local(path).and_then(|place| place.local_dir(home));
            assert_eq!(dir, Some(PathBuf::from(path)));
        }
        assert_eq!(
            place("~/a").local_dir(Path::new("/Users/v")),
            Some(PathBuf::from("/Users/v/a"))
        );
        for text in ["root", "sftp", "web:/var/www"] {
            assert_eq!(place(text).local_dir(home), None);
        }
    }

    #[test]
    fn rejects_invalid_workspaces() {
        let sides = "[[workspace.left]]\nlocation = \"~\"\n[[workspace.right]]\nlocation = \"~\"\n";
        assert_invalid(&format!("[[workspace]]\nname = \"\"\n{sides}"), "", "blank");
        assert_invalid(
            &format!("[[workspace]]\nname = \" \\t\"\n{sides}"),
            " \t",
            "blank",
        );
        assert_invalid(
            &format!("[[workspace]]\nname = \"a\"\n{sides}[[workspace]]\nname = \"a\"\n{sides}"),
            "a",
            "same name",
        );
        assert_invalid(
            "[[workspace]]\nname = \"a\"\nleft = []\n[[workspace.right]]\nlocation = \"~\"\n",
            "a",
            "`left` has no tabs",
        );
        assert_invalid(
            "[[workspace]]\nname = \"a\"\nright = []\n[[workspace.left]]\nlocation = \"~\"\n",
            "a",
            "`right` has no tabs",
        );
        assert_invalid(
            "[[workspace]]\nname = \"a\"\n[[workspace.left]]\nlocation = \"~\"\n\
             [[workspace.right]]\nlocation = \"~\"\ncurrent = true\n\
             [[workspace.right]]\nlocation = \"/\"\ncurrent = true\n",
            "a",
            "`right` has more than one current tab",
        );
        // One current tab per side is fine, and so is none.
        parse(&format!(
            "[[workspace]]\nname = \"a\"\n[[workspace.left]]\nlocation = \"~\"\ncurrent = true\n\
             [[workspace.right]]\nlocation = \"~\"\ncurrent = true\n\
             [[workspace]]\nname = \"b\"\n{sides}"
        ));
    }

    #[test]
    fn rejects_what_the_schema_does_not_know() {
        let sides = "[[workspace.left]]\nlocation = \"~\"\n[[workspace.right]]\nlocation = \"~\"\n";
        for text in [
            "workspaces = []".to_owned(),
            "[workspace]\nname = \"a\"".to_owned(),
            format!("[[workspace]]\nname = \"a\"\ncolor = 1\n{sides}"),
            format!("[[workspace]]\nname = \"a\"\nactive = \"top\"\n{sides}"),
            format!("[[workspace]]\nname = 1\n{sides}"),
            "[[workspace]]\nname = \"a\"\n[[workspace.right]]\nlocation = \"~\"\n".to_owned(),
            format!("[[workspace]]\n{sides}"),
            "[[workspace]]\nname = \"a\"\n[[workspace.left]]\nlocation = \"src\"\n\
             [[workspace.right]]\nlocation = \"~\"\n"
                .to_owned(),
            "[[workspace]]\nname = \"a\"\n[[workspace.left]]\nlocation = \"~\"\npath = \"/\"\n\
             [[workspace.right]]\nlocation = \"~\"\n"
                .to_owned(),
            "[[workspace]]\nname = \"a\"\n[[workspace.left]]\ncursor = \"x\"\n\
             [[workspace.right]]\nlocation = \"~\"\n"
                .to_owned(),
            "[[workspace]]\nname = \"a\"\n[[workspace.left]]\nlocation = \"~\"\nsort = \"date\"\n\
             [[workspace.right]]\nlocation = \"~\"\n"
                .to_owned(),
            "[[workspace]]\nname = \"a\"\n[[workspace.left]]\nlocation = \"~\"\n\
             descending = \"yes\"\n[[workspace.right]]\nlocation = \"~\"\n"
                .to_owned(),
            "[[workspace]\n".to_owned(),
        ] {
            assert_parse_error(&text);
        }
    }

    #[test]
    fn places_built_in_code_must_read_back() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("workspaces.toml");
        for bad in [
            Place::Local("src".to_owned()),
            Place::Local("~user".to_owned()),
            Place::Remote {
                host: "a:b".to_owned(),
                path: "c".to_owned(),
            },
            Place::Remote {
                host: String::new(),
                path: "/srv".to_owned(),
            },
            Place::Remote {
                host: "~web".to_owned(),
                path: "/srv".to_owned(),
            },
        ] {
            let workspace = Workspace {
                right: vec![SavedTab {
                    place: bad.clone(),
                    ..tab("~")
                }],
                ..named("a")
            };
            match save_workspace(&path, &workspace) {
                Err(ConfigError::InvalidWorkspace { reason, .. }) => {
                    assert!(reason.contains("cannot be written"), "{reason}");
                }
                other => panic!("expected an invalid workspace for {bad:?}, got {other:?}"),
            }
            assert!(!path.exists());
        }
    }

    #[test]
    fn a_missing_file_holds_no_workspaces() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            Workspaces::load(&tmp.path().join("workspaces.toml")).unwrap(),
            Workspaces::default()
        );
    }

    #[test]
    fn an_unreadable_file_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("workspaces.toml");
        fs::create_dir(&path).unwrap();
        assert!(matches!(
            Workspaces::load(&path),
            Err(ConfigError::Read { .. })
        ));
        assert!(matches!(
            save_workspace(&path, &named("a")),
            Err(ConfigError::Read { .. })
        ));
    }

    #[test]
    fn saving_appends_then_replaces_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("workspaces.toml");
        for name in ["a", "b", "c"] {
            let saved = save_workspace(&path, &named(name)).unwrap();
            assert_eq!(saved, Workspaces::load(&path).unwrap());
        }
        let b = Workspace {
            active: PanelSide::Right,
            right: vec![tab("web:"), tab("/srv")],
            ..named("b")
        };
        let saved = save_workspace(&path, &b).unwrap();
        assert_eq!(names(&saved), ["a", "b", "c"]);
        assert_eq!(saved.get("b"), Some(&b));
        assert_eq!(saved, Workspaces::load(&path).unwrap());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(HEADER), "{text}");
    }

    #[test]
    fn saving_keeps_changes_made_meanwhile() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("workspaces.toml");
        fs::write(&path, EXAMPLE).unwrap();
        let saved = save_workspace(&path, &named("a")).unwrap();
        assert_eq!(saved.workspaces, [example(), named("a")]);
    }

    #[test]
    fn removing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("workspaces.toml");
        assert_eq!(remove_workspace(&path, "a").unwrap(), Workspaces::default());
        assert!(!path.exists(), "nothing to remove writes nothing");
        for name in ["a", "b", "c"] {
            save_workspace(&path, &named(name)).unwrap();
        }
        let removed = remove_workspace(&path, "b").unwrap();
        assert_eq!(names(&removed), ["a", "c"]);
        assert_eq!(removed, Workspaces::load(&path).unwrap());
        assert_eq!(remove_workspace(&path, "b").unwrap(), removed);
        remove_workspace(&path, "a").unwrap();
        assert_eq!(remove_workspace(&path, "c").unwrap(), Workspaces::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), HEADER);
    }

    #[test]
    fn renaming_keeps_the_place() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("workspaces.toml");
        for name in ["a", "b", "c"] {
            save_workspace(&path, &named(name)).unwrap();
        }
        let c = Workspace {
            active: PanelSide::Right,
            ..named("c")
        };
        save_workspace(&path, &c).unwrap();
        let renamed = rename_workspace(&path, "b", "d").unwrap();
        assert_eq!(names(&renamed), ["a", "d", "c"]);
        assert_eq!(renamed, Workspaces::load(&path).unwrap());

        // The workspace that had the new name goes.
        let renamed = rename_workspace(&path, "c", "a").unwrap();
        assert_eq!(names(&renamed), ["d", "a"]);
        assert_eq!(
            renamed.get("a"),
            Some(&Workspace {
                name: "a".to_owned(),
                ..c
            })
        );
        assert_eq!(renamed, Workspaces::load(&path).unwrap());

        assert_eq!(rename_workspace(&path, "x", "y").unwrap(), renamed);
        assert_eq!(rename_workspace(&path, "a", "a").unwrap(), renamed);
        assert!(matches!(
            rename_workspace(&path, "a", " "),
            Err(ConfigError::InvalidWorkspace { .. })
        ));
        assert_eq!(Workspaces::load(&path).unwrap(), renamed);
    }

    #[test]
    fn writes_leave_an_invalid_file_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("workspaces.toml");
        for text in [
            "[[workspace]\nname = \"a\"\n",
            "# Mine\n[[workspace]]\nname = \"a\"\ncolor = 1\n",
            "[[workspace]]\nname = \"a\"\nleft = []\nright = []\n",
        ] {
            fs::write(&path, text).unwrap();
            let results = [
                save_workspace(&path, &named("b")),
                remove_workspace(&path, "a"),
                rename_workspace(&path, "a", "b"),
            ];
            for result in results {
                assert!(
                    matches!(
                        result,
                        Err(ConfigError::Parse { .. } | ConfigError::InvalidWorkspace { .. })
                    ),
                    "{result:?} for {text:?}"
                );
            }
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
    }

    #[test]
    fn saving_an_invalid_workspace_leaves_the_file_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("workspaces.toml");
        fs::write(&path, EXAMPLE).unwrap();
        for workspace in [
            named(""),
            Workspace {
                left: Vec::new(),
                ..named("a")
            },
            Workspace {
                right: vec![
                    SavedTab {
                        current: true,
                        ..tab("~")
                    },
                    SavedTab {
                        current: true,
                        ..tab("/")
                    },
                ],
                ..named("a")
            },
        ] {
            assert!(matches!(
                save_workspace(&path, &workspace),
                Err(ConfigError::InvalidWorkspace { .. })
            ));
            assert_eq!(fs::read_to_string(&path).unwrap(), EXAMPLE);
        }
    }

    #[test]
    fn writes_create_the_file_and_its_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("data/noc/workspaces.toml");
        save_workspace(&path, &example()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{HEADER}{EXAMPLE}")
        );
        assert_eq!(
            Workspaces::load(&path).unwrap().get("noon"),
            Some(&example())
        );
    }
}
