//! Shell patterns for marking by name, read as mc reads them for `+` and `-`: `*` matches any
//! text, `?` one character, `[a-z]` one character of a set (`[!…]` or `[^…]` one outside it),
//! `{a,b}` either alternative, and `\` takes the next character as it is. The whole name must
//! match, case counts, and anything that does not parse is taken literally.

/// Most alternatives that braces expand to; more are dropped, which no one types.
const MAX_ALTERNATIVES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// `*`: any text, also none.
    Star,
    /// `?`: one character.
    One,
    Char(char),
    /// `[…]`: one character in one of the ranges, or outside all of them.
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

impl Token {
    /// Whether this token, which is not `*`, matches `c`.
    fn matches(&self, c: char) -> bool {
        match self {
            Self::Star | Self::One => true,
            Self::Char(wanted) => *wanted == c,
            Self::Class { negated, ranges } => {
                ranges.iter().any(|(low, high)| (*low..=*high).contains(&c)) != *negated
            }
        }
    }
}

/// A parsed pattern: braces expanded into alternatives without them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pattern(Vec<Vec<Token>>);

impl Pattern {
    pub(crate) fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let mut pos = 0;
        let mut alternatives = sequence(&chars, &mut pos, 0);
        // At depth 0 nothing ends a sequence early.
        debug_assert_eq!(pos, chars.len());
        alternatives.dedup();
        Self(alternatives)
    }

    /// Whether all of `name` matches.
    pub(crate) fn matches(&self, name: &str) -> bool {
        let name: Vec<char> = name.chars().collect();
        self.0.iter().any(|tokens| matches(tokens, &name))
    }
}

/// The alternatives of the text from `pos` on, up to a `,` or `}` of the group it is in when
/// `depth` is above 0.
fn sequence(chars: &[char], pos: &mut usize, depth: usize) -> Vec<Vec<Token>> {
    let mut alternatives = vec![Vec::new()];
    let push = |alternatives: &mut Vec<Vec<Token>>, token: Token| {
        for tokens in alternatives.iter_mut() {
            tokens.push(token.clone());
        }
    };
    while let Some(&c) = chars.get(*pos) {
        match c {
            ',' | '}' if depth > 0 => break,
            '{' => {
                if let Some(group) = group(chars, pos, depth) {
                    alternatives = alternatives
                        .iter()
                        .flat_map(|head| {
                            group.iter().map(move |tail| {
                                let mut tokens = head.clone();
                                tokens.extend(tail.iter().cloned());
                                tokens
                            })
                        })
                        .take(MAX_ALTERNATIVES)
                        .collect();
                } else {
                    push(&mut alternatives, Token::Char('{'));
                    *pos += 1;
                }
            }
            '[' => {
                let token = class(chars, pos).unwrap_or_else(|| {
                    *pos += 1;
                    Token::Char('[')
                });
                push(&mut alternatives, token);
            }
            '*' => {
                *pos += 1;
                // `**` matches what `*` does.
                if alternatives
                    .iter()
                    .any(|tokens| tokens.last() != Some(&Token::Star))
                {
                    push(&mut alternatives, Token::Star);
                }
            }
            '?' => {
                *pos += 1;
                push(&mut alternatives, Token::One);
            }
            '\\' => {
                // A trailing backslash stands for itself.
                let c = chars.get(*pos + 1).copied().unwrap_or('\\');
                *pos = (*pos + 2).min(chars.len());
                push(&mut alternatives, Token::Char(c));
            }
            c => {
                *pos += 1;
                push(&mut alternatives, Token::Char(c));
            }
        }
    }
    alternatives
}

/// The alternatives of the group that starts at `pos`, at `{`; `None`, with `pos` unchanged,
/// if it is not closed.
fn group(chars: &[char], pos: &mut usize, depth: usize) -> Option<Vec<Vec<Token>>> {
    let start = *pos;
    *pos += 1;
    let mut alternatives = Vec::new();
    loop {
        alternatives.extend(sequence(chars, pos, depth + 1));
        match chars.get(*pos) {
            Some(',') => *pos += 1,
            Some('}') => {
                *pos += 1;
                alternatives.truncate(MAX_ALTERNATIVES);
                return Some(alternatives);
            }
            _ => {
                *pos = start;
                return None;
            }
        }
    }
}

