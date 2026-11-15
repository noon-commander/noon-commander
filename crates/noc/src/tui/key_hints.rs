//! The key hints with the keys, after Ctrl+k while they show, as `K K` in nvim gives its hover
//! window the keys: every key sequence of what is under them, whole, with what it does, in a
//! list that `/` filters, under the names of their groups of keys, or of the contexts that bind
//! them. Enter closes it and runs the command under the cursor where the keys went before; Esc
//! closes it.

use noc_scroll::scrollbar::Bar;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::help;
use super::keymap::{Action, Extra, Hints, Keymap, Mode, Resolved, Typed};
use super::mouse::Pointer;
use super::picker::{Body, MenuLine, Pick, Picker};
use super::search::{Input, Kind};
use super::theme::Theme;
use super::which_key;
use super::window::{self, Colors};
use crate::i18n::fl;

/// Widest the column of keys gets; longer lists of keys are cut.
const MAX_KEYS_WIDTH: usize = 24;

/// What a key did in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyHintsEvent {
    Pending,
    Closed,
    /// Close the window, and run this where the keys went before it opened.
    Run(Typed),
}

/// A command of the list: its key sequences, what it does, and the heading it shows under.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    keys: String,
    text: String,
    action: Action,
    heading: String,
}

impl Row {
    /// Whether Enter runs it: not a command that takes the character typed after it, as `f`
    /// does, which the list has no key for, nor the tab of a digit, whose key says which.
    fn runs(&self) -> bool {
        !self.action.takes_char() && self.action != Action::GoToTab
    }
}

/// The window, with the commands of the hints it took the place of.
#[derive(Debug)]
pub(crate) struct KeyHintsWindow {
    title: String,
    rows: Vec<Row>,
    /// The count and the register typed before the hints, for the command run.
    extra: Extra,
    picker: Picker,
}

impl KeyHintsWindow {
    /// A window on the commands of `hints` in `keymap`, each sequence whole, after the keys
    /// the hints had typed; the command run from it gets `extra`. Its filter matches as `input`
    /// says, as it was last.
    pub(crate) fn new(keymap: &Keymap, hints: &Hints, extra: Extra, input: Input) -> Self {
        let place = hints.place;
        let rows: Vec<Row> = keymap
            .sequences(place, &hints.prefix)
            .into_iter()
            .filter_map(|listed| {
                let text = which_key::describe(place, listed.action)?;
                Some(Row {
                    keys: listed.keys,
                    text,
                    action: listed.action,
                    heading: help::heading_title(listed.heading),
                })
            })
            .collect();
        let mut picker = Picker::new(input);
        picker.set_rows(
            rows.iter()
                .map(|row| vec![row.keys.clone(), row.text.clone()])
                .collect(),
        );
        Self {
            title: hints
                .typed
                .clone()
                .unwrap_or_else(|| fl!("which-key-title")),
            rows,
            extra,
            picker,
        }
    }

    /// The rows, for the count before a page key.
    pub(crate) fn picker_mut(&mut self) -> &mut Picker {
        &mut self.picker
    }

    /// The mode keys are typed in: Insert while the filter takes them.
    pub(crate) fn mode(&self) -> Mode {
        self.picker.mode()
    }

    /// The kind of its filter while that takes the keys.
    pub(crate) fn search(&self) -> Option<Kind> {
        self.picker.search()
    }

    /// How its filter matches now, and whether case counts.
    pub(crate) fn input(&self) -> Input {
        self.picker.input()
    }

    /// Takes a key, as the picker does: Enter runs the command under the cursor, unless it
    /// takes a character.
    pub(crate) fn handle(&mut self, input: Resolved) -> KeyHintsEvent {
        match self.picker.handle(input) {
            Pick::Chosen(index) => match self.picker.shown(&self.rows).get(index) {
                Some(row) if row.runs() => KeyHintsEvent::Run(Typed {
                    resolved: Resolved::Action(row.action),
                    extra: self.extra,
                }),
                _ => KeyHintsEvent::Pending,
            },
            Pick::Closed => KeyHintsEvent::Closed,
            Pick::Pending | Pick::Filtered | Pick::Other(_) => KeyHintsEvent::Pending,
        }
    }

