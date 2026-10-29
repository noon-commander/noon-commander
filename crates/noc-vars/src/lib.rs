//! Variables in text: `$NAME` and `${NAME}` stand for the value of a variable, `$$` for a `$`
//! (ADR 0054). A [`Template`] is read once, where its text comes from, such as a setting, and
//! expanded later where it is used: by Noon Commander, with [`Template::expand`], or by a shell
//! on another machine, which gets its [`pieces`](Template::pieces) quoted. Nothing else in the
//! text is special: a value is never split into words, matched as a pattern, or given a `~`.

use std::ffi::OsString;
use std::fmt;
use std::str::FromStr;

/// A part of a [`Template`]: text as it is, or the name of a variable.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Piece {
    Text(String),
    Var(String),
}

/// Why text is not a template.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VarsError {
    /// A `${` without a `}` after it; the text from the `${` on.
    #[error("`{0}` has no closing `}}`")]
    Unclosed(String),
    /// Braces around something that is not the name of a variable; the `${…}`.
    #[error("`{0}` names no variable")]
    Name(String),
}

/// Text with variables in it, read once.
///
/// A name is a letter or `_`, then letters, digits, and `_`, as a POSIX shell has it. `$NAME`
/// takes the longest name after the `$`, and `${NAME}` the name between the braces. A `$`
/// before anything else, such as a digit, a space, or the end, is a `$`; `$$` is one too.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Template {
    /// The text as it was written, to write it back the same.
    source: String,
    /// No two texts in a row, and no empty one.
    pieces: Vec<Piece>,
}

impl Template {
    /// Reads `text`.
    pub fn parse(text: &str) -> Result<Self, VarsError> {
        let mut pieces = Vec::new();
        let mut literal = String::new();
        let mut rest = text;
        while let Some(at) = rest.find('$') {
            literal.push_str(&rest[..at]);
            let after = &rest[at + 1..];
            if let Some(tail) = after.strip_prefix('$') {
                literal.push('$');
                rest = tail;
            } else if let Some(inner) = after.strip_prefix('{') {
                let Some(end) = inner.find('}') else {
                    return Err(VarsError::Unclosed(rest[at..].to_owned()));
                };
                let name = &inner[..end];
                if name_len(name) != name.len() || name.is_empty() {
                    // `$`, `{`, the name, and `}`.
                    return Err(VarsError::Name(rest[at..at + end + 3].to_owned()));
                }
                push_var(&mut pieces, &mut literal, name);
                rest = &inner[end + 1..];
            } else {
                let len = name_len(after);
                if len == 0 {
                    literal.push('$');
                } else {
                    push_var(&mut pieces, &mut literal, &after[..len]);
                }
                rest = &after[len..];
            }
        }
        literal.push_str(rest);
        if !literal.is_empty() {
            pieces.push(Piece::Text(literal));
        }
        Ok(Self {
            source: text.to_owned(),
            pieces,
        })
    }

    /// The template of `text` as it is, without variables: its `$`s are written as `$$`.
    pub fn literal(text: &str) -> Self {
        let pieces = if text.is_empty() {
            Vec::new()
        } else {
            vec![Piece::Text(text.to_owned())]
        };
        Self {
            source: text.replace('$', "$$"),
            pieces,
        }
    }

    /// The text as it was written.
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Its parts in order, for those that hand it to a shell to expand.
    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// The text with each variable replaced by what `lookup` gives for its name; a variable
    /// it gives nothing for, an unset one, leaves nothing.
    pub fn expand(&self, mut lookup: impl FnMut(&str) -> Option<OsString>) -> OsString {
        let mut text = OsString::new();
        for piece in &self.pieces {
            match piece {
                Piece::Text(part) => text.push(part),
                Piece::Var(name) => {
                    if let Some(value) = lookup(name) {
                        text.push(value);
                    }
                }
            }
        }
        text
    }

    /// [`expand`](Self::expand) with the variables of this process's environment.
    pub fn expand_env(&self) -> OsString {
        self.expand(|name| std::env::var_os(name))
    }
}

impl fmt::Display for Template {
    /// As it was written.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.source)
    }
}

impl FromStr for Template {
    type Err = VarsError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

/// The length in bytes of the name at the start of `text`; 0 if none starts there.
fn name_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    match bytes.first() {
        Some(first) if first.is_ascii_alphabetic() || *first == b'_' => bytes
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
            .count(),
        _ => 0,
    }
}