/// The class that starts at `pos`, at `[`; `None`, with `pos` unchanged, if it is not closed.
/// A `]` right after the opening (and its `!` or `^`) belongs to the set.
fn class(chars: &[char], pos: &mut usize) -> Option<Token> {
    let mut at = *pos + 1;
    let negated = matches!(chars.get(at), Some('!' | '^'));
    if negated {
        at += 1;
    }
    let first = at;
    let mut ranges = Vec::new();
    loop {
        let mut c = *chars.get(at)?;
        if c == ']' && at > first {
            *pos = at + 1;
            return Some(Token::Class { negated, ranges });
        }
        if c == '\\' {
            at += 1;
            c = *chars.get(at)?;
        }
        at += 1;
        match (chars.get(at), chars.get(at + 1)) {
            (Some('-'), Some(&high)) if high != ']' => {
                ranges.push((c, high));
                at += 2;
            }
            _ => ranges.push((c, c)),
        }
    }
}

/// Whether `tokens`, without braces, match all of `text`: the usual walk that goes back only
/// to the last `*`, so that no pattern takes more than length times length steps.
fn matches(tokens: &[Token], text: &[char]) -> bool {
    let (mut token, mut at) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while at < text.len() {
        match tokens.get(token) {
            Some(Token::Star) => {
                star = Some((token, at));
                token += 1;
            }
            Some(wanted) if wanted.matches(text[at]) => {
                token += 1;
                at += 1;
            }
            _ => match star {
                // Let the last `*` take one more character and try again from there.
                Some((star_token, star_at)) => {
                    token = star_token + 1;
                    at = star_at + 1;
                    star = Some((star_token, star_at + 1));
                }
                None => return false,
            },
        }
    }
    tokens[token..].iter().all(|token| *token == Token::Star)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, name: &str) -> bool {
        Pattern::new(pattern).matches(name)
    }

    #[test]
    fn stars_and_question_marks() {
        assert!(matches("*", "anything"));
        assert!(matches("*", ".hidden"), "dot files too, as in mc");
        assert!(matches("*", ""));
        assert!(matches("*.txt", "notes.txt"));
        assert!(!matches("*.txt", "notes.txt.bak"), "the whole name");
        assert!(matches("a*b*c", "abc"));
        assert!(matches("a*b*c", "a-b-b-c"));
        assert!(!matches("a*b*c", "a-b-b-d"));
        assert!(matches("???", "a€c"), "characters, not bytes");
        assert!(!matches("??", "abc"));
        assert!(matches("**x", "ax"));
        assert!(!matches("*.TXT", "notes.txt"), "case counts, as in mc");
    }

    #[test]
    fn classes() {
        assert!(matches("[abc].md", "b.md"));
        assert!(!matches("[abc].md", "d.md"));
        assert!(matches("[a-c0-9]", "7"));
        assert!(matches("[!a-c]x", "dx"));
        assert!(!matches("[^a-c]x", "bx"));
        assert!(matches("[]]", "]"), "a leading ] belongs to the set");
        assert!(matches("[a-]", "-"), "so does a trailing -");
        assert!(matches("[\\]]", "]"));
        assert!(matches("[ab", "[ab"), "not closed: literal");
    }

    #[test]
    fn braces() {
        for name in ["main.c", "main.h", "lib.rs"] {
            assert!(matches("{*.c,*.h,lib.rs}", name), "{name}");
        }
        assert!(!matches("{*.c,*.h}", "main.o"));
        assert!(matches("a{b,c{d,e}}f", "acef"));
        assert!(matches("{a,b}{1,2}", "b1"));
        assert!(matches("{}", ""));
        assert!(matches("a,b", "a,b"), "a comma outside braces is literal");
        assert!(matches("{a,b", "{a,b"), "not closed: literal");
        assert!(matches("x}", "x}"));
    }

    #[test]
    fn backslashes() {
        assert!(matches("\\*", "*"));
        assert!(!matches("\\*", "x"));
        assert!(matches("a\\{b,c}", "a{b,c}"));
        assert!(matches("end\\", "end\\"));
    }

    #[test]
    fn stars_never_take_long() {
        let name = "a".repeat(255);
        let pattern = format!("{}b", "*a".repeat(30));
        assert!(!matches(&pattern, &name));
        let braces = "{a,b}".repeat(20);
        assert_eq!(Pattern::new(&braces).0.len(), MAX_ALTERNATIVES);
    }
}
