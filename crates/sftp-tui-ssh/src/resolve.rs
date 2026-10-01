//! Effective host settings from `ssh -G`.
//!
//! `ssh -G` evaluates the whole config, including `Match exec` predicates, which run
//! commands. Call it lazily for one host at a time, never for every host at startup.

use std::time::Duration;

use crate::command::{Role, SshSettings, Target, session_command};
use crate::error::SshError;
use crate::session::stderr_tail;

/// Settings ssh would use to connect to a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHost {
    pub user: String,
    pub hostname: String,
    pub port: u16,
    /// `None` when ssh connects directly.
    pub proxy_jump: Option<String>,
    /// Every `key value` line of the output, in order. Keys are lowercase.
    pub options: Vec<(String, String)>,
}

impl ResolvedHost {
    /// All values of `key` (lowercase), in order.
    pub fn values<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.options
            .iter()
            .filter(move |(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The first value of `key` (lowercase).
    pub fn value(&self, key: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// `user@hostname`, with `:port` unless it is 22.
    pub fn address(&self) -> String {
        if self.port == 22 {
            format!("{}@{}", self.user, self.hostname)
        } else {
            format!("{}@{}:{}", self.user, self.hostname, self.port)
        }
    }

    /// Parses the output of `ssh -G`.
    pub fn parse(output: &str) -> Result<Self, ParseError> {
        let options: Vec<(String, String)> = output
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return None;
                }
                let (key, value) = line.split_once(' ').unwrap_or((line, ""));
                Some((key.to_ascii_lowercase(), value.trim().to_owned()))
            })
            .collect();
        let find = |key: &str| {
            options
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        let user = find("user").ok_or(ParseError::Missing("user"))?;
        let hostname = find("hostname").ok_or(ParseError::Missing("hostname"))?;
        let port = find("port").ok_or(ParseError::Missing("port"))?;
        let port = port.parse().map_err(|_| ParseError::InvalidPort(port))?;
        let proxy_jump = find("proxyjump").filter(|jump| !jump.eq_ignore_ascii_case("none"));
        Ok(Self {
            user,
            hostname,
            port,
            proxy_jump,
            options,
        })
    }
}

/// Why the output of `ssh -G` could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("`{0}` is missing")]
    Missing(&'static str),
    #[error("invalid port `{0}`")]
    InvalidPort(String),
}

/// Runs `ssh -G` for `target`.
pub async fn resolve(settings: &SshSettings, target: &Target) -> Result<ResolvedHost, SshError> {
    target.validate(settings)?;
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        session_command(settings, target, Role::Resolve, None).output(),
    )
    .await
    .map_err(|_| SshError::Timeout)?
    .map_err(|source| SshError::Spawn {
        program: settings.program.clone(),
        source,
    })?;
    if !output.status.success() {
        return Err(SshError::Exited {
            status: output.status,
            stderr: stderr_tail(&String::from_utf8_lossy(&output.stderr)),
        });
    }
    Ok(ResolvedHost::parse(&String::from_utf8_lossy(
        &output.stdout,
    ))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: &str = "host web\nuser deploy\nhostname 10.0.0.5\nport 2222\n\
        identityfile ~/.ssh/id_ed25519\nidentityfile ~/.ssh/id_rsa\nproxyjump bastion\n";

    #[test]
    fn parses_ssh_g_output() {
        let host = ResolvedHost::parse(OUTPUT).unwrap();
        assert_eq!(host.user, "deploy");
        assert_eq!(host.hostname, "10.0.0.5");
        assert_eq!(host.port, 2222);
        assert_eq!(host.proxy_jump.as_deref(), Some("bastion"));
        assert_eq!(
            host.values("identityfile").collect::<Vec<_>>(),
            ["~/.ssh/id_ed25519", "~/.ssh/id_rsa"]
        );
        assert_eq!(host.value("host"), Some("web"));
        assert_eq!(host.address(), "deploy@10.0.0.5:2222");
    }

    #[test]
    fn proxy_jump_none_means_direct() {
        let host = ResolvedHost::parse("user u\nhostname h\nport 22\nproxyjump none\n").unwrap();
        assert_eq!(host.proxy_jump, None);
        assert_eq!(host.address(), "u@h");
    }

    #[test]
    fn rejects_incomplete_output() {
        assert_eq!(
            ResolvedHost::parse("user u\nport 22\n"),
            Err(ParseError::Missing("hostname"))
        );
        assert_eq!(
            ResolvedHost::parse("user u\nhostname h\nport x\n"),
            Err(ParseError::InvalidPort("x".to_owned()))
        );
    }
}
