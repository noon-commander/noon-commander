//! Validation of user-supplied ssh arguments (`ssh.args`).
//!
//! See `docs/adr/0004-forwarding-compile-time-feature.md` for the rules and
//! `docs/adr/0062-forwarding-is-never-allowed.md`, which leaves no build that forwards.

/// Why a list of user-supplied ssh arguments was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArgsError {
    #[error("unexpected argument `{0}`: only ssh options are allowed")]
    NotAnOption(String),
    #[error("unknown ssh flag `-{0}`")]
    UnknownFlag(char),
    #[error("ssh flag `-{0}` requires a value")]
    MissingValue(char),
    #[error("ssh flag `-{0}` is reserved by Noon Commander")]
    ReservedFlag(char),
    #[error("ssh flag `-F` is not allowed: set `ssh.config_file` instead")]
    ConfigFileFlag,
    #[error("ssh flag `-{0}` enables forwarding, which Noon Commander does not allow")]
    ForwardingFlag(char),
    #[error("malformed ssh option `{0}`")]
    MalformedOption(String),
    #[error("ssh option `{0}` is reserved by Noon Commander")]
    ReservedOption(String),
    #[error("ssh option `{0}` enables forwarding, which Noon Commander does not allow")]
    ForwardingOption(String),
}

const RESERVED_OPTIONS: &[&str] = &[
    "ControlMaster",
    "ControlPath",
    "ControlPersist",
    "SessionType",
    "StdinNull",
    "ForkAfterAuthentication",
    "RemoteCommand",
    "RequestTTY",
    "PermitLocalCommand",
    "LocalCommand",
];

/// Any other key containing `forward` or `tunnel` counts as forwarding too.
const FORWARDING_OPTIONS: &[&str] = &[
    "LocalForward",
    "RemoteForward",
    "DynamicForward",
    "ForwardAgent",
    "ForwardX11",
    "ForwardX11Trusted",
    "Tunnel",
    "TunnelDevice",
    "GatewayPorts",
    "ExitOnForwardFailure",
    "PermitRemoteOpen",
    "GSSAPIDelegateCredentials",
    "ClearAllForwardings",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Allowed,
    Forwarding,
    Reserved,
    ConfigFile,
    ConfigOption,
}

/// Checks user-supplied ssh arguments against the policy.
pub fn validate(args: &[String]) -> Result<(), ArgsError> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let Some(flags) = arg
            .strip_prefix('-')
            .filter(|flags| !flags.is_empty() && *flags != "-")
        else {
            return Err(ArgsError::NotAnOption(arg.clone()));
        };
        for (index, flag) in flags.char_indices() {
            let Some((class, takes_value)) = classify(flag) else {
                return Err(ArgsError::UnknownFlag(flag));
            };
            match class {
                Class::Reserved => return Err(ArgsError::ReservedFlag(flag)),
                Class::ConfigFile => return Err(ArgsError::ConfigFileFlag),
                Class::Forwarding => return Err(ArgsError::ForwardingFlag(flag)),
                Class::Allowed | Class::ConfigOption => {}
            }
            if takes_value {
                // Like getopt: the rest of this argument, or else the whole next one.
                let attached = &flags[index + flag.len_utf8()..];
                let value = if attached.is_empty() {
                    args.next()
                        .map(String::as_str)
                        .ok_or(ArgsError::MissingValue(flag))?
                } else {
                    attached
                };
                if class == Class::ConfigOption {
                    check_option(value)?;
                }
                break;
            }
        }
    }
    Ok(())
}

