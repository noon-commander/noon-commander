//! Host discovery: lists the host aliases defined in `ssh_config` files.
//!
//! OpenSSH cannot list hosts, so this module scans the config files itself. It reads only
//! `Host`, `Match`, and `Include`; effective settings always come from `ssh -G`.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A host alias found on an `ssh_config` `Host` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredHost {
    /// The first concrete pattern on the `Host` line.
    pub alias: String,
    /// Other concrete patterns on the same line.
    pub other_names: Vec<String>,
    /// File that contains the `Host` line.
    pub file: PathBuf,
    /// 1-based line number of the `Host` line.
    pub line: usize,
}

/// Something the scanner skipped. Never fatal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryWarning {
    /// A file exists but could not be read.
    Unreadable { file: PathBuf, error: String },
    /// An `Include` path uses `%` tokens, which only ssh can expand for a given host.
    IncludeWithTokens {
        file: PathBuf,
        line: usize,
        path: String,
    },
    /// An `Include` path references an environment variable that is not set.
    UndefinedVariable {
        file: PathBuf,
        line: usize,
        name: String,
    },
    /// Includes are nested deeper than ssh allows (16 levels).
    IncludeTooDeep { file: PathBuf, line: usize },
}

/// Result of a discovery run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Discovery {
    /// Hosts in config order. A name is listed once, at its first definition.
    pub hosts: Vec<DiscoveredHost>,
    /// Every file that was read, for cache invalidation.
    pub files: Vec<PathBuf>,
    /// Problems that were skipped.
    pub warnings: Vec<DiscoveryWarning>,
}

impl Discovery {
    /// Hosts whose alias matches none of the `hide` patterns.
    pub fn visible<'a>(
        &'a self,
        hide: &'a [String],
    ) -> impl Iterator<Item = &'a DiscoveredHost> + 'a {
        self.hosts.iter().filter(move |host| {
            !hide
                .iter()
                .any(|pattern| crate::pattern::wildcard_match(pattern, &host.alias))
        })
    }
}

/// Where to look for `ssh_config` files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryOptions {
    /// `ssh.config_file`: when set, replaces both the user and the system config, like `ssh -F`.
    pub config_file: Option<PathBuf>,
    /// The user's home directory. `~` expands to it, and relative includes from user files
    /// resolve against `<home>/.ssh`.
    pub home: PathBuf,
    /// The system-wide config, normally `/etc/ssh/ssh_config`. Relative includes from system
    /// files resolve against its directory.
    pub system_config: PathBuf,
}

impl DiscoveryOptions {
    /// Options for the standard locations.
    pub fn new(home: PathBuf, config_file: Option<PathBuf>) -> Self {
        Self {
            config_file,
            home,
            system_config: PathBuf::from("/etc/ssh/ssh_config"),
        }
    }

    /// The top-level config files ssh reads: `config_file`, unless it is `none`, or
    /// `<home>/.ssh/config` and `system_config`, whether they exist or not. Include them in the
    /// files of a [`ConfigStamp`](crate::ConfigStamp), so that creating a missing config file
    /// changes it.
    pub fn root_files(&self) -> Vec<PathBuf> {
        self.roots().into_iter().map(|(file, _)| file).collect()
    }

    fn roots(&self) -> Vec<(PathBuf, Origin)> {
        match &self.config_file {
            // `ssh -F none` reads no config at all.
            Some(file) if file.as_os_str().eq_ignore_ascii_case("none") => Vec::new(),
            Some(file) => vec![(file.clone(), Origin::User)],
            None => vec![
                (self.home.join(".ssh").join("config"), Origin::User),
                (self.system_config.clone(), Origin::System),
            ],
        }
    }
}

/// How deep ssh nests includes; the top-level file is at depth 0.
const MAX_INCLUDE_DEPTH: usize = 16;

/// Scans `ssh_config` files for host aliases.
///
/// Blocking: call it from a blocking thread. `env` looks up environment variables for
/// `${NAME}` expansion in `Include` paths.
pub fn discover(options: &DiscoveryOptions, env: &dyn Fn(&str) -> Option<String>) -> Discovery {
    let mut scanner = Scanner {
        options,
        env,
        discovery: Discovery::default(),
        names: HashSet::new(),
        reads: HashSet::new(),
    };
    for (file, origin) in options.roots() {
        scanner.read_file(&file, origin, 0);
    }
    scanner.discovery
}

