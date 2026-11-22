//! The machine Noon Commander runs on and who runs it, for the line of the mode, the prompt of
//! the command line, the terminal title, and what the panels set apart as its own.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt as _;

use super::cells;

/// The user name of uid 0, which [`Machine::user`] takes whatever the environment says.
pub(crate) const ROOT: &str = "root";

/// Where local panels are, and with whose rights.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Machine {
    /// The host name up to its first dot, terminal-safe; `None` if the system gives none.
    pub(crate) host: Option<String>,
    /// The user name, terminal-safe: `root` for uid 0, else `$USER` or `$LOGNAME`.
    pub(crate) user: Option<String>,
    /// The process runs with the effective uid 0.
    pub(crate) root: bool,
    /// Who the process runs as, by number.
    pub(crate) identity: Option<Identity>,
}

/// Who a process runs as, by number: its effective uid, and the groups whose rights it has; and
/// their names, those that are known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) uid: u32,
    pub(crate) groups: Vec<u32>,
    pub(crate) names: Names,
}

/// The names of users and of groups by their numbers, those that are known, terminal-safe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Names {
    pub(crate) users: HashMap<u32, String>,
    pub(crate) groups: HashMap<u32, String>,
}

impl Names {
    /// The names this machine gives `uids` and `gids`, those it knows. Blocking: the system may
    /// ask a directory service.
    pub(crate) fn local(uids: &BTreeSet<u32>, gids: &BTreeSet<u32>) -> Self {
        use nix::unistd::{Gid, Group, Uid, User};
        let users = (uids.iter())
            .filter_map(|&uid| {
                let user = User::from_uid(Uid::from_raw(uid)).ok()??;
                Some((uid, cells::sanitize(user.name.as_bytes())))
            })
            .collect();
        let groups = (gids.iter())
            .filter_map(|&gid| {
                let group = Group::from_gid(Gid::from_raw(gid)).ok()??;
                Some((gid, cells::sanitize(group.name.as_bytes())))
            })
            .collect();
        Self { users, groups }
    }
}

impl Identity {
    /// This process: its effective uid, its effective gid, and its supplementary groups.
    fn current() -> Self {
        let mut groups = vec![rustix::process::getegid().as_raw()];
        let more = rustix::process::getgroups().unwrap_or_default();
        groups.extend(more.into_iter().map(rustix::process::Gid::as_raw));
        Self {
            uid: rustix::process::geteuid().as_raw(),
            groups,
            names: Names::default(),
        }
    }

    /// Who `id -u` and `id -G` say, from what they print; `None` where that is not one uid and
    /// a list of gids.
    pub(crate) fn parse(uid: &[u8], groups: &[u8]) -> Option<Self> {
        let numbers = |text: &[u8]| -> Option<Vec<u32>> {
            let text = std::str::from_utf8(text).ok()?;
            text.split_whitespace().map(|id| id.parse().ok()).collect()
        };
        let [uid] = numbers(uid)?[..] else {
            return None;
        };
        let groups = numbers(groups).filter(|groups| !groups.is_empty())?;
        Some(Self {
            uid,
            groups,
            names: Names::default(),
        })
    }

    /// `self` with the names `id -un` and `id -Gn` print, of its uid and of its groups; those of
    /// the groups only where they are as many as its groups, as `id -G` lists them in that order.
    pub(crate) fn named(mut self, user: Option<&[u8]>, groups: Option<&[u8]>) -> Self {
        if let Some(user) = user.map(<[u8]>::trim_ascii).filter(|user| !user.is_empty()) {
            self.names.users.insert(self.uid, cells::sanitize(user));
        }
        let words: Vec<&[u8]> = (groups.unwrap_or_default())
            .split(u8::is_ascii_whitespace)
            .filter(|word| !word.is_empty())
            .collect();
        if words.len() == self.groups.len() {
            let pairs = self.groups.iter().zip(words);
            (self.names.groups).extend(pairs.map(|(&gid, word)| (gid, cells::sanitize(word))));
        }
        self
    }
}

impl Machine {
    /// This machine and this process.
    pub(crate) fn current() -> Self {
        let uname = rustix::system::uname();
        let machine = Self::new(
            uname.nodename().to_bytes(),
            rustix::process::geteuid().is_root(),
            |name| std::env::var_os(name),
        );
        Self {
            identity: Some(Identity::current()),
            ..machine
        }
    }

