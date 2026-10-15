//! What the binary is and the system it runs on, for `noc debug version` and the About page of
//! the help.

/// What was decided when the binary was built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Build {
    pub(crate) version: &'static str,
    /// Cargo features and whether they are on.
    pub(crate) features: &'static [(&'static str, bool)],
    pub(crate) commit: &'static str,
    pub(crate) dirty: &'static str,
    pub(crate) commit_time: &'static str,
    pub(crate) target: &'static str,
    pub(crate) profile: &'static str,
    pub(crate) license: &'static str,
    pub(crate) repository: &'static str,
}

impl Build {
    pub(crate) fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            features: &[("forwarding", noc_ssh::policy::FORWARDING_ENABLED)],
            commit: env!("NOC_BUILD_COMMIT"),
            dirty: env!("NOC_BUILD_DIRTY"),
            commit_time: env!("NOC_BUILD_COMMIT_TIME"),
            target: env!("NOC_BUILD_TARGET"),
            profile: env!("NOC_BUILD_PROFILE"),
            license: env!("CARGO_PKG_LICENSE"),
            repository: env!("CARGO_PKG_REPOSITORY"),
        }
    }

    /// The features, each after `+` when it is on and `-` when it is off.
    pub(crate) fn features_text(&self) -> String {
        self.features
            .iter()
            .map(|&(name, on)| format!("{}{name}", if on { '+' } else { '-' }))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The system the binary runs on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct System {
    /// The distribution, or macOS and its version.
    pub(crate) os: Option<String>,
    pub(crate) kernel: Option<String>,
    pub(crate) arch: Option<String>,
}

impl System {
    /// Reads files on macOS and Linux: call it where blocking is allowed.
    pub(crate) fn current() -> Self {
        let uname = rustix::system::uname();
        let sysname = known(uname.sysname().to_bytes());
        let release = known(uname.release().to_bytes());
        let kernel = match (sysname, release) {
            (Some(sysname), Some(release)) => Some(format!("{sysname} {release}")),
            (sysname, _) => sysname,
        };
        Self {
            os: os_name(),
            kernel,
            arch: known(uname.machine().to_bytes()),
        }
    }
}

/// The macOS version, which `uname` does not give, or the Linux distribution.
fn os_name() -> Option<String> {
    if cfg!(target_os = "macos") {
        let plist =
            std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
        let name = plist_string(&plist, "ProductName")?;
        let version = plist_string(&plist, "ProductVersion")?;
        Some(format!("{name} {version}"))
    } else {
        ["/etc/os-release", "/usr/lib/os-release"]
            .into_iter()
            .find_map(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| os_release_name(&text))
    }
}

/// The `<string>` after `<key>key</key>` in a property list.
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let tag = format!("<key>{key}</key>");
    let rest = plist[plist.find(&tag)? + tag.len()..].trim_start();
    let rest = rest.strip_prefix("<string>")?;
    known(&rest.as_bytes()[..rest.find("</string>")?])
}

/// `PRETTY_NAME` from `os-release`, or `NAME` where it is missing.
fn os_release_name(text: &str) -> Option<String> {
    let value = |key: &str| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
            .and_then(|value| known(unquote(value).as_bytes()))
    };
    value("PRETTY_NAME").or_else(|| value("NAME"))
}

/// An `os-release` value without its quotes and backslashes, as a shell would read it.
fn unquote(value: &str) -> String {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return inner.to_owned();
    }
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    let mut text = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => text.extend(chars.next()),
            c => text.push(c),
        }
    }
    text
}

/// Text from the system, made safe to print on one line; `None` when empty.
pub(crate) fn known(bytes: &[u8]) -> Option<String> {
    let text = noc_text::sanitize(bytes);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{os_release_name, plist_string};

    #[test]
    fn macos_version_comes_from_the_property_list() {
        let plist = "<dict>\n\t<key>ProductName</key>\n\t<string>macOS</string>\n\t\
                     <key>ProductVersion</key>\n\t<string>26.0.1</string>\n</dict>";
        assert_eq!(plist_string(plist, "ProductName").as_deref(), Some("macOS"));
        assert_eq!(
            plist_string(plist, "ProductVersion").as_deref(),
            Some("26.0.1")
        );
        assert_eq!(plist_string(plist, "ProductBuildVersion"), None);
        assert_eq!(
            plist_string("<key>ProductName</key><true/>", "ProductName"),
            None
        );
        assert_eq!(plist_string("", "ProductName"), None);
    }

    #[test]
    fn linux_distribution_comes_from_os_release() {
        let text = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n";
        assert_eq!(os_release_name(text).as_deref(), Some("Ubuntu 24.04.1 LTS"));
        assert_eq!(os_release_name("NAME=Arch\n").as_deref(), Some("Arch"));
        assert_eq!(
            os_release_name("PRETTY_NAME=\"Say \\\"hi\\\"\"\n").as_deref(),
            Some("Say \"hi\"")
        );
        assert_eq!(
            os_release_name("PRETTY_NAME='C:\\\\'\n").as_deref(),
            Some("C:\\\\")
        );
        assert_eq!(
            os_release_name("PRETTY_NAME=\"\"\nNAME=Foo"),
            Some("Foo".to_owned())
        );
        assert_eq!(os_release_name(""), None);
    }
}
