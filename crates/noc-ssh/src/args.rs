//! Validation of user-supplied ssh arguments (`ssh.args` and per-host `args`).
//!
//! See `docs/adr/0004-forwarding-compile-time-feature.md` for the rules.

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
    #[error("ssh flag `-{0}` enables forwarding, which this build does not support")]
    ForwardingFlag(char),
    #[error("malformed ssh option `{0}`")]
    MalformedOption(String),
    #[error("ssh option `{0}` is reserved by Noon Commander")]
    ReservedOption(String),
    #[error("ssh option `{0}` enables forwarding, which this build does not support")]
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

/// Checks user-supplied ssh arguments against the policy of this build.
pub fn validate(args: &[String]) -> Result<(), ArgsError> {
    validate_with(args, crate::policy::FORWARDING_ENABLED)
}

/// Like [`validate`], with forwarding allowed or not regardless of the build.
pub(crate) fn validate_with(args: &[String], allow_forwarding: bool) -> Result<(), ArgsError> {
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
                Class::Forwarding if !allow_forwarding => {
                    return Err(ArgsError::ForwardingFlag(flag));
                }
                Class::Allowed | Class::Forwarding | Class::ConfigOption => {}
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
                    check_option(value, allow_forwarding)?;
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
fn check_option(option: &str, allow_forwarding: bool) -> Result<(), ArgsError> {
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
    if forwards && !allow_forwarding {
        return Err(ArgsError::ForwardingOption(key.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::policy::FORWARDING_ENABLED;

    const ALLOWED_FLAGS: &[char] = &['4', '6', 'a', 'C', 'k', 'q', 'v', 'x'];
    const ALLOWED_VALUE_FLAGS: &[char] = &['B', 'b', 'c', 'e', 'I', 'i', 'J', 'l', 'm', 'P', 'p'];
    const RESERVED_FLAGS: &[char] = &['f', 'G', 'M', 'N', 'n', 's', 'T', 't', 'V', 'y'];
    const RESERVED_VALUE_FLAGS: &[char] = &['E', 'O', 'Q', 'S', 'W'];
    const FORWARDING_FLAGS: &[char] = &['A', 'g', 'K', 'X', 'Y'];
    const FORWARDING_VALUE_FLAGS: &[char] = &['D', 'L', 'R', 'w'];

    fn check(args: &[&str], allow_forwarding: bool) -> Result<(), ArgsError> {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        validate_with(&args, allow_forwarding)
    }

    fn option(value: &str, allow_forwarding: bool) -> Result<(), ArgsError> {
        check(&["-o", value], allow_forwarding)
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
        for allow_forwarding in [false, true] {
            for flag in ALLOWED_FLAGS {
                assert_eq!(check(&[&format!("-{flag}")], allow_forwarding), Ok(()));
            }
            for flag in ALLOWED_VALUE_FLAGS {
                assert_eq!(check(&[&format!("-{flag}"), "x"], allow_forwarding), Ok(()));
                assert_eq!(check(&[&format!("-{flag}x")], allow_forwarding), Ok(()));
            }
        }
    }

    #[test]
    fn reserved_flags_fail_in_every_build() {
        for allow_forwarding in [false, true] {
            for &flag in RESERVED_FLAGS {
                assert_eq!(
                    check(&[&format!("-{flag}")], allow_forwarding),
                    Err(ArgsError::ReservedFlag(flag))
                );
            }
            for &flag in RESERVED_VALUE_FLAGS {
                assert_eq!(
                    check(&[&format!("-{flag}"), "x"], allow_forwarding),
                    Err(ArgsError::ReservedFlag(flag))
                );
                assert_eq!(
                    check(&[&format!("-{flag}x")], allow_forwarding),
                    Err(ArgsError::ReservedFlag(flag))
                );
            }
        }
    }

    #[test]
    fn config_file_flag_fails() {
        for allow_forwarding in [false, true] {
            for args in [&["-F", "x"][..], &["-Fx"], &["-vF", "x"], &["-F"]] {
                assert_eq!(
                    check(args, allow_forwarding),
                    Err(ArgsError::ConfigFileFlag)
                );
            }
        }
    }

    #[test]
    fn forwarding_flags_need_the_feature() {
        for &flag in FORWARDING_FLAGS {
            let arg = format!("-{flag}");
            assert_eq!(check(&[&arg], false), Err(ArgsError::ForwardingFlag(flag)));
            assert_eq!(check(&[&arg], true), Ok(()));
        }
        for &flag in FORWARDING_VALUE_FLAGS {
            let arg = format!("-{flag}");
            let attached = format!("-{flag}8080:localhost:80");
            assert_eq!(
                check(&[&arg, "8080:localhost:80"], false),
                Err(ArgsError::ForwardingFlag(flag))
            );
            assert_eq!(
                check(&[&attached], false),
                Err(ArgsError::ForwardingFlag(flag))
            );
            assert_eq!(check(&[&arg, "8080:localhost:80"], true), Ok(()));
            assert_eq!(check(&[&attached], true), Ok(()));
        }
    }

    #[test]
    fn unknown_flags_fail() {
        for allow_forwarding in [false, true] {
            for (arg, flag) in [("-Z", 'Z'), ("-h", 'h'), ("-1", '1'), ("-é", 'é')] {
                assert_eq!(
                    check(&[arg], allow_forwarding),
                    Err(ArgsError::UnknownFlag(flag))
                );
            }
            assert_eq!(
                check(&["--verbose"], allow_forwarding),
                Err(ArgsError::UnknownFlag('-'))
            );
        }
    }

    #[test]
    fn bundled_flags() {
        assert_eq!(check(&["-4Cv"], false), Ok(()));
        assert_eq!(check(&["-4X"], false), Err(ArgsError::ForwardingFlag('X')));
        assert_eq!(check(&["-4X"], true), Ok(()));
        assert_eq!(check(&["-vp22"], false), Ok(()));
        assert_eq!(check(&["-vp", "22"], false), Ok(()));
        assert_eq!(check(&["-vM"], true), Err(ArgsError::ReservedFlag('M')));
        // The rest of the argument is a value, never more flags.
        assert_eq!(check(&["-pX"], false), Ok(()));
        assert_eq!(check(&["-lMN"], false), Ok(()));
        assert_eq!(check(&["-vlA", "-x"], false), Ok(()));
    }

    #[test]
    fn separate_values_are_never_flags() {
        assert_eq!(check(&["-p", "-M"], false), Ok(()));
        assert_eq!(check(&["-l", "-F"], false), Ok(()));
        assert_eq!(check(&["-i", "-A", "-v"], false), Ok(()));
        assert_eq!(check(&["-J", "host"], false), Ok(()));
    }

    #[test]
    fn missing_values() {
        assert_eq!(check(&["-p"], false), Err(ArgsError::MissingValue('p')));
        assert_eq!(check(&["-vp"], false), Err(ArgsError::MissingValue('p')));
        assert_eq!(
            check(&["-v", "-i"], false),
            Err(ArgsError::MissingValue('i'))
        );
        assert_eq!(check(&["-o"], false), Err(ArgsError::MissingValue('o')));
        assert_eq!(check(&["-L"], true), Err(ArgsError::MissingValue('L')));
        // A flag that is rejected anyway reports why, not that its value is missing.
        assert_eq!(check(&["-L"], false), Err(ArgsError::ForwardingFlag('L')));
        assert_eq!(check(&["-O"], true), Err(ArgsError::ReservedFlag('O')));
    }

    #[test]
    fn first_problem_wins() {
        assert_eq!(
            check(&["-M", "-Z"], false),
            Err(ArgsError::ReservedFlag('M'))
        );
        assert_eq!(check(&["-vZM"], false), Err(ArgsError::UnknownFlag('Z')));
        assert_eq!(
            check(&["-A", "-o", "ControlPath=x"], false),
            Err(ArgsError::ForwardingFlag('A'))
        );
        assert_eq!(
            check(&["-A", "-o", "ControlPath=x"], true),
            Err(ArgsError::ReservedOption("ControlPath".into()))
        );
    }

    #[test]
    fn option_forms() {
        for allow_forwarding in [false, true] {
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
                assert_eq!(check(args, allow_forwarding), reserved, "{args:?}");
            }
            assert_eq!(
                check(&["-oServerAliveInterval=15"], allow_forwarding),
                Ok(())
            );
            assert_eq!(option("ServerAliveInterval 15", allow_forwarding), Ok(()));
            assert_eq!(option("Compression", allow_forwarding), Ok(()));
            assert_eq!(option("Compression=", allow_forwarding), Ok(()));
        }
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
                option(value, true),
                Err(ArgsError::MalformedOption(value.into())),
                "{value:?}"
            );
        }
    }

    #[test]
    fn reserved_options_fail_in_every_build() {
        for allow_forwarding in [false, true] {
            for key in RESERVED_OPTIONS {
                assert_eq!(
                    option(&format!("{key}=x"), allow_forwarding),
                    Err(ArgsError::ReservedOption((*key).into()))
                );
            }
        }
    }

    #[test]
    fn forwarding_options_need_the_feature() {
        for key in FORWARDING_OPTIONS.iter().chain(&[
            "SomeForwardThing",
            "ForwardX11Timeout",
            "myTUNNELkey",
        ]) {
            let value = format!("{key} yes");
            assert_eq!(
                option(&value, false),
                Err(ArgsError::ForwardingOption((*key).into()))
            );
            assert_eq!(option(&value, true), Ok(()));
        }
        assert_eq!(
            check(&["-o", "SomeForwardThing=1"], false),
            Err(ArgsError::ForwardingOption("SomeForwardThing".into()))
        );
    }

    #[test]
    fn option_keys_ignore_case_and_keep_their_spelling() {
        assert_eq!(
            option("forwardagent=yes", false),
            Err(ArgsError::ForwardingOption("forwardagent".into()))
        );
        assert_eq!(
            option("CONTROLMASTER auto", true),
            Err(ArgsError::ReservedOption("CONTROLMASTER".into()))
        );
        assert_eq!(
            option("gatewayPorts=yes", false),
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
            assert_eq!(option(value, false), Ok(()), "{value}");
        }
    }

    #[test]
    fn only_options_are_allowed() {
        for allow_forwarding in [false, true] {
            for arg in ["host", "-", "--", "", "user@host", " -v"] {
                assert_eq!(
                    check(&[arg], allow_forwarding),
                    Err(ArgsError::NotAnOption(arg.into()))
                );
            }
            assert_eq!(
                check(&["-v", "host"], allow_forwarding),
                Err(ArgsError::NotAnOption("host".into()))
            );
            assert_eq!(
                check(&["-p", "22", "host"], allow_forwarding),
                Err(ArgsError::NotAnOption("host".into()))
            );
            assert_eq!(check(&[], allow_forwarding), Ok(()));
        }
    }

    #[test]
    fn validate_follows_the_build() {
        let cases: [&[&str]; 6] = [
            &["-v"],
            &["-A"],
            &["-L", "8080:localhost:80"],
            &["-o", "ForwardAgent=yes"],
            &["-M"],
            &["host"],
        ];
        for case in cases {
            let args: Vec<String> = case.iter().map(ToString::to_string).collect();
            assert_eq!(validate(&args), validate_with(&args, FORWARDING_ENABLED));
        }
        let forward_agent = ["-A".to_owned()];
        assert_eq!(validate(&forward_agent).is_ok(), FORWARDING_ENABLED);
    }
}