/// Whether a file belongs to the user's or the system-wide config. Included files inherit it
/// from the file that includes them; it decides where relative `Include` paths point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Origin {
    User,
    System,
}

struct Scanner<'a> {
    options: &'a DiscoveryOptions,
    env: &'a dyn Fn(&str) -> Option<String>,
    discovery: Discovery,
    /// Every name listed so far, in ASCII lowercase.
    names: HashSet<String>,
    /// Every `(file, origin, depth)` scanned so far.
    reads: HashSet<(PathBuf, Origin, usize)>,
}

impl Scanner<'_> {
    fn read_file(&mut self, path: &Path, origin: Origin, depth: usize) {
        // Reading a file again with the same origin and depth cannot find anything new. Without
        // this, include cycles through globs would take exponential time to reach the depth limit.
        if !self.reads.insert((path.to_owned(), origin, depth)) {
            return;
        }
        let bytes = match read_config(path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return,
            Err(error) => {
                self.warn(DiscoveryWarning::Unreadable {
                    file: path.to_owned(),
                    error: error.to_string(),
                });
                return;
            }
        };
        if !self.discovery.files.iter().any(|file| file == path) {
            self.discovery.files.push(path.to_owned());
        }
        for (index, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
            let Some((keyword, rest)) = split_keyword(line) else {
                continue;
            };
            if keyword.eq_ignore_ascii_case("host") {
                self.add_host(split_arguments(rest), path, index + 1);
            } else if keyword.eq_ignore_ascii_case("include") {
                self.include(split_arguments(rest), path, index + 1, origin, depth);
            }
        }
    }

    fn add_host(&mut self, patterns: Vec<String>, file: &Path, line: usize) {
        let mut names = Vec::new();
        for pattern in patterns {
            if crate::pattern::is_concrete(&pattern)
                && self.names.insert(pattern.to_ascii_lowercase())
            {
                names.push(pattern);
            }
        }
        let mut names = names.into_iter();
        if let Some(alias) = names.next() {
            self.discovery.hosts.push(DiscoveredHost {
                alias,
                other_names: names.collect(),
                file: file.to_owned(),
                line,
            });
        }
    }

    fn include(
        &mut self,
        paths: Vec<String>,
        file: &Path,
        line: usize,
        origin: Origin,
        depth: usize,
    ) {
        for path in paths {
            if path.contains('%') {
                self.warn(DiscoveryWarning::IncludeWithTokens {
                    file: file.to_owned(),
                    line,
                    path,
                });
                continue;
            }
            let path = match expand_variables(&path, self.env) {
                Ok(path) => path,
                Err(name) => {
                    self.warn(DiscoveryWarning::UndefinedVariable {
                        file: file.to_owned(),
                        line,
                        name,
                    });
                    continue;
                }
            };
            let Some(pattern) = self.resolve(&path, origin) else {
                continue;
            };
            for included in expand_glob(&pattern) {
                if fs::metadata(&included).is_ok_and(|metadata| !metadata.is_file()) {
                    continue;
                }
                if depth >= MAX_INCLUDE_DEPTH {
                    self.warn(DiscoveryWarning::IncludeTooDeep {
                        file: file.to_owned(),
                        line,
                    });
                    break;
                }
                self.read_file(&included, origin, depth + 1);
            }
        }
    }

    /// Anchors an `Include` path the way ssh does. Other users' homes (`~user`) are not
    /// supported.
    fn resolve(&self, path: &str, origin: Origin) -> Option<PathBuf> {
        let home = &self.options.home;
        if let Some(rest) = path.strip_prefix('~') {
            if rest.is_empty() {
                return Some(home.clone());
            }
            let rest = rest.strip_prefix('/')?;
            return Some(home.join(rest.trim_start_matches('/')));
        }
        let path = Path::new(path);
        if path.is_absolute() {
            return Some(path.to_owned());
        }
        let base = match origin {
            Origin::User => home.join(".ssh"),
            Origin::System => self
                .options
                .system_config
                .parent()
                .unwrap_or(Path::new("/"))
                .to_owned(),
        };
        Some(base.join(path))
    }

    fn warn(&mut self, warning: DiscoveryWarning) {
        if !self.discovery.warnings.contains(&warning) {
            self.discovery.warnings.push(warning);
        }
    }
}

