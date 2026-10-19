//! The key hints with the keys, after Ctrl+k while they show, as `K K` in nvim gives its hover
//! window the keys: every key sequence of what is under them, whole, with what it does, in a
//! list that `/` filters. Enter closes it and runs the command under the cursor where the keys
//! went before; Esc closes it.

use noc_config::SearchMode;
use noc_scroll::scrollbar::Bar;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::Colors;
use super::keymap::{Action, Extra, Hints, Keymap, Mode, Resolved, Typed};
use super::mouse::Pointer;
use super::picker::{Body, MenuLine, Pick, Picker};
use super::theme::Theme;
use super::which_key;
use crate::i18n::fl;

/// Widest the window gets, in cells, borders included.
const WIDTH: u16 = 76;
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

/// A command of the list: its key sequences, and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    keys: String,
    text: String,
    action: Action,
}

impl Row {
    /// Whether Enter runs it: not a command that takes the character typed after it, as `f`
    /// does, which the list has no key for.
    fn runs(&self) -> bool {
        !self.action.takes_char()
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
    /// the hints had typed; the command run from it gets `extra`. Its filter matches in `mode`
    /// (`ui.search_mode`).
    pub(crate) fn new(keymap: &Keymap, hints: &Hints, extra: Extra, mode: SearchMode) -> Self {
        let place = (hints.mode, hints.context);
        let rows: Vec<Row> = keymap
            .sequences(place, &hints.prefix)
            .into_iter()
            .filter_map(|hint| {
                let action = hint.action?;
                let text = which_key::describe(place, action)?;
                Some(Row {
                    keys: hint.keys,
                    text,
                    action,
                })
            })
            .collect();
        let mut picker = Picker::new(mode);
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

    /// Draws the window centered over `area`: the filter, then the commands, scrolled to the
    /// cursor, those that take a character dimmed.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let shown = self.picker.shown(&self.rows);
        let lines: Vec<MenuLine> = (0..shown.len()).map(MenuLine::Row).collect();
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
            width: WIDTH,
            lines: &lines,
            full,
            ..Body::default()
        };
        let focused = Colors::of(theme, false).focused_style();
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::keymap::{Context, KeyState};

    /// The window over every key of `place`, as Ctrl+k twice opens it.
    fn window(place: (Mode, Context), extra: Extra) -> KeyHintsWindow {
        let keymap = Keymap::default();
        let mut state = KeyState::default();
        keymap.show_hints(&mut state, place);
        let hints = keymap.hints_of(&state, place).unwrap();
        KeyHintsWindow::new(&keymap, &hints, extra, SearchMode::Fzf)
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
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
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
        assert!(colors.len() > 1, "{}", terminal.backend());
        assert_ne!(colors[0], keys, "the row under the cursor");
        assert!(colors[1..].iter().all(|fg| *fg == keys), "{colors:?}");
    }

    #[test]
    fn draws_the_keys_and_what_they_do() {
        let mut window = window((Mode::Normal, Context::Panel), Extra::default());
        filter(&mut window, "'ctrl+w");
        insta::assert_snapshot!(draw(&mut window));
    }
}
