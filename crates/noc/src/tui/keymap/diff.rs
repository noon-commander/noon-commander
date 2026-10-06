//! Two keymap presets compared action by action, for `noc keymap diff`.

use super::{Action, Bound, Context, Preset, describe, parse_sequence, preset};

const RED: &str = "31";
const GREEN: &str = "32";
const YELLOW: &str = "33";

/// How the two presets bind an action in a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Same,
    Changed,
    LeftOnly,
    RightOnly,
}

/// An action, with its keys in each preset that binds it there, as the help writes them.
#[derive(Debug)]
struct Row {
    action: Action,
    left: Option<Vec<String>>,
    right: Option<Vec<String>>,
}

impl Row {
    fn mark(&self) -> Mark {
        match (&self.left, &self.right) {
            (Some(left), Some(right)) if left == right => Mark::Same,
            (Some(_), Some(_)) => Mark::Changed,
            (Some(_), None) => Mark::LeftOnly,
            (None, _) => Mark::RightOnly,
        }
    }
}

/// A context, whether each preset has it, and its actions.
#[derive(Debug)]
struct Section {
    context: Context,
    on_left: bool,
    on_right: bool,
    rows: Vec<Row>,
}

/// Two presets compared: the contexts in the order of the left one, then those only the right
/// one has, and in each the actions in the same order.
#[derive(Debug)]
pub(crate) struct Diff {
    left: String,
    right: String,
    sections: Vec<Section>,
}

impl Diff {
    /// The built-in presets `left` and `right` compared; `None` if either is not one.
    pub(crate) fn by_names(left: &str, right: &str) -> Option<Self> {
        Some(Self::new(left, preset(left)?, right, preset(right)?))
    }

    fn new(left_name: &str, left: Preset, right_name: &str, right: Preset) -> Self {
        let mut contexts: Vec<Context> = left.iter().map(|(context, _)| *context).collect();
        for (context, _) in right {
            if !contexts.contains(context) {
                contexts.push(*context);
            }
        }
        let find = |preset: Preset, context: Context| {
            preset
                .iter()
                .find(|(other, _)| *other == context)
                .map(|(_, bound)| *bound)
        };
        let sections = contexts
            .into_iter()
            .map(|context| {
                let (left, right) = (find(left, context), find(right, context));
                let mut actions: Vec<Action> = Vec::new();
                for (action, _) in left.into_iter().chain(right).flatten() {
                    if !actions.contains(action) {
                        actions.push(*action);
                    }
                }
                let rows = actions
                    .into_iter()
                    .map(|action| Row {
                        action,
                        left: keys(left, action),
                        right: keys(right, action),
                    })
                    .collect();
                Section {
                    context,
                    on_left: left.is_some(),
                    on_right: right.is_some(),
                    rows,
                }
            })
            .collect();
        Self {
            left: left_name.to_owned(),
            right: right_name.to_owned(),
            sections,
        }
    }

    /// The comparison as a table per context: an action, its keys in the left preset, a mark,
    /// and its keys in the right one. The mark is `|` where the keys differ, `<` where only the
    /// left preset binds the action, and `>` where only the right one does. Without `all`, only
    /// those rows show, and a context without them is `same`. With `color`, as git diff: red
    /// for what only the left has, green for what only the right has, and a yellow `|`.
    pub(crate) fn render(&self, all: bool, color: bool) -> String {
        let paint = |text: &str, sgr: &str| {
            if color && !text.is_empty() {
                format!("\x1b[{sgr}m{text}\x1b[0m")
            } else {
                text.to_owned()
            }
        };
        let shown = |row: &&Row| all || row.mark() != Mark::Same;
        let rows = || {
            self.sections
                .iter()
                .flat_map(|section| &section.rows)
                .filter(shown)
        };
        let action_width = rows().map(|row| width(row.action.name())).max();
        let action_width = action_width.unwrap_or(0);
        let left_width = rows()
            .map(|row| width(&join(row.left.as_deref())))
            .chain([width(&self.left)])
            .max()
            .unwrap_or(0);
        let pad = |plain: &str, width: usize| " ".repeat(width.saturating_sub(self::width(plain)));

        let mut lines = vec![paint(
            &format!(
                "  {}  {}{}   {}",
                pad("", action_width),
                self.left,
                pad(&self.left, left_width),
                self.right
            ),
            "1",
        )];
        for section in &self.sections {
            let mut header = paint(&format!("[{}]", section.context.name()), "1;36");
            if !section.on_left {
                header = format!(
                    "{header}  {}",
                    paint(&format!("only in {}", self.right), GREEN)
                );
            } else if !section.on_right {
                header = format!(
                    "{header}  {}",
                    paint(&format!("only in {}", self.left), RED)
                );
            }
            let rows: Vec<&Row> = section.rows.iter().filter(shown).collect();
            if rows.is_empty() {
                lines.push(format!("{header} {}", paint("same", "2")));
                continue;
            }
            lines.push(header);
            for row in rows {
                let name = row.action.name();
                let (left, right) = (row.left.as_deref(), row.right.as_deref());
                let (painted, left_keys, mark, right_keys) = match row.mark() {
                    Mark::Same => (name.to_owned(), join(left), " ".to_owned(), join(right)),
                    Mark::Changed => (
                        name.to_owned(),
                        contrast(left, right, RED, &paint),
                        paint("|", YELLOW),
                        contrast(right, left, GREEN, &paint),
                    ),
                    Mark::LeftOnly => (
                        paint(name, RED),
                        paint(&join(left), RED),
                        paint("<", RED),
                        String::new(),
                    ),
                    Mark::RightOnly => (
                        paint(name, GREEN),
                        String::new(),
                        paint(">", GREEN),
                        paint(&join(right), GREEN),
                    ),
                };
                let line = format!(
                    "  {painted}{}  {left_keys}{} {mark}  {right_keys}",
                    pad(name, action_width),
                    pad(&join(left), left_width),
                );
                lines.push(line.trim_end_matches(' ').to_owned());
            }
        }
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }
}