/// Reads a config file, or returns `None` if it does not exist. Special files such as
/// `/dev/null` count as empty: reading a FIFO could block forever. Directories are errors.
fn read_config(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let result = fs::metadata(path).and_then(|metadata| {
        if metadata.is_file() || metadata.is_dir() {
            fs::read(path).map(Some)
        } else {
            Ok(None)
        }
    });
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        result => result,
    }
}

/// Splits a line into its keyword and the rest, or returns `None` for blank and comment lines.
/// Like ssh, the keyword ends at whitespace or `=`, so `Host=a` and `Host = a` both work.
fn split_keyword(line: &str) -> Option<(&str, &str)> {
    let line = line
        .trim_start_matches(is_space)
        .trim_end_matches(|c: char| is_space(c) || c == '\x0c');
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let end = line
        .find(|c: char| is_space(c) || c == '=')
        .unwrap_or(line.len());
    let (keyword, rest) = line.split_at(end);
    let rest = rest.trim_start_matches(is_space);
    Some((keyword, rest.strip_prefix('=').unwrap_or(rest)))
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

/// Splits arguments like ssh does: at spaces and tabs, with single and double quotes grouping,
/// and backslash escaping quotes, backslashes, and (outside quotes) spaces. An unquoted argument
/// that starts with `#` begins a comment.
fn split_arguments(text: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.next_if(|&c| c == ' ' || c == '\t').is_some() {}
        if chars.peek().is_none_or(|&c| c == '#') {
            return arguments;
        }
        let mut argument = String::new();
        let mut quote = None;
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.peek() {
                    Some(&escaped @ ('\'' | '"' | '\\')) => {
                        argument.push(escaped);
                        chars.next();
                    }
                    Some(' ') if quote.is_none() => {
                        argument.push(' ');
                        chars.next();
                    }
                    _ => argument.push('\\'),
                },
                ' ' | '\t' if quote.is_none() => break,
                '"' | '\'' if quote.is_none() => quote = Some(c),
                _ if quote == Some(c) => quote = None,
                _ => argument.push(c),
            }
        }
        arguments.push(argument);
    }
}

/// Expands `${NAME}` references. On failure, returns the name that could not be expanded.
fn expand_variables(path: &str, env: &dyn Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut expanded = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find("${") {
        expanded.push_str(&rest[..start]);
        let reference = &rest[start + 2..];
        let Some(end) = reference.find('}') else {
            return Err(reference.to_owned());
        };
        let name = &reference[..end];
        let value = if name.is_empty() { None } else { env(name) };
        let Some(value) = value else {
            return Err(name.to_owned());
        };
        expanded.push_str(&value);
        rest = &reference[end + 1..];
    }
    expanded.push_str(rest);
    Ok(expanded)
}

/// Expands wildcards like glob(3), which ssh uses: a wildcard never matches a leading dot, `**`
/// is not recursive, and matches are sorted by their bytes. Returns only paths that exist.
///
/// `glob::glob` differs on all three points, and with `require_literal_leading_dot` it panics on
/// file names that are not UTF-8, so only its pattern matching is used.
fn expand_glob(pattern: &Path) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::new()];
    for component in pattern.components() {
        let component = component.as_os_str();
        if let Some(wildcard) = wildcard(component) {
            paths = paths
                .iter()
                .flat_map(|dir| matching_entries(dir, &wildcard))
                .collect();
        } else {
            for path in &mut paths {
                path.push(component);
            }
        }
    }
    paths.retain(|path| fs::symlink_metadata(path).is_ok());
    paths.sort_by(|a, b| {
        a.as_os_str()
            .as_encoded_bytes()
            .cmp(b.as_os_str().as_encoded_bytes())
    });
    paths
}

/// Compiles a path component that contains wildcards. Runs of `*` are collapsed into one, and an
/// invalid pattern, like an unclosed `[`, is taken literally, as glob(3) does.
fn wildcard(component: &OsStr) -> Option<glob::Pattern> {
    let text = component.to_str()?;
    if !text.contains(['*', '?', '[']) {
        return None;
    }
    let mut pattern = String::with_capacity(text.len());
    for c in text.chars() {
        if c != '*' || !pattern.ends_with('*') {
            pattern.push(c);
        }
    }
    glob::Pattern::new(&pattern).ok()
}