fn push_var(pieces: &mut Vec<Piece>, literal: &mut String, name: &str) {
    if !literal.is_empty() {
        pieces.push(Piece::Text(std::mem::take(literal)));
    }
    pieces.push(Piece::Var(name.to_owned()));
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    use super::*;

    fn text(part: &str) -> Piece {
        Piece::Text(part.to_owned())
    }

    fn var(name: &str) -> Piece {
        Piece::Var(name.to_owned())
    }

    fn pieces(source: &str) -> Vec<Piece> {
        Template::parse(source).unwrap().pieces().to_vec()
    }

    #[test]
    fn reads_variables_in_both_forms() {
        assert_eq!(pieces("$SHELL"), [var("SHELL")]);
        assert_eq!(pieces("${HOME}/bin"), [var("HOME"), text("/bin")]);
        assert_eq!(pieces("$HOME/bin"), [var("HOME"), text("/bin")]);
        assert_eq!(pieces("$HOME_bin"), [var("HOME_bin")]);
        assert_eq!(pieces("${HOME}_bin"), [var("HOME"), text("_bin")]);
        assert_eq!(
            pieces("a$_1-b${c}d"),
            [text("a"), var("_1"), text("-b"), var("c"), text("d")]
        );
        assert_eq!(pieces("$A$B"), [var("A"), var("B")]);
        assert_eq!(pieces("-i"), [text("-i")]);
        assert_eq!(pieces(""), []);
    }

    #[test]
    fn a_dollar_that_starts_no_name_is_a_dollar() {
        assert_eq!(pieces("$$"), [text("$")]);
        assert_eq!(pieces("a$$HOME"), [text("a$HOME")]);
        assert_eq!(pieces("$$$HOME"), [text("$"), var("HOME")]);
        for source in ["$", "a$", "$1", "$-", "$ x", "$é"] {
            assert_eq!(pieces(source), [text(source)], "{source}");
        }
    }

    #[test]
    fn braces_must_close_around_a_name() {
        assert_eq!(
            Template::parse("x${HOME/bin"),
            Err(VarsError::Unclosed("${HOME/bin".to_owned()))
        );
        assert_eq!(
            Template::parse("${"),
            Err(VarsError::Unclosed("${".to_owned()))
        );
        for (source, bad) in [("a${}b", "${}"), ("${1x}", "${1x}"), ("${A B}c", "${A B}")] {
            assert_eq!(
                Template::parse(source),
                Err(VarsError::Name(bad.to_owned())),
                "{source}"
            );
        }
        assert_eq!(
            VarsError::Unclosed("${HOME".to_owned()).to_string(),
            "`${HOME` has no closing `}`"
        );
    }

    #[test]
    fn writes_back_what_was_read() {
        for source in ["$SHELL", "${HOME}/a", "$1 $$ $", "-i"] {
            let template: Template = source.parse().unwrap();
            assert_eq!(template.to_string(), source);
            assert_eq!(template.as_str(), source);
        }
        let literal = Template::literal("a$HOME$");
        assert_eq!(literal.as_str(), "a$$HOME$$");
        assert_eq!(literal.pieces(), [text("a$HOME$")]);
        assert_eq!(
            Template::parse(literal.as_str()).unwrap().pieces(),
            literal.pieces()
        );
        assert_eq!(Template::literal("").pieces(), []);
    }

    #[test]
    fn expands_values_as_they_are() {
        let lookup = |name: &str| match name {
            "SHELL" => Some(OsString::from("/bin/zsh")),
            "SPACED" => Some(OsString::from("a b *")),
            "RAW" => Some(OsStr::from_bytes(b"caf\xe9").to_owned()),
            _ => None,
        };
        let expand = |source: &str| Template::parse(source).unwrap().expand(lookup);
        assert_eq!(expand("$SHELL"), "/bin/zsh");
        assert_eq!(expand("[$SPACED]"), "[a b *]");
        assert_eq!(expand("x${UNSET}y"), "xy");
        assert_eq!(expand("$$SHELL"), "$SHELL");
        assert_eq!(expand("~/$RAW").as_bytes(), b"~/caf\xe9");
    }
}
