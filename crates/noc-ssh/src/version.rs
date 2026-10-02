//! The OpenSSH client version (`ssh -V`).

use std::fmt;
use std::time::Duration;

use crate::command::{SshSettings, version_command};
use crate::error::SshError;

/// An OpenSSH release number, such as 9.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpenSshVersion {
    pub major: u32,
    pub minor: u32,
}

impl OpenSshVersion {
    /// The oldest supported client. 8.4 added `SSH_ASKPASS_REQUIRE` (ADR 0003); 8.7 added the
    /// `StdinNull` and `ForkAfterAuthentication` keywords, which Noon Commander forces off.
    pub const MINIMUM: Self = Self { major: 8, minor: 7 };

    /// Parses the banner printed by `ssh -V`, e.g. `OpenSSH_9.6p1 Ubuntu-3ubuntu13, OpenSSL 3.0.13`.
    pub fn parse_banner(banner: &str) -> Option<Self> {
        let (_, rest) = banner.split_once("OpenSSH_")?;
        let (major, rest) = leading_number(rest)?;
        let (minor, _) = leading_number(rest.strip_prefix('.')?)?;
        Some(Self { major, minor })
    }
}

impl fmt::Display for OpenSshVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

fn leading_number(text: &str) -> Option<(u32, &str)> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let number = text[..end].parse().ok()?;
    Some((number, &text[end..]))
}

/// Runs `ssh -V` and checks that the client is a supported OpenSSH release.
pub async fn check_version(settings: &SshSettings) -> Result<OpenSshVersion, SshError> {
    let output = tokio::time::timeout(Duration::from_secs(10), version_command(settings).output())
        .await
        .map_err(|_| SshError::Timeout)?
        .map_err(|source| SshError::Spawn {
            program: settings.program.clone(),
            source,
        })?;
    // ssh prints its version to stderr.
    let banner = String::from_utf8_lossy(&output.stderr);
    let banner = banner.trim();
    let version = OpenSshVersion::parse_banner(banner).ok_or_else(|| SshError::NotOpenSsh {
        program: settings.program.clone(),
        banner: banner.to_owned(),
    })?;
    if version < OpenSshVersion::MINIMUM {
        return Err(SshError::TooOld {
            found: version,
            required: OpenSshVersion::MINIMUM,
        });
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major: u32, minor: u32) -> OpenSshVersion {
        OpenSshVersion { major, minor }
    }

    #[test]
    fn parses_banners() {
        assert_eq!(
            OpenSshVersion::parse_banner("OpenSSH_10.3p1, LibreSSL 3.3.6"),
            Some(version(10, 3))
        );
        assert_eq!(
            OpenSshVersion::parse_banner(
                "OpenSSH_9.6p1 Ubuntu-3ubuntu13.5, OpenSSL 3.0.13 30 Jan 2024"
            ),
            Some(version(9, 6))
        );
        assert_eq!(
            OpenSshVersion::parse_banner("OpenSSH_8.7"),
            Some(version(8, 7))
        );
        assert_eq!(OpenSshVersion::parse_banner("Dropbear v2022.83"), None);
        assert_eq!(OpenSshVersion::parse_banner("OpenSSH_x.y"), None);
        assert_eq!(OpenSshVersion::parse_banner(""), None);
    }

    #[test]
    fn orders_versions() {
        assert!(version(8, 6) < OpenSshVersion::MINIMUM);
        assert!(version(8, 10) > OpenSshVersion::MINIMUM);
        assert!(version(10, 0) > version(9, 9));
    }
}
