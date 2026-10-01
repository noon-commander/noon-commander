//! `ssh_config`-style host patterns.

/// Matches `text` against a pattern with `*` and `?` wildcards, ignoring ASCII case.
///
/// `*` matches any sequence of characters, including none; `?` matches exactly one character.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    // After a mismatch, retry from the last `*`, letting it swallow one more character. This
    // keeps the worst case at O(pattern × text) instead of exponential.
    let mut last_star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                last_star = Some((p, t));
                p += 1;
            }
            Some(&c) if c == '?' || c.eq_ignore_ascii_case(&text[t]) => {
                p += 1;
                t += 1;
            }
            _ => match last_star {
                Some((star, start)) => {
                    last_star = Some((star, start + 1));
                    p = star + 1;
                    t = start + 1;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// Whether `pattern` names exactly one host: no wildcards and no negation.
pub fn is_concrete(pattern: &str) -> bool {
    !pattern.is_empty() && !pattern.starts_with('!') && !pattern.contains(['*', '?'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_literally() {
        assert!(wildcard_match("prod-web", "prod-web"));
        assert!(!wildcard_match("prod-web", "prod-we"));
        assert!(!wildcard_match("prod-we", "prod-web"));
        assert!(!wildcard_match("prod-web", "staging"));
    }

    #[test]
    fn ignores_ascii_case() {
        assert!(wildcard_match("GitHub.COM", "github.com"));
        assert!(wildcard_match("*.EXAMPLE.org", "www.example.ORG"));
        assert!(wildcard_match("h?st", "HOST"));
    }

    #[test]
    fn star_matches_any_sequence() {
        assert!(wildcard_match("*.example.com", "www.example.com"));
        assert!(wildcard_match("*.example.com", ".example.com"));
        assert!(!wildcard_match("*.example.com", "example.com"));
        assert!(wildcard_match("prod-*-db", "prod-eu-1-db"));
        assert!(wildcard_match("prod-*-db", "prod--db"));
        assert!(!wildcard_match("prod-*-db", "prod-db"));
        assert!(wildcard_match("prod*", "prod"));
        assert!(wildcard_match("prod*", "production"));
        assert!(!wildcard_match("prod*", "pro"));
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("*", ""));
    }

    #[test]
    fn question_mark_matches_one_character() {
        assert!(wildcard_match("web?", "web1"));
        assert!(!wildcard_match("web?", "web"));
        assert!(!wildcard_match("web?", "web12"));
        assert!(wildcard_match("w?b??", "web12"));
        assert!(wildcard_match("?", "é"));
        assert!(!wildcard_match("?", ""));
    }

    #[test]
    fn multiple_stars() {
        assert!(wildcard_match("*a*b*", "xxaxxbxx"));
        assert!(wildcard_match("*a*b*", "ab"));
        assert!(!wildcard_match("*a*b*", "ba"));
        assert!(wildcard_match("a**b", "ab"));
        assert!(wildcard_match("*?*", "x"));
        assert!(!wildcard_match("*?*", ""));
        assert!(wildcard_match("a*b*c", "abcbc"));
        assert!(!wildcard_match("a*b*c", "abcb"));
    }

    #[test]
    fn empty_pattern_and_text() {
        assert!(wildcard_match("", ""));
        assert!(!wildcard_match("", "a"));
        assert!(!wildcard_match("a", ""));
        assert!(wildcard_match("**", ""));
    }

    #[test]
    fn pathological_pattern_is_fast() {
        let text = "a".repeat(10_000);
        assert!(!wildcard_match("a*a*a*a*b", &text));
        assert!(wildcard_match("a*a*a*a*a", &text));
        let pattern = "*a".repeat(50);
        assert!(!wildcard_match(&format!("{pattern}b"), &text));
    }

    #[test]
    fn concrete_patterns() {
        assert!(is_concrete("prod-web"));
        assert!(is_concrete("10.0.0.5"));
        assert!(!is_concrete(""));
        assert!(!is_concrete("*"));
        assert!(!is_concrete("*.example.com"));
        assert!(!is_concrete("web?"));
        assert!(!is_concrete("!bastion"));
    }
}
