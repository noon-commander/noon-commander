//! Options that sftp-tui forces on `ssh`, and whether this build may forward anything.
//!
//! This module is the only place that checks the `forwarding` feature.
//! See `docs/adr/0004-forwarding-compile-time-feature.md`.

/// Whether this build was compiled with the `forwarding` feature.
pub const FORWARDING_ENABLED: bool = cfg!(feature = "forwarding");

/// Options forced on every SFTP channel, in every build.
///
/// The first five mirror what `sftp(1)` passes to `ssh`. `RemoteCommand` and
/// `RequestTTY` from the user's config would otherwise break the subsystem request.
pub const SFTP_CHANNEL_OPTIONS: &[(&str, &str)] = &[
    ("ForwardX11", "no"),
    ("PermitLocalCommand", "no"),
    ("ClearAllForwardings", "yes"),
    ("ControlMaster", "no"),
    ("ForwardAgent", "no"),
    ("RemoteCommand", "none"),
    ("RequestTTY", "no"),
];

/// Options forced on the master connection and the console when forwarding is compiled out.
pub const NO_FORWARDING_OPTIONS: &[(&str, &str)] = &[
    ("ClearAllForwardings", "yes"),
    ("ForwardAgent", "no"),
    ("ForwardX11", "no"),
    ("Tunnel", "no"),
    ("GSSAPIDelegateCredentials", "no"),
];

/// Forwarding-related options this build forces on the master connection and the console.
pub const fn session_options() -> &'static [(&'static str, &'static str)] {
    if FORWARDING_ENABLED {
        &[]
    } else {
        NO_FORWARDING_OPTIONS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(options: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
        options.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    #[test]
    fn sftp_channels_never_forward() {
        assert_eq!(
            value(SFTP_CHANNEL_OPTIONS, "ClearAllForwardings"),
            Some("yes")
        );
        assert_eq!(value(SFTP_CHANNEL_OPTIONS, "ForwardAgent"), Some("no"));
        assert_eq!(value(SFTP_CHANNEL_OPTIONS, "ForwardX11"), Some("no"));
    }

    #[test]
    fn session_options_follow_the_feature() {
        if FORWARDING_ENABLED {
            assert!(session_options().is_empty());
        } else {
            assert_eq!(session_options(), NO_FORWARDING_OPTIONS);
        }
    }
}