    /// The scroll bar of the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.picker.bar()
    }

    /// Shows the rows from `top`, where the thumb of the scroll bar was dragged.
    pub(crate) fn scroll_to(&mut self, top: usize) {
        self.picker.scroll_to(top);
    }

    /// Takes a press of the mouse, where the window was drawn last: a click puts the cursor on
    /// a row, and returns the key that a double click on one, or a click outside the window,
    /// stands for: Enter or Esc.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        self.picker.pointer(pointer)
    }

    /// The lines: the rows the filter shows, with its heading above the first of each run of
    /// rows under one, while they have more than one; without headings while the rows are
    /// `ranked`, best first, out of their groups.
    fn lines(rows: &[&Row], ranked: bool) -> Vec<MenuLine> {
        let headed = !ranked
            && rows
                .windows(2)
                .any(|pair| pair[0].heading != pair[1].heading);
        let mut lines = Vec::with_capacity(rows.len());
        let mut heading = None;
        for (index, row) in rows.iter().enumerate() {
            if headed && heading != Some(&row.heading) {
                heading = Some(&row.heading);
                lines.push(MenuLine::Heading(row.heading.clone()));
            }
            lines.push(MenuLine::Row(index));
        }
        lines
    }

    /// Draws the window centered over `area`: the filter, then the commands under their
    /// headings, scrolled to the cursor, those that take a character dimmed.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let shown = self.picker.shown(&self.rows);
        let lines = Self::lines(&shown, self.picker.ranked());
        let keys_width = shown
            .iter()
            .map(|row| cells::width(&row.keys))
            .max()
            .unwrap_or(0)
            .min(MAX_KEYS_WIDTH);
        let full = shown
            .get(self.picker.cursor())
            .map(|row| format!("{}  {}", row.keys, row.text));
        let body = Body {
            title: &self.title,
            width: window::WIDTH,
            lines: &lines,
            full,
            ..Body::default()
        };
        let focused = Colors::plain(theme).focused_style();
        self.picker
            .render(frame, area, theme, body, &mut |shown_row| {
                let row = shown[shown_row.index];
                let (keys_style, text_style, mark) = if shown_row.selected {
                    (focused, focused, theme.truncated_cursor)
                } else if row.runs() {
                    (theme.keys, theme.dialog, theme.truncated_dialog)
                } else {
                    let dim = theme.dialog.add_modifier(Modifier::DIM);
                    (
                        dim,
                        dim,
                        theme.truncated_dialog.remove_modifier(Modifier::DIM),
                    )
                };
                let marks = theme.glyphs.marks;
                let keys_width = keys_width.min(shown_row.width / 2);
                // A cell on either side, and two between the columns.
                let text_width = shown_row.width.saturating_sub(keys_width + 4);
                let mut spans = vec![Span::styled(" ", text_style)];
                let keys = which_key::fit_keys(&row.keys, keys_width);
                spans.extend(cells::spans(keys, (keys_style, mark), marks));
                spans.push(Span::styled("  ", text_style));
                let text = cells::fit(
                    &row.text,
                    text_width,
                    Align::Left,
                    (text_style, mark),
                    marks,
                );
                spans.extend(text);
                spans.push(Span::styled(" ", text_style));
                Line::from(spans).style(text_style)
            });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use noc_config::SearchMode;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::keymap::{Context, KeyState};

    /// The window over every key of `place`, as Ctrl+k twice opens it.
    fn window(place: (Mode, Context), extra: Extra) -> KeyHintsWindow {
        window_after(place, &[], SearchMode::Fzf, extra)
    }

    /// The window over the keys that can follow `typed` in `place`, its filter matching as
    /// `mode` says.
    fn window_after(
        place: (Mode, Context),
        typed: &[KeyEvent],
        mode: SearchMode,
        extra: Extra,
    ) -> KeyHintsWindow {
        let keymap = Keymap::default();
        let mut state = KeyState::default();
        for key in typed {
            keymap.feed(&mut state, place, *key, Instant::now());
        }
        keymap.show_hints(&mut state, place);
        let hints = keymap.hints_of(&state, place).unwrap();
        KeyHintsWindow::new(&keymap, &hints, extra, Input::new(Kind::FILTER, mode))
    }

    /// The headings the window shows over its rows.
    fn headings(window: &KeyHintsWindow) -> Vec<String> {
        let shown = window.picker.shown(&window.rows);
        KeyHintsWindow::lines(&shown, window.picker.ranked())
            .into_iter()
            .filter_map(|line| match line {
                MenuLine::Heading(heading) => Some(heading),
                MenuLine::Row(_) => None,
            })
            .collect()
    }

    fn press(window: &mut KeyHintsWindow, action: Action) -> KeyHintsEvent {
        window.handle(Resolved::Action(action))
    }

    fn filter(window: &mut KeyHintsWindow, text: &str) {
        press(window, Action::QuickSearch);
        for c in text.chars() {
            window.handle(Resolved::Insert(c));
        }
    }

    fn keys(window: &KeyHintsWindow) -> Vec<&str> {
        window
            .picker
            .shown(&window.rows)
            .iter()
            .map(|row| row.keys.as_str())
            .collect()
    }

    fn draw(window: &mut KeyHintsWindow) -> String {
        draw_in(window, 16)
    }

    fn draw_in(window: &mut KeyHintsWindow, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal
            .draw(|frame| window.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn sequences_show_whole_one_row_for_each_command() {
        let window = window((Mode::Normal, Context::Panel), Extra::default());
        let keys = keys(&window);
        assert!(keys.contains(&"Ctrl+w h"), "{keys:?}");
        assert!(keys.contains(&"Ctrl+w x, Ctrl+w r"), "{keys:?}");
        assert!(keys.contains(&"g g, Home"), "{keys:?}");
        assert!(!keys.contains(&"Ctrl+w"), "no groups: {keys:?}");
        assert!(keys.iter().all(|keys| !keys.starts_with("Esc")));
    }

    #[test]
    fn slash_filters_by_keys_and_text_and_enter_runs_with_the_count() {
        let extra = Extra {
            count: Some(3),
            ..Extra::default()
        };
        let mut window = window((Mode::Normal, Context::Panel), extra);
        filter(&mut window, "'swap");
        assert_eq!(keys(&window), ["Ctrl+w x, Ctrl+w r"]);
        assert_eq!(
            press(&mut window, Action::Confirm),
            KeyHintsEvent::Run(Typed {
                resolved: Resolved::Action(Action::SwapPanels),
                extra,
            })
        );
        press(&mut window, Action::NormalMode);
        assert_eq!(press(&mut window, Action::Cancel), KeyHintsEvent::Closed);
    }

    #[test]
    fn a_command_that_takes_a_character_does_not_run() {
        let mut window = window((Mode::Normal, Context::DialogInput), Extra::default());
        filter(&mut window, "'character typed after it");
        let row = window.picker.shown(&window.rows)[window.picker.cursor()].clone();
        assert!(row.action.takes_char(), "{row:?}");
        assert_eq!(press(&mut window, Action::Confirm), KeyHintsEvent::Pending);
    }

    #[test]
    fn keys_take_the_color_of_keys_but_under_the_cursor() {
        let mut window = window((Mode::Normal, Context::Panel), Extra::default());
        filter(&mut window, "'ctrl+w");
        let theme = Theme::mc_classic();
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
        terminal
            .draw(|frame| window.render(frame, frame.area(), &theme))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let colors: Vec<_> = (0..buffer.area.height)
            .filter_map(|y| {
                let row: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                let x = row.find("Ctrl+w")?;
                let x = u16::try_from(row[..x].chars().count()).unwrap();
                Some(buffer[(x, y)].fg)
            })
            .collect();
        let keys = theme.keys.fg.unwrap();
        // The last is the whole row under the cursor, under the rows, in the dialog's color.
        let (full, rows) = colors.split_last().unwrap();
        assert!(rows.len() > 1, "{}", terminal.backend());
        assert_ne!(rows[0], keys, "the row under the cursor");
        assert!(rows[1..].iter().all(|fg| *fg == keys), "{colors:?}");
        assert_eq!(Some(*full), theme.dialog.fg, "{}", terminal.backend());
    }

    #[test]
    fn rows_show_under_their_groups_then_their_contexts() {
        let panel = window((Mode::Normal, Context::Panel), Extra::default());
        assert_eq!(
            headings(&panel),
            [
                "Navigation",
                "Two panels",
                "Tabs",
                "Workspaces",
                "Options and sorting",
                "Marks and selection",
                "Long names",
                "Search",
                "Command line",
                "Go to",
                "Files",
                "Other",
                "Everywhere",
            ]
        );
        // A text field lists its own keys under its name, then those it falls back to.
        let insert = window((Mode::Insert, Context::DialogInput), Extra::default());
        assert_eq!(
            headings(&insert),
            ["Text fields of dialogs", "Motions", "Changes", "Everywhere"]
        );
        let normal = window((Mode::Normal, Context::DialogInput), Extra::default());
        assert_eq!(
            headings(&normal),
            ["Motions", "Modes", "Changes", "Dialogs", "Everywhere"]
        );
    }

    #[test]
    fn headings_go_while_the_rows_are_ranked_or_under_one() {
        let place = (Mode::Normal, Context::Panel);
        let mut ranked = window(place, Extra::default());
        filter(&mut ranked, "tab");
        assert!(keys(&ranked).len() > 1);
        assert_eq!(headings(&ranked), Vec::<String>::new(), "best first");
        // A plain filter keeps the order, and the headings over the rows it shows: Tab, which
        // is Ctrl+i without the kitty keyboard protocol, the tabs, and saving them.
        let mut plain = window_after(place, &[], SearchMode::Plain, Extra::default());
        filter(&mut plain, "tab");
        assert_eq!(headings(&plain), ["Navigation", "Tabs", "Workspaces"]);
        let ctrl_w = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
        let panels = window_after(place, &[ctrl_w], SearchMode::Fzf, Extra::default());
        assert_eq!(keys(&panels).len(), 6);
        assert_eq!(headings(&panels), Vec::<String>::new(), "one group");
    }

    #[test]
    fn draws_the_headings_over_their_rows() {
        let mut window = window((Mode::Normal, Context::Viewer), Extra::default());
        insta::assert_snapshot!(draw_in(&mut window, 36));
    }

    #[test]
    fn draws_the_keys_and_what_they_do() {
        let mut window = window((Mode::Normal, Context::Panel), Extra::default());
        filter(&mut window, "'ctrl+w");
        insta::assert_snapshot!(draw(&mut window));
    }
}
