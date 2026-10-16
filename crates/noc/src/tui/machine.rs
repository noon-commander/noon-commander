//! The machine Noon Commander runs on and who runs it, for the line of the mode, the prompt of
//! the command line, and the terminal title.

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
}

impl Machine {
    /// This machine and this process.
    pub(crate) fn current() -> Self {
        let uname = rustix::system::uname();
        Self::new(
            uname.nodename().to_bytes(),
            rustix::process::geteuid().is_root(),
            |name| std::env::var_os(name),
        )
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
        Self { host, user, root }
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
    fn an_empty_or_unsafe_host_name_is_cleaned() {
        assert_eq!(Machine::new(b"", false, env(&[])).host, None);
        assert_eq!(Machine::new(b".local", false, env(&[])).host, None);
        let host = Machine::new(b"a\x1bb", false, env(&[])).host.unwrap();
        assert!(!host.contains('\x1b'), "{host:?}");
    }
}