    /// The machine named `nodename`, run by uid 0 if `root`; `env` looks up environment
    /// variables.
    fn new(nodename: &[u8], root: bool, env: impl Fn(&str) -> Option<OsString>) -> Self {
        let short = nodename
            .split(|&byte| byte == b'.')
            .next()
            .unwrap_or_default();
        let host = Some(cells::sanitize(short)).filter(|host| !host.is_empty());
        // sudo and su may leave the caller's name behind; the uid is what counts.
        let user = if root {
            Some(ROOT.to_owned())
        } else {
            ["USER", "LOGNAME"]
                .into_iter()
                .filter_map(&env)
                .map(|value| cells::sanitize(value.as_bytes()))
                .find(|user| !user.is_empty())
        };
        Self {
            host,
            user,
            root,
            identity: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: Vec<(String, OsString)> = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
            .collect();
        move |name| {
            vars.iter()
                .find(|(var, _)| var == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn the_host_ends_at_its_first_dot_and_the_user_comes_from_the_environment() {
        let machine = Machine::new(b"mac.local", false, env(&[("USER", "alice")]));
        assert_eq!(machine.host.as_deref(), Some("mac"));
        assert_eq!(machine.user.as_deref(), Some("alice"));
        assert!(!machine.root);
    }

    #[test]
    fn logname_stands_in_for_user() {
        let vars = [("USER", ""), ("LOGNAME", "bob")];
        let machine = Machine::new(b"box", false, env(&vars));
        assert_eq!(machine.user.as_deref(), Some("bob"));
        assert_eq!(Machine::new(b"box", false, env(&[])).user, None);
    }

    #[test]
    fn uid_0_is_root_whatever_the_environment_says() {
        let machine = Machine::new(b"mac", true, env(&[("USER", "alice")]));
        assert_eq!(machine.user.as_deref(), Some("root"));
        assert!(machine.root);
    }

    #[test]
    fn who_id_says_is_one_uid_and_its_groups() {
        let parsed = Identity::parse(b"501\n", b"20 12 61 79\n");
        assert_eq!(
            parsed,
            Some(Identity {
                uid: 501,
                groups: vec![20, 12, 61, 79],
                names: Names::default(),
            })
        );
        assert_eq!(Identity::parse(b"501 502\n", b"20\n"), None, "one uid");
        assert_eq!(Identity::parse(b"alice\n", b"20\n"), None, "numbers");
        assert_eq!(Identity::parse(b"501\n", b""), None, "a group at least");
        let current = Identity::current();
        assert_eq!(current.uid, rustix::process::geteuid().as_raw());
        let egid = rustix::process::getegid().as_raw();
        assert_eq!(
            current.groups.first(),
            Some(&egid),
            "the effective gid first"
        );
    }

    #[test]
    fn id_names_the_uid_and_the_groups_in_their_order() {
        let alice = || Identity::parse(b"501\n", b"20 12\n").unwrap();
        let named = alice().named(Some(b"alice\n"), Some(b"staff everyone\n"));
        assert_eq!(
            named.names.users.get(&501).map(String::as_str),
            Some("alice")
        );
        assert_eq!(
            named.names.groups.get(&20).map(String::as_str),
            Some("staff")
        );
        assert_eq!(
            named.names.groups.get(&12).map(String::as_str),
            Some("everyone")
        );
        let spaced = alice().named(None, Some(b"domain users everyone\n"));
        assert!(spaced.names.users.is_empty(), "no name without -un");
        assert!(
            spaced.names.groups.is_empty(),
            "nor where they do not match"
        );
    }

    #[test]
    fn this_machine_names_uid_0() {
        let names = Names::local(&BTreeSet::from([0]), &BTreeSet::from([0]));
        assert_eq!(names.users.get(&0).map(String::as_str), Some(ROOT));
        assert!(names.groups.contains_key(&0), "wheel or root: {names:?}");
    }

    #[test]
    fn an_empty_or_unsafe_host_name_is_cleaned() {
        assert_eq!(Machine::new(b"", false, env(&[])).host, None);
        assert_eq!(Machine::new(b".local", false, env(&[])).host, None);
        let host = Machine::new(b"a\x1bb", false, env(&[])).host.unwrap();
        assert!(!host.contains('\x1b'), "{host:?}");
    }
}