/// Classifies a flag and tells whether it takes a value, following the option string of
/// ssh(1): `46AaB:b:Cc:D:E:e:F:fGgI:i:J:KkL:l:Mm:NnO:o:P:p:Q:qR:S:sTtVvW:w:XxYy`.
fn classify(flag: char) -> Option<(Class, bool)> {
    Some(match flag {
        '4' | '6' | 'a' | 'C' | 'k' | 'q' | 'v' | 'x' => (Class::Allowed, false),
        'B' | 'b' | 'c' | 'e' | 'I' | 'i' | 'J' | 'l' | 'm' | 'P' | 'p' => (Class::Allowed, true),
        'A' | 'g' | 'K' | 'X' | 'Y' => (Class::Forwarding, false),
        'D' | 'L' | 'R' | 'w' => (Class::Forwarding, true),
        'f' | 'G' | 'M' | 'N' | 'n' | 's' | 'T' | 't' | 'V' | 'y' => (Class::Reserved, false),
        'E' | 'O' | 'Q' | 'S' | 'W' => (Class::Reserved, true),
        'F' => (Class::ConfigFile, true),
        'o' => (Class::ConfigOption, true),
        _ => return None,
    })
}

/// Checks the value of `-o`, a config line whose keyword is the option key.
fn check_option(option: &str) -> Result<(), ArgsError> {
    let line = option.trim_start();
    let end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let key = &line[..end];
    // ssh drops double quotes inside a keyword (`"ControlPath"` is `ControlPath`), so only plain
    // keywords can be checked. Every real keyword is alphanumeric.
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ArgsError::MalformedOption(option.to_owned()));
    }
    let listed = |keys: &[&str]| keys.iter().any(|name| name.eq_ignore_ascii_case(key));
    if listed(RESERVED_OPTIONS) {
        return Err(ArgsError::ReservedOption(key.to_owned()));
    }
    let lowercase = key.to_ascii_lowercase();
    let forwards =
        listed(FORWARDING_OPTIONS) || lowercase.contains("forward") || lowercase.contains("tunnel");
    if forwards {
        return Err(ArgsError::ForwardingOption(key.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const ALLOWED_FLAGS: &[char] = &['4', '6', 'a', 'C', 'k', 'q', 'v', 'x'];
    const ALLOWED_VALUE_FLAGS: &[char] = &['B', 'b', 'c', 'e', 'I', 'i', 'J', 'l', 'm', 'P', 'p'];
    const RESERVED_FLAGS: &[char] = &['f', 'G', 'M', 'N', 'n', 's', 'T', 't', 'V', 'y'];
    const RESERVED_VALUE_FLAGS: &[char] = &['E', 'O', 'Q', 'S', 'W'];
    const FORWARDING_FLAGS: &[char] = &['A', 'g', 'K', 'X', 'Y'];
    const FORWARDING_VALUE_FLAGS: &[char] = &['D', 'L', 'R', 'w'];

    fn check(args: &[&str]) -> Result<(), ArgsError> {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        validate(&args)
    }

    fn option(value: &str) -> Result<(), ArgsError> {
        check(&["-o", value])
    }

    #[test]
    fn classification_matches_the_ssh_option_string() {
        let mut chars = "46AaB:b:Cc:D:E:e:F:fGgI:i:J:KkL:l:Mm:NnO:o:P:p:Q:qR:S:sTtVvW:w:XxYy"
            .chars()
            .peekable();
        let mut takes_value = HashMap::new();
        while let Some(flag) = chars.next() {
            takes_value.insert(flag, chars.next_if_eq(&':').is_some());
        }
        for flag in (0..=127u8).map(char::from) {
            assert_eq!(
                classify(flag).map(|(_, value)| value),
                takes_value.get(&flag).copied(),
                "-{flag}"
            );
        }
    }

    #[test]
    fn allowed_flags_pass() {
        for flag in ALLOWED_FLAGS {
            assert_eq!(check(&[&format!("-{flag}")]), Ok(()));
        }
        for flag in ALLOWED_VALUE_FLAGS {
            assert_eq!(check(&[&format!("-{flag}"), "x"]), Ok(()));
            assert_eq!(check(&[&format!("-{flag}x")]), Ok(()));
        }
    }

    #[test]
    fn reserved_flags_fail() {
        for &flag in RESERVED_FLAGS {
            assert_eq!(
                check(&[&format!("-{flag}")]),
                Err(ArgsError::ReservedFlag(flag))
            );
        }
        for &flag in RESERVED_VALUE_FLAGS {
            assert_eq!(
                check(&[&format!("-{flag}"), "x"]),
                Err(ArgsError::ReservedFlag(flag))
            );
            assert_eq!(
                check(&[&format!("-{flag}x")]),
                Err(ArgsError::ReservedFlag(flag))
            );
        }
    }

    #[test]
    fn config_file_flag_fails() {
        for args in [&["-F", "x"][..], &["-Fx"], &["-vF", "x"], &["-F"]] {
            assert_eq!(check(args), Err(ArgsError::ConfigFileFlag));
        }
    }

    #[test]
    fn forwarding_flags_fail() {
        for &flag in FORWARDING_FLAGS {
            assert_eq!(
                check(&[&format!("-{flag}")]),
                Err(ArgsError::ForwardingFlag(flag))
            );
        }
        for &flag in FORWARDING_VALUE_FLAGS {
            assert_eq!(
                check(&[&format!("-{flag}"), "8080:localhost:80"]),
                Err(ArgsError::ForwardingFlag(flag))
            );
            assert_eq!(
                check(&[&format!("-{flag}8080:localhost:80")]),
                Err(ArgsError::ForwardingFlag(flag))
            );
        }
    }

    #[test]
    fn unknown_flags_fail() {
        for (arg, flag) in [("-Z", 'Z'), ("-h", 'h'), ("-1", '1'), ("-é", 'é')] {
            assert_eq!(check(&[arg]), Err(ArgsError::UnknownFlag(flag)));
        }
        assert_eq!(check(&["--verbose"]), Err(ArgsError::UnknownFlag('-')));
    }

    #[test]
    fn bundled_flags() {
        assert_eq!(check(&["-4Cv"]), Ok(()));
        assert_eq!(check(&["-4X"]), Err(ArgsError::ForwardingFlag('X')));
        assert_eq!(check(&["-vp22"]), Ok(()));
        assert_eq!(check(&["-vp", "22"]), Ok(()));
        assert_eq!(check(&["-vM"]), Err(ArgsError::ReservedFlag('M')));
        // The rest of the argument is a value, never more flags.
        assert_eq!(check(&["-pX"]), Ok(()));
        assert_eq!(check(&["-lMN"]), Ok(()));
        assert_eq!(check(&["-vlA", "-x"]), Ok(()));
    }

    #[test]
    fn separate_values_are_never_flags() {
        assert_eq!(check(&["-p", "-M"]), Ok(()));
        assert_eq!(check(&["-l", "-F"]), Ok(()));
        assert_eq!(check(&["-i", "-A", "-v"]), Ok(()));
        assert_eq!(check(&["-J", "host"]), Ok(()));
    }

    #[test]
    fn missing_values() {
        assert_eq!(check(&["-p"]), Err(ArgsError::MissingValue('p')));
        assert_eq!(check(&["-vp"]), Err(ArgsError::MissingValue('p')));
        assert_eq!(check(&["-v", "-i"]), Err(ArgsError::MissingValue('i')));
        assert_eq!(check(&["-o"]), Err(ArgsError::MissingValue('o')));
        // A flag that is rejected anyway reports why, not that its value is missing.
        assert_eq!(check(&["-L"]), Err(ArgsError::ForwardingFlag('L')));
        assert_eq!(check(&["-O"]), Err(ArgsError::ReservedFlag('O')));
    }

    #[test]
    fn first_problem_wins() {
        assert_eq!(check(&["-M", "-Z"]), Err(ArgsError::ReservedFlag('M')));
        assert_eq!(check(&["-vZM"]), Err(ArgsError::UnknownFlag('Z')));
        assert_eq!(
            check(&["-A", "-o", "ControlPath=x"]),
            Err(ArgsError::ForwardingFlag('A'))
        );
        assert_eq!(
            check(&["-o", "ControlPath=x", "-A"]),
            Err(ArgsError::ReservedOption("ControlPath".into()))
        );
    }

    #[test]
    fn option_forms() {
        let reserved = Err(ArgsError::ReservedOption("ControlPath".into()));
        for args in [
            &["-oControlPath=/tmp/x"][..],
            &["-o", "ControlPath=/tmp/x"],
            &["-o", "ControlPath /tmp/x"],
            &["-o", "ControlPath = /tmp/x"],
            &["-o", "ControlPath\t/tmp/x"],
            &["-o", "  ControlPath /tmp/x"],
            &["-o", "ControlPath"],
            &["-o", "ControlPath="],
            &["-voControlPath=/tmp/x"],
            &["-vo", "ControlPath=/tmp/x"],
        ] {
            assert_eq!(check(args), reserved, "{args:?}");
        }
        assert_eq!(check(&["-oServerAliveInterval=15"]), Ok(()));
        assert_eq!(option("ServerAliveInterval 15"), Ok(()));
        assert_eq!(option("Compression"), Ok(()));
        assert_eq!(option("Compression="), Ok(()));
    }

    #[test]
    fn malformed_options() {
        for value in [
            "",
            "   ",
            "=x",
            " = x",
            "\"ControlPath\" x",
            "Control\"Path\" x",
            "#x",
        ] {
            assert_eq!(
                option(value),
                Err(ArgsError::MalformedOption(value.into())),
                "{value:?}"
            );
        }
    }

    #[test]
    fn reserved_options_fail() {
        for key in RESERVED_OPTIONS {
            assert_eq!(
                option(&format!("{key}=x")),
                Err(ArgsError::ReservedOption((*key).into()))
            );
        }
    }

    #[test]
    fn forwarding_options_fail() {
        for key in FORWARDING_OPTIONS.iter().chain(&[
            "SomeForwardThing",
            "ForwardX11Timeout",
            "myTUNNELkey",
        ]) {
            assert_eq!(
                option(&format!("{key} yes")),
                Err(ArgsError::ForwardingOption((*key).into()))
            );
        }
        assert_eq!(
            check(&["-o", "SomeForwardThing=1"]),
            Err(ArgsError::ForwardingOption("SomeForwardThing".into()))
        );
    }

    #[test]
    fn option_keys_ignore_case_and_keep_their_spelling() {
        assert_eq!(
            option("forwardagent=yes"),
            Err(ArgsError::ForwardingOption("forwardagent".into()))
        );
        assert_eq!(
            option("CONTROLMASTER auto"),
            Err(ArgsError::ReservedOption("CONTROLMASTER".into()))
        );
        assert_eq!(
            option("gatewayPorts=yes"),
            Err(ArgsError::ForwardingOption("gatewayPorts".into()))
        );
    }

    #[test]
    fn other_options_pass() {
        for value in [
            "UseKeychain=yes",
            "IdentityFile=~/.ssh/id_ed25519",
            "ProxyJump bastion",
            "ConnectTimeout=10",
            "IdentityFile2 x",
        ] {
            assert_eq!(option(value), Ok(()), "{value}");
        }
    }

    #[test]
    fn only_options_are_allowed() {
        for arg in ["host", "-", "--", "", "user@host", " -v"] {
            assert_eq!(check(&[arg]), Err(ArgsError::NotAnOption(arg.into())));
        }
        assert_eq!(
            check(&["-v", "host"]),
            Err(ArgsError::NotAnOption("host".into()))
        );
        assert_eq!(
            check(&["-p", "22", "host"]),
            Err(ArgsError::NotAnOption("host".into()))
        );
        assert_eq!(check(&[]), Ok(()));
    }
}