/// The keys that `bound` gives `action`, as the help writes them.
fn keys(bound: Option<Bound>, action: Action) -> Option<Vec<String>> {
    let (_, keys) = bound?.iter().find(|(other, _)| *other == action)?;
    Some(
        keys.iter()
            .map(|keys| {
                let sequence = parse_sequence(keys)
                    .unwrap_or_else(|| unreachable!("invalid preset key `{keys}`"));
                describe(&sequence)
            })
            .collect(),
    )
}

fn join(keys: Option<&[String]>) -> String {
    keys.unwrap_or_default().join(", ")
}

/// The keys of `keys` apart by commas, those that `other` lacks painted in `sgr`.
fn contrast(
    keys: Option<&[String]>,
    other: Option<&[String]>,
    sgr: &str,
    paint: &dyn Fn(&str, &str) -> String,
) -> String {
    let other = other.unwrap_or_default();
    keys.unwrap_or_default()
        .iter()
        .map(|key| {
            if other.contains(key) {
                key.clone()
            } else {
                paint(key, sgr)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn width(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: Preset = &[
        (
            Context::Panel,
            &[
                (Action::Up, &["up", "ctrl-p"]),
                (Action::Quit, &["f10"]),
                (Action::Help, &["f1"]),
            ],
        ),
        (Context::Root, &[(Action::EditHost, &["f4"])]),
        (Context::Dialog, &[(Action::Cancel, &["esc"])]),
    ];

    const RIGHT: Preset = &[
        (
            Context::Panel,
            &[
                (Action::Up, &["k", "up"]),
                (Action::Help, &["f1"]),
                (Action::Redraw, &["ctrl-l"]),
            ],
        ),
        (Context::Root, &[(Action::EditHost, &["f4"])]),
        (Context::Viewer, &[(Action::Quit, &["q"])]),
    ];

    fn diff() -> Diff {
        Diff::new("left", LEFT, "right", RIGHT)
    }

    #[test]
    fn shows_what_differs_action_by_action() {
        assert_eq!(
            diff().render(false, false),
            "          left         right
[panel]
  up      Up, Ctrl+p |  k, Up
  quit    F10        <
  redraw             >  Ctrl+l
[root] same
[dialog]  only in left
  cancel  Esc        <
[viewer]  only in right
  quit               >  q
"
        );
    }

    #[test]
    fn all_shows_what_is_alike_too() {
        assert_eq!(
            diff().render(true, false),
            "             left         right
[panel]
  up         Up, Ctrl+p |  k, Up
  quit       F10        <
  help       F1            F1
  redraw                >  Ctrl+l
[root]
  edit_host  F4            F4
[dialog]  only in left
  cancel     Esc        <
[viewer]  only in right
  quit                  >  q
"
        );
    }

    #[test]
    fn colors_what_only_one_side_has_and_keep_the_columns() {
        let colored = diff().render(false, true);
        let up = colored.lines().nth(2).unwrap();
        assert_eq!(
            up,
            "  up      Up, \x1b[31mCtrl+p\x1b[0m \x1b[33m|\x1b[0m  \x1b[32mk\x1b[0m, Up"
        );
        let mut plain = String::new();
        let mut rest = colored.as_str();
        while let Some(start) = rest.find('\x1b') {
            plain.push_str(&rest[..start]);
            let end = rest[start..].find('m').unwrap();
            rest = &rest[start + end + 1..];
        }
        plain.push_str(rest);
        assert_eq!(plain, diff().render(false, false));
    }

    #[test]
    fn knows_only_the_built_in_presets() {
        assert!(Diff::by_names("default", "vim").is_some());
        assert!(Diff::by_names("default", "emacs").is_none());
        let same = Diff::by_names("vim", "vim").unwrap().render(false, false);
        assert!(
            same.lines().skip(1).all(|line| line.ends_with(" same")),
            "{same}"
        );
    }
}