fn matching_entries(dir: &Path, wildcard: &glob::Pattern) -> Vec<PathBuf> {
    let listed = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let Ok(entries) = fs::read_dir(listed) else {
        return Vec::new();
    };
    // glob(3) matches a leading dot only with a literal dot at the start of the pattern.
    let dot_allowed = wildcard.as_str().starts_with('.');
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .filter(|name| {
            let name = name.to_string_lossy();
            (dot_allowed || !name.starts_with('.')) && wildcard.matches(&name)
        })
        .map(|name| dir.join(name))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;

    /// A scratch directory with the user config under `home/.ssh/` and the system config at
    /// `etc/ssh/ssh_config`.
    struct Tree(TempDir);

    impl Tree {
        fn new() -> Self {
            Self(TempDir::new().unwrap())
        }

        fn path(&self, relative: &str) -> PathBuf {
            self.0.path().join(relative)
        }

        fn write_bytes(&self, relative: &str, contents: &[u8]) -> PathBuf {
            let path = self.path(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }

        fn write(&self, relative: &str, lines: &[&str]) -> PathBuf {
            self.write_bytes(relative, (lines.join("\n") + "\n").as_bytes())
        }

        fn mkdir(&self, relative: &str) -> PathBuf {
            let path = self.path(relative);
            fs::create_dir_all(&path).unwrap();
            path
        }

        fn options(&self) -> DiscoveryOptions {
            DiscoveryOptions {
                config_file: None,
                home: self.path("home"),
                system_config: self.path("etc/ssh/ssh_config"),
            }
        }

        fn discover(&self) -> Discovery {
            discover(&self.options(), &no_env)
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn host(alias: &str, other_names: &[&str], file: &Path, line: usize) -> DiscoveredHost {
        DiscoveredHost {
            alias: alias.to_owned(),
            other_names: other_names.iter().map(ToString::to_string).collect(),
            file: file.to_owned(),
            line,
        }
    }

    fn aliases(discovery: &Discovery) -> Vec<&str> {
        discovery
            .hosts
            .iter()
            .map(|host| host.alias.as_str())
            .collect()
    }

    fn too_deep(file: PathBuf, line: usize) -> DiscoveryWarning {
        DiscoveryWarning::IncludeTooDeep { file, line }
    }

    /// Writes `home/.ssh/1.conf` … `<n>.conf`, where each file defines `h<i>` and includes the
    /// next one.
    fn write_chain(tree: &Tree, n: usize) {
        for i in 1..=n {
            tree.write(
                &format!("home/.ssh/{i}.conf"),
                &[&format!("Host h{i}"), &format!("Include {}.conf", i + 1)],
            );
        }
    }

    #[test]
    fn splits_lines_like_ssh() {
        for line in ["", " \t ", "# Host a", "   #Host a", "\r"] {
            assert_eq!(split_keyword(line), None, "{line:?}");
        }
        let cases: &[(&str, &str, &[&str])] = &[
            ("Host a", "Host", &["a"]),
            ("Host=a", "Host", &["a"]),
            ("Host = a", "Host", &["a"]),
            ("Host =a", "Host", &["a"]),
            ("Host==a", "Host", &["=a"]),
            (" \tInclude\tx  y \r", "Include", &["x", "y"]),
            ("Host a\x0c", "Host", &["a"]),
            ("Host", "Host", &[]),
            ("Host=", "Host", &[]),
        ];
        for &(line, keyword, arguments) in cases {
            let (parsed, rest) = split_keyword(line).unwrap();
            assert_eq!(parsed, keyword, "{line:?}");
            assert_eq!(split_arguments(rest), arguments, "{line:?}");
        }
    }

    #[test]
    fn splits_arguments_like_ssh() {
        let cases: &[(&str, &[&str])] = &[
            ("", &[]),
            ("a b\tc", &["a", "b", "c"]),
            ("  a  ", &["a"]),
            ("\"a b\" 'c d'", &["a b", "c d"]),
            ("a\"b c\"d", &["ab cd"]),
            ("'say \"hi\"'", &["say \"hi\""]),
            ("a\\ b", &["a b"]),
            ("\"a\\ b\"", &["a\\ b"]),
            ("\\\"a \\'b \\\\c", &["\"a", "'b", "\\c"]),
            ("a\\b", &["a\\b"]),
            ("a # comment", &["a"]),
            ("a#b", &["a#b"]),
            ("\"#a\" b", &["#a", "b"]),
            ("#a", &[]),
            ("\"\"", &[""]),
            ("\"open", &["open"]),
        ];
        for &(text, expected) in cases {
            assert_eq!(split_arguments(text), expected, "{text:?}");
        }
    }

    #[test]
    fn lists_hosts_in_config_order() {
        let tree = Tree::new();
        let config = tree.write(
            "home/.ssh/config",
            &[
                "Host alpha",
                "    HostName 10.0.0.1",
                "    User deploy",
                "",
                "Host beta gamma",
                "host delta",
            ],
        );
        let discovery = tree.discover();
        assert_eq!(
            discovery.hosts,
            [
                host("alpha", &[], &config, 1),
                host("beta", &["gamma"], &config, 5),
                host("delta", &[], &config, 6),
            ]
        );
        assert_eq!(discovery.files, [config]);
        assert_eq!(discovery.warnings, []);
    }

    #[test]
    fn skips_wildcards_and_negations() {
        let tree = Tree::new();
        let config = tree.write(
            "home/.ssh/config",
            &[
                "Host *",
                "Host *.example.com !bastion web?",
                "Host a *.x b",
                "Host !c d",
            ],
        );
        assert_eq!(
            tree.discover().hosts,
            [host("a", &["b"], &config, 3), host("d", &[], &config, 4)]
        );
    }

    #[test]
    fn first_definition_wins() {
        let tree = Tree::new();
        let config = tree.write(
            "home/.ssh/config",
            &["Host a b", "Host B c", "Host A", "Host a d A D"],
        );
        let system = tree.write("etc/ssh/ssh_config", &["Host c", "Host e"]);
        assert_eq!(
            tree.discover().hosts,
            [
                host("a", &["b"], &config, 1),
                host("c", &[], &config, 2),
                host("d", &[], &config, 4),
                host("e", &[], &system, 2),
            ]
        );
    }

    #[test]
    fn parses_keywords_and_arguments() {
        let tree = Tree::new();
        let config = tree.write(
            "home/.ssh/config",
            &[
                "Host=one",
                "Host = two",
                "HOST three",
                "  \thOsT\tfour   ",
                "Host \"five six\" 'seven' eight # Host nine",
                "Host ten#eleven \"#twelve\"",
                "# Host thirteen",
                "Hostname fourteen",
                "HostKeyAlias fifteen",
                "Host six\\ teen",
                "Host",
            ],
        );
        assert_eq!(
            tree.discover().hosts,
            [
                host("one", &[], &config, 1),
                host("two", &[], &config, 2),
                host("three", &[], &config, 3),
                host("four", &[], &config, 4),
                host("five six", &["seven", "eight"], &config, 5),
                host("ten#eleven", &["#twelve"], &config, 6),
                host("six teen", &[], &config, 10),
            ]
        );
    }

    #[test]
    fn decodes_files_leniently() {
        let tree = Tree::new();
        let config = tree.write_bytes(
            "home/.ssh/config",
            b"Host a\r\nHost b\xff c\r\n\r\nHost d\r",
        );
        assert_eq!(
            tree.discover().hosts,
            [
                host("a", &[], &config, 1),
                host("b\u{fffd}", &["c"], &config, 2),
                host("d", &[], &config, 4),
            ]
        );
    }

    #[test]
    fn ignores_match_but_follows_includes_in_any_block() {
        let tree = Tree::new();
        let config = tree.write(
            "home/.ssh/config",
            &[
                "Match host one exec \"true\"",
                "  Include match.conf",
                "Match all",
                "Host two",
                "  Include host.conf",
            ],
        );
        let in_match = tree.write("home/.ssh/match.conf", &["Host three"]);
        let in_host = tree.write("home/.ssh/host.conf", &["Host four"]);
        assert_eq!(
            tree.discover().hosts,
            [
                host("three", &[], &in_match, 1),
                host("two", &[], &config, 4),
                host("four", &[], &in_host, 1),
            ]
        );
    }

    #[test]
    fn resolves_include_paths() {
        let tree = Tree::new();
        let absolute = tree.write("elsewhere/absolute.conf", &["Host absolute"]);
        let absolute_line = format!("Include \"{}\"", absolute.display());
        let config = tree.write(
            "home/.ssh/config",
            &[
                "Include relative.conf",
                &absolute_line,
                "Include ~/tilde.conf",
                "Include ${CONF_DIR}/variable.conf",
                "Include first.conf \"second file.conf\"",
                "Include ~",
            ],
        );
        let relative = tree.write("home/.ssh/relative.conf", &["Host relative"]);
        let tilde = tree.write("home/tilde.conf", &["Host tilde"]);
        let variable = tree.write("vars/variable.conf", &["Host variable"]);
        let first = tree.write("home/.ssh/first.conf", &["Host first"]);
        let second = tree.write("home/.ssh/second file.conf", &["Host second"]);
        let vars = tree.path("vars");
        let env = |name: &str| (name == "CONF_DIR").then(|| vars.display().to_string());

        let discovery = discover(&tree.options(), &env);
        assert_eq!(
            aliases(&discovery),
            [
                "relative", "absolute", "tilde", "variable", "first", "second"
            ]
        );
        assert_eq!(
            discovery.files,
            [config, relative, absolute, tilde, variable, first, second]
        );
        assert_eq!(discovery.warnings, []);
    }

    #[test]
    fn reports_includes_that_cannot_be_expanded() {
        let tree = Tree::new();
        let config = tree.write(
            "home/.ssh/config",
            &[
                "Include conf.d/%h.conf",
                "Include ${UNSET}/unset.conf",
                "Include ${SET}/set.conf ${ALSO_UNSET}/x.conf",
                "Include ${}/empty.conf",
                "Include ${OPEN/open.conf",
                "Host after",
            ],
        );
        tree.write("vars/set.conf", &["Host set"]);
        let vars = tree.path("vars");
        let env = |name: &str| (name == "SET").then(|| vars.display().to_string());

        let discovery = discover(&tree.options(), &env);
        assert_eq!(aliases(&discovery), ["set", "after"]);
        let undefined = |line, name: &str| DiscoveryWarning::UndefinedVariable {
            file: config.clone(),
            line,
            name: name.to_owned(),
        };
        assert_eq!(
            discovery.warnings,
            [
                DiscoveryWarning::IncludeWithTokens {
                    file: config.clone(),
                    line: 1,
                    path: "conf.d/%h.conf".to_owned(),
                },
                undefined(2, "UNSET"),
                undefined(3, "ALSO_UNSET"),
                undefined(4, ""),
                undefined(5, "OPEN/open.conf"),
            ]
        );
    }

    #[test]
    fn expands_include_globs_in_order() {
        let tree = Tree::new();
        let config = tree.write(
            "home/.ssh/config",
            &[
                "Include conf.d/*",
                "Include nothing/* missing.conf",
                "Include conf.d/.h*",
            ],
        );
        let b = tree.write("home/.ssh/conf.d/b", &["Host b"]);
        let a = tree.write("home/.ssh/conf.d/a", &["Host a"]);
        let c = tree.write("home/.ssh/conf.d/c.conf", &["Host c"]);
        let hidden = tree.write("home/.ssh/conf.d/.hidden", &["Host hidden"]);
        tree.write("home/.ssh/conf.d/dir.conf/nested", &["Host nested"]);

        let discovery = tree.discover();
        assert_eq!(aliases(&discovery), ["a", "b", "c", "hidden"]);
        assert_eq!(discovery.files, [config, a, b, c, hidden]);
        assert_eq!(discovery.warnings, []);
    }

    #[test]
    fn expands_wildcard_directories() {
        let tree = Tree::new();
        tree.write("home/.ssh/config", &["Include ~/.lima/*/ssh.config"]);
        tree.write("home/.lima/two/ssh.config", &["Host lima-two"]);
        tree.write("home/.lima/one/ssh.config", &["Host lima-one"]);
        tree.write("home/.lima/.hidden/ssh.config", &["Host lima-hidden"]);
        tree.mkdir("home/.lima/empty");
        tree.write("home/.lima/file", &["Host lima-file"]);
        assert_eq!(aliases(&tree.discover()), ["lima-one", "lima-two"]);
    }

    #[test]
    fn sorts_glob_matches_by_whole_path() {
        let tree = Tree::new();
        tree.write("home/.ssh/config", &["Include hosts/*/conf"]);
        tree.write("home/.ssh/hosts/a/conf", &["Host a"]);
        tree.write("home/.ssh/hosts/a-b/conf", &["Host a-b"]);
        // `-` sorts before `/`, so glob(3) returns `a-b/conf` first.
        assert_eq!(aliases(&tree.discover()), ["a-b", "a"]);
    }

    #[test]
    fn wildcards_work_like_glob3() {
        let tree = Tree::new();
        tree.write(
            "home/.ssh/config",
            &["Include d/**.conf", "Include d/[ab].x", "Include d/[c"],
        );
        tree.write("home/.ssh/d/one.conf", &["Host one"]);
        tree.write("home/.ssh/d/b.x", &["Host b"]);
        tree.write("home/.ssh/d/[c", &["Host bracket"]);
        assert_eq!(aliases(&tree.discover()), ["one", "b", "bracket"]);
    }

    #[test]
    fn origin_decides_where_relative_includes_point() {
        let tree = Tree::new();
        tree.write("home/.ssh/config", &["Include nested.conf"]);
        tree.write("home/.ssh/nested.conf", &["Host user-nested"]);
        tree.write(
            "etc/ssh/ssh_config",
            &["Include ssh_config.d/*.conf", "Host system"],
        );
        tree.write(
            "etc/ssh/ssh_config.d/10-a.conf",
            &["Host system-dir", "Include nested.conf"],
        );
        tree.write("etc/ssh/nested.conf", &["Host system-nested"]);
        assert_eq!(
            aliases(&tree.discover()),
            ["user-nested", "system-dir", "system-nested", "system"]
        );
    }

    #[test]
    fn self_include_is_too_deep() {
        let tree = Tree::new();
        let config = tree.write(
            "home/.ssh/config",
            &["Host before", "Include config", "Host after"],
        );
        let discovery = tree.discover();
        assert_eq!(
            discovery.hosts,
            [
                host("before", &[], &config, 1),
                host("after", &[], &config, 3),
            ]
        );
        assert_eq!(discovery.warnings, [too_deep(config.clone(), 2)]);
        assert_eq!(discovery.files, [config]);
    }

    #[test]
    fn includes_nest_at_most_16_deep() {
        let tree = Tree::new();
        tree.write("home/.ssh/config", &["Include 1.conf"]);
        write_chain(&tree, 17);
        let discovery = tree.discover();
        let expected: Vec<String> = (1..=16).map(|i| format!("h{i}")).collect();
        assert_eq!(aliases(&discovery), expected);
        assert_eq!(
            discovery.warnings,
            [too_deep(tree.path("home/.ssh/16.conf"), 2)]
        );
        assert_eq!(discovery.files.len(), 17);
    }

    #[test]
    fn files_cut_off_by_depth_are_read_again_closer_to_the_top() {
        let tree = Tree::new();
        tree.write("home/.ssh/config", &["Include 1.conf", "Include 16.conf"]);
        write_chain(&tree, 17);
        let discovery = tree.discover();
        let expected: Vec<String> = (1..=17).map(|i| format!("h{i}")).collect();
        assert_eq!(aliases(&discovery), expected);
        assert_eq!(
            discovery.warnings,
            [too_deep(tree.path("home/.ssh/16.conf"), 2)]
        );
    }

    #[test]
    fn include_cycles_do_not_stop_other_includes() {
        let tree = Tree::new();
        tree.write(
            "home/.ssh/config",
            &["Include a.conf", "Include other.conf"],
        );
        tree.write("home/.ssh/a.conf", &["Host a", "Include b.conf"]);
        let b = tree.write("home/.ssh/b.conf", &["Host b", "Include a.conf"]);
        tree.write("home/.ssh/other.conf", &["Host other"]);
        let discovery = tree.discover();
        assert_eq!(aliases(&discovery), ["a", "b", "other"]);
        // a.conf is at odd depths, b.conf at even ones.
        assert_eq!(discovery.warnings, [too_deep(b, 2)]);
        assert_eq!(discovery.files.len(), 4);
    }

    #[test]
    fn mutually_including_globs_finish() {
        let tree = Tree::new();
        tree.write("home/.ssh/config", &["Include conf.d/*"]);
        let names: Vec<String> = (0..20).map(|i| format!("h{i:02}")).collect();
        for name in &names {
            tree.write(
                &format!("home/.ssh/conf.d/{name}"),
                &[&format!("Host {name}"), "Include conf.d/*"],
            );
        }
        let discovery = tree.discover();
        assert_eq!(aliases(&discovery), names);
        assert_eq!(discovery.warnings.len(), names.len());
        assert_eq!(discovery.files.len(), names.len() + 1);
    }

    #[test]
    fn same_file_included_twice_is_listed_once() {
        let tree = Tree::new();
        let config = tree.write(
            "home/.ssh/config",
            &["Include common.conf", "Host own", "Include common.conf"],
        );
        let common = tree.write("home/.ssh/common.conf", &["Host common"]);
        let discovery = tree.discover();
        assert_eq!(aliases(&discovery), ["common", "own"]);
        assert_eq!(discovery.files, [config, common]);
    }

    #[test]
    fn config_file_replaces_the_default_files() {
        let tree = Tree::new();
        tree.write("home/.ssh/config", &["Host user"]);
        tree.write("etc/ssh/ssh_config", &["Host system"]);
        let custom = tree.write("custom/ssh_config", &["Host custom", "Include extra.conf"]);
        let extra = tree.write("home/.ssh/extra.conf", &["Host extra"]);
        tree.write("custom/extra.conf", &["Host wrong"]);
        let options = DiscoveryOptions {
            config_file: Some(custom.clone()),
            ..tree.options()
        };
        let discovery = discover(&options, &no_env);
        assert_eq!(aliases(&discovery), ["custom", "extra"]);
        assert_eq!(discovery.files, [custom, extra]);

        for none in ["none", "NONE"] {
            let options = DiscoveryOptions {
                config_file: Some(PathBuf::from(none)),
                ..tree.options()
            };
            assert_eq!(discover(&options, &no_env), Discovery::default());
        }
    }

    #[test]
    fn root_files_are_what_ssh_reads_first() {
        let tree = Tree::new();
        assert_eq!(
            tree.options().root_files(),
            [
                tree.path("home/.ssh/config"),
                tree.path("etc/ssh/ssh_config")
            ]
        );
        assert_eq!(
            DiscoveryOptions::new(PathBuf::from("/home/u"), None).root_files(),
            [
                PathBuf::from("/home/u/.ssh/config"),
                PathBuf::from("/etc/ssh/ssh_config")
            ]
        );

        let custom = tree.path("custom/ssh_config");
        let options = DiscoveryOptions {
            config_file: Some(custom.clone()),
            ..tree.options()
        };
        assert_eq!(options.root_files(), [custom]);

        for none in ["none", "NONE"] {
            let options = DiscoveryOptions {
                config_file: Some(PathBuf::from(none)),
                ..tree.options()
            };
            assert_eq!(options.root_files(), Vec::<PathBuf>::new());
        }
    }

    #[test]
    fn missing_files_are_not_reported() {
        let tree = Tree::new();
        assert_eq!(tree.discover(), Discovery::default());

        let options = DiscoveryOptions {
            config_file: Some(tree.path("missing")),
            ..tree.options()
        };
        assert_eq!(discover(&options, &no_env), Discovery::default());
    }

    #[test]
    fn unreadable_files_are_reported() {
        let tree = Tree::new();
        let system = tree.write(
            "etc/ssh/ssh_config",
            &["Include secret.conf", "Host system"],
        );
        let secret = tree.write("etc/ssh/secret.conf", &["Host secret"]);
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&secret).is_ok() {
            // Running as root, which can read anything.
            return;
        }
        let discovery = tree.discover();
        assert_eq!(aliases(&discovery), ["system"]);
        assert_eq!(discovery.files, [system]);
        assert!(
            matches!(
                discovery.warnings.as_slice(),
                [DiscoveryWarning::Unreadable { file, .. }] if *file == secret
            ),
            "{:?}",
            discovery.warnings
        );
    }

    #[test]
    fn a_directory_as_config_is_unreadable() {
        let tree = Tree::new();
        let config = tree.mkdir("home/.ssh/config");
        let discovery = tree.discover();
        assert!(discovery.files.is_empty());
        assert!(
            matches!(
                discovery.warnings.as_slice(),
                [DiscoveryWarning::Unreadable { file, .. }] if *file == config
            ),
            "{:?}",
            discovery.warnings
        );
    }

    #[test]
    fn visible_skips_hidden_aliases() {
        let tree = Tree::new();
        tree.write(
            "home/.ssh/config",
            &[
                "Host github.com",
                "Host prod-web",
                "Host GitLab.com",
                "Host staging",
            ],
        );
        let discovery = tree.discover();
        let hide = ["github.com".to_owned(), "gitlab.*".to_owned()];
        let visible: Vec<&str> = discovery
            .visible(&hide)
            .map(|host| host.alias.as_str())
            .collect();
        assert_eq!(visible, ["prod-web", "staging"]);
        assert_eq!(discovery.visible(&[]).count(), 4);
    }
}
