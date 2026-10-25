//! The globs of the Files field of the search of a tree, which ripgrep and fd take: split at
//! whitespace, `!` before one that leaves out what it matches. As in a `.gitignore`, a glob with
//! a `/` matches the path, others the name. Both programs read them with globset, as this module
//! does.

use globset::{Glob, GlobBuilder};

/// `glob`, without its `!`, built as ripgrep and fd read it.
pub fn build(glob: &str) -> Result<Glob, globset::Error> {
    let glob = glob.strip_prefix('!').unwrap_or(glob);
    if glob.contains('/') {
        GlobBuilder::new(glob.trim_start_matches('/'))
            .literal_separator(true)
            .build()
    } else {
        Glob::new(glob)
    }
}

/// Whether every glob in `text`, split at whitespace, is one.
pub fn valid(text: &str) -> bool {
    text.split_whitespace().all(|glob| build(glob).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_are_valid_until_one_is_not() {
        assert!(valid(""));
        assert!(valid("*.rs !target src/**/*.md /Cargo.toml {a,b}"));
        assert!(!valid("*.rs ["));
        assert!(!valid("!a{b"));
    }

    #[test]
    fn a_glob_with_a_slash_matches_the_path_and_others_the_name() {
        let path = build("/src/*.rs").unwrap().compile_matcher();
        assert!(path.is_match("src/main.rs"));
        assert!(!path.is_match("src/a/main.rs"), "no / in a *");
        let name = build("!*.rs").unwrap().compile_matcher();
        assert!(name.is_match("main.rs"));
    }
}
