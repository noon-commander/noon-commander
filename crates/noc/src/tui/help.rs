//! The help screen: the keys of what the app can do, read from the keymap.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Colors, draw_box};
use super::keymap::{Action, Context, Keymap, Resolved};
use super::theme::Theme;
use crate::i18n::fl;

/// Widest the help gets, in cells, borders included.
const MAX_WIDTH: u16 = 80;
/// Widest the key column gets; longer lists of keys go on to the next lines.
const MAX_KEYS_WIDTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Heading(String),
    Keys { keys: String, text: String },
    Note(String),
    Blank,
}

/// The help screen, scrolled to `offset`.
#[derive(Debug)]
pub(crate) struct Help {
    entries: Vec<Entry>,
    /// First screen row shown.
    offset: usize,
    /// Rows the help had at the last render, and how many fit on screen.
    rows: usize,
    page: usize,
}

impl Help {
    /// The keys `keymap` binds to what the app can do, by context. `type_to_search` adds a
    /// note on typing in panels.
    pub(crate) fn new(keymap: &Keymap, type_to_search: bool) -> Self {
        let sections = [
            (Context::Panel, fl!("help-panels")),
            (Context::Root, fl!("help-root")),
            (Context::QuickSearch, fl!("help-quick-search")),
            (Context::Menu, fl!("help-menu")),
            (Context::Jump, fl!("help-jump")),
            (Context::PullDown, fl!("help-pulldown")),
            (Context::Dialog, fl!("help-dialogs")),
            (Context::DialogInput, fl!("help-text-fields")),
            (Context::Viewer, fl!("help-viewer")),
        ];
        let mut entries = Vec::new();
        for (context, title) in sections {
            let rows: Vec<Entry> = keymap
                .help(context)
                .into_iter()
                .filter_map(|(action, keys)| {
                    let text = describe(context, action)?;
                    Some(Entry::Keys { keys, text })
                })
                .collect();
            if !rows.is_empty() {
                if !entries.is_empty() {
                    entries.push(Entry::Blank);
                }
                entries.push(Entry::Heading(title));
                entries.extend(rows);
            }
        }
        entries.push(Entry::Blank);
        entries.push(Entry::Note(fl!("help-note-menu")));
        entries.push(Entry::Note(fl!("help-note-jump")));
        entries.push(Entry::Note(fl!("help-note-pulldown")));
        entries.push(Entry::Note(fl!("help-note-esc")));
        if type_to_search {
            entries.push(Entry::Note(fl!("help-note-typing")));
        }
        Self {
            entries,
            offset: 0,
            rows: 0,
            page: 1,
        }
    }

    /// Scrolls; `true` when the help should close.
    pub(crate) fn handle(&mut self, input: Resolved) -> bool {
        let Resolved::Action(action) = input else {
            return false;
        };
        let last = self.rows.saturating_sub(self.page);
        self.offset = match action {
            Action::Up => self.offset.saturating_sub(1),
            Action::Down => self.offset + 1,
            Action::PageUp => self.offset.saturating_sub(self.page),
            Action::PageDown => self.offset + self.page,
            Action::Home => 0,
            Action::End => last,
            Action::Cancel | Action::Confirm => return true,
            _ => self.offset,
        }
        .min(last);
        false
    }

    /// Draws the help over most of `area`.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let width = MAX_WIDTH.min(area.width.saturating_sub(4)).max(20);
        let height = area.height.saturating_sub(4).max(3);
        let colors = Colors::of(theme, false);
        let size = (width, height);
        let inner = draw_box(frame, area, size, &fl!("help-title"), colors, theme);
        let width = usize::from(inner.width);
        let keys_width = self
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Keys { keys, .. } => Some(cells::width(keys)),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            .min(MAX_KEYS_WIDTH)
            .min(width / 2);
        let text_width = width.saturating_sub(keys_width + 1);
        let lines: Vec<Line<'static>> = self
            .entries
            .iter()
            .flat_map(|entry| match entry {
                Entry::Heading(title) => {
                    vec![Line::styled(
                        cells::fit(title, width, Align::Left),
                        theme.dialog_title,
                    )]
                }
                Entry::Keys { keys, text } => {
                    let text = cells::fit(text, text_width, Align::Left);
                    key_lines(keys, keys_width)
                        .into_iter()
                        .zip(std::iter::once(text).chain(std::iter::repeat(String::new())))
                        .map(|(keys, text)| {
                            let keys = cells::fit(&keys, keys_width, Align::Left);
                            Line::from(vec![
                                Span::styled(keys, theme.dialog_title),
                                Span::raw(" "),
                                Span::raw(text),
                            ])
                        })
                        .collect()
                }
                Entry::Note(text) => cells::wrap(text, width)
                    .into_iter()
                    .map(Line::raw)
                    .collect(),
                Entry::Blank => vec![Line::default()],
            })
            .collect();
        self.rows = lines.len();
        self.page = usize::from(inner.height).max(1);
        self.offset = self.offset.min(self.rows.saturating_sub(self.page));
        for (index, line) in lines
            .into_iter()
            .skip(self.offset)
            .take(self.page)
            .enumerate()
        {
            let y = inner.y + u16::try_from(index).unwrap_or(u16::MAX);
            frame.render_widget(line, Rect::new(inner.x, y, inner.width, 1));
        }
    }
}

/// `keys`, a list such as `Insert, Ctrl-t`, in lines of at most `width` cells, broken after
/// the commas. A single key wider than that gets a line of its own.
fn key_lines(keys: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut alternatives = keys.split(", ").peekable();
    while let Some(key) = alternatives.next() {
        let key = if alternatives.peek().is_some() {
            format!("{key},")
        } else {
            key.to_owned()
        };
        match lines.last_mut() {
            Some(line) if cells::width(line) + 1 + cells::width(&key) <= width => {
                line.push(' ');
                line.push_str(&key);
            }
            _ => lines.push(key),
        }
    }
    lines
}

/// What `action` does in `context`; `None` for what the app cannot do yet, which the help
/// leaves out.
fn describe(context: Context, action: Action) -> Option<String> {
    // Lists of rows, where the arrows move the cursor.
    let rows = matches!(
        context,
        Context::Panel | Context::Viewer | Context::Menu | Context::Jump
    );
    let text = match (context, action) {
        (_, Action::Up) if rows => fl!("help-row-up"),
        (_, Action::Down) if rows => fl!("help-row-down"),
        (_, Action::PageUp) if rows => fl!("help-page-up"),
        (_, Action::PageDown) if rows => fl!("help-page-down"),
        (Context::Panel | Context::Menu | Context::Jump, Action::Home) => fl!("help-first-row"),
        (Context::Panel | Context::Menu | Context::Jump, Action::End) => fl!("help-last-row"),
        (Context::Panel, Action::Enter) => fl!("help-enter"),
        (Context::Panel, Action::Mark) => fl!("help-mark"),
        (Context::Panel, Action::MarkUp) => fl!("help-mark-up"),
        (Context::Panel, Action::InvertMarks) => fl!("help-invert-marks"),
        (Context::Panel, Action::Select) => fl!("help-select"),
        (Context::Panel, Action::Unselect) => fl!("help-unselect"),
        (Context::Panel, Action::Parent) => fl!("help-parent"),
        (Context::Panel, Action::SwitchPanel) => fl!("help-switch-panel"),
        (Context::Panel, Action::SwapPanels) => fl!("help-swap-panels"),
        (Context::Panel, Action::OtherPanelOpen) => fl!("help-other-open"),
        (Context::Panel, Action::OtherPanelSync) => fl!("help-other-sync"),
        (Context::Panel, Action::Reload) => fl!("help-reload"),
        (Context::Panel, Action::Cancel) => fl!("help-stop"),
        (Context::Panel, Action::ToggleHidden) => fl!("help-toggle-hidden"),
        (Context::Panel, Action::SortByName) => fl!("help-sort-name"),
        (Context::Panel, Action::SortByExtension) => fl!("help-sort-extension"),
        (Context::Panel, Action::SortByTime) => fl!("help-sort-time"),
        (Context::Panel, Action::SortBySize) => fl!("help-sort-size"),
        (Context::Panel, Action::QuickSearch) => fl!("help-quick-search-start"),
        (Context::Panel, Action::View) => fl!("help-view"),
        (Context::Panel, Action::Edit) => fl!("help-edit"),
        (Context::Panel, Action::Copy) => fl!("help-copy"),
        (Context::Panel, Action::Move) => fl!("help-move"),
        (Context::Panel, Action::Mkdir) => fl!("help-mkdir"),
        (Context::Panel, Action::Delete) => fl!("help-delete"),
        (Context::Panel, Action::Jobs) => fl!("help-jobs"),
        (Context::Panel, Action::Checksum) => fl!("help-checksum"),
        (Context::Panel, Action::LocationMenuLeft) => fl!("help-menu-left"),
        (Context::Panel, Action::LocationMenuRight) => fl!("help-menu-right"),
        (Context::Panel, Action::Jump) => fl!("help-jump-open"),
        (Context::Panel, Action::QuickCd) => fl!("help-quick-cd"),
        (Context::Panel, Action::NewTab) => fl!("help-new-tab"),
        (Context::Panel, Action::CloseTab) => fl!("help-close-tab"),
        (Context::Panel, Action::NextTab) => fl!("help-next-tab"),
        (Context::Panel, Action::PrevTab) => fl!("help-prev-tab"),
        (Context::Panel, Action::TabList) => fl!("help-tab-list"),
        (Context::Panel | Context::Viewer, Action::Help) => fl!("help-help"),
        (Context::Panel, Action::Quit) => fl!("help-quit"),
        (Context::Panel | Context::Viewer, Action::Redraw) => fl!("help-redraw"),
        (Context::Root | Context::Menu, Action::Disconnect) => fl!("help-disconnect"),
        (Context::Root, Action::EditHost) => fl!("help-edit-host"),
        (Context::Menu, Action::Confirm) => fl!("help-menu-open"),
        (Context::Menu, Action::Backspace) => fl!("help-menu-back"),
        (Context::Menu, Action::Reload) => fl!("help-menu-reload"),
        (Context::Menu | Context::Jump, Action::Cancel) => fl!("help-menu-close"),
        (Context::Jump, Action::Confirm) => fl!("help-jump-go"),
        (Context::PullDown, Action::Cancel) => fl!("help-pulldown-close"),
        (Context::Panel, Action::PullDown) => fl!("help-pulldown-open"),
        (Context::PullDown, Action::Up) => fl!("help-pulldown-up"),
        (Context::PullDown, Action::Down) => fl!("help-pulldown-down"),
        (Context::PullDown, Action::Left) => fl!("help-pulldown-left"),
        (Context::PullDown, Action::Right) => fl!("help-pulldown-right"),
        (Context::PullDown, Action::Home) => fl!("help-pulldown-home"),
        (Context::PullDown, Action::End) => fl!("help-pulldown-end"),
        (Context::PullDown, Action::Confirm) => fl!("help-pulldown-run"),
        (Context::QuickSearch | Context::Jump, Action::Backspace) => fl!("help-search-back"),
        (Context::QuickSearch, Action::Cancel) => fl!("help-search-end"),
        (Context::Dialog, Action::Up) => fl!("help-dialog-up"),
        (Context::Dialog, Action::Down) => fl!("help-dialog-down"),
        (Context::Dialog, Action::Left) => fl!("help-dialog-left"),
        (Context::Dialog, Action::Right) => fl!("help-dialog-right"),
        (Context::Dialog, Action::PageUp) => fl!("help-dialog-page-up"),
        (Context::Dialog, Action::PageDown) => fl!("help-dialog-page-down"),
        (Context::Dialog, Action::Home) => fl!("help-dialog-home"),
        (Context::Dialog, Action::End) => fl!("help-dialog-end"),
        (Context::Dialog, Action::NextField) => fl!("help-next-field"),
        (Context::Dialog, Action::PrevField) => fl!("help-prev-field"),
        (Context::Dialog, Action::Confirm) => fl!("help-confirm"),
        (Context::Dialog, Action::Toggle) => fl!("help-toggle"),
        (Context::Dialog, Action::Cancel) => fl!("help-dialog-cancel"),
        (Context::DialogInput, Action::Home) => fl!("help-field-home"),
        (Context::DialogInput, Action::End) => fl!("help-field-end"),
        (Context::DialogInput, Action::Backspace) => fl!("help-field-backspace"),
        (Context::DialogInput, Action::Delete) => fl!("help-field-delete"),
        (Context::DialogInput, Action::DeleteToStart) => fl!("help-field-delete-to-start"),
        (Context::DialogInput, Action::DeleteToEnd) => fl!("help-field-delete-to-end"),
        (Context::Viewer, Action::Home) => fl!("help-viewer-top"),
        (Context::Viewer, Action::End) => fl!("help-viewer-end"),
        (Context::Viewer, Action::Left) => fl!("help-viewer-left"),
        (Context::Viewer, Action::Right) => fl!("help-viewer-right"),
        (Context::Viewer, Action::ToggleWrap) => fl!("help-viewer-wrap"),
        (Context::Viewer, Action::Quit) => fl!("help-viewer-quit"),
        _ => return None,
    };
    Some(text)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn action(action: Action) -> Resolved {
        Resolved::Action(action)
    }

    fn draw(help: &mut Help, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| help.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn lists_what_the_app_can_do_by_context() {
        let help = Help::new(&Keymap::mc(), true);
        let has = |keys: &str, text: &str| {
            help.entries.contains(&Entry::Keys {
                keys: keys.to_owned(),
                text: text.to_owned(),
            })
        };
        assert!(has("F10", "Quit"));
        assert!(has("F8", "Disconnect the host under the cursor"));
        assert!(has("Ctrl-s, Alt-s", "Quick search; again: the next match"));
        assert!(has("Esc, F10", "Cancel, or close this help"));
        // One key, two meanings.
        let ctrl_u: Vec<&str> = help
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Keys { keys, text } if keys == "Ctrl-u" => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ctrl_u, ["Swap the panels", "Delete to the start"]);
        let headings: Vec<&str> = help
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Heading(title) => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            headings,
            [
                "Panels",
                "Volumes and hosts",
                "Quick search",
                "Location menu",
                "zoxide",
                "Pull-down menu",
                "Dialogs and help",
                "Text fields",
                "Viewer"
            ]
        );
        assert_eq!(
            help.entries.last(),
            Some(&Entry::Note(fl!("help-note-typing")))
        );
        let without_typing = Help::new(&Keymap::mc(), false);
        assert!(matches!(
            without_typing.entries.last(),
            Some(Entry::Note(_))
        ));
        assert_ne!(without_typing.entries.last(), help.entries.last());
    }

    #[test]
    fn long_lists_of_keys_go_on_to_the_next_lines() {
        assert_eq!(key_lines("F10", 16), ["F10"]);
        assert_eq!(key_lines("PageDown, Ctrl-v", 16), ["PageDown, Ctrl-v"]);
        assert_eq!(
            key_lines("Insert, Ctrl-t, Shift-Down", 16),
            ["Insert, Ctrl-t,", "Shift-Down"]
        );
        assert_eq!(key_lines("Esc, Esc Esc", 4), ["Esc,", "Esc Esc"]);
    }

    #[test]
    fn scrolls_within_its_rows_and_closes() {
        let mut help = Help::new(&Keymap::mc(), true);
        let top = draw(&mut help, 60, 12);
        assert!(top.contains("Panels"), "{top}");
        assert!(!help.handle(action(Action::PageDown)));
        assert_ne!(draw(&mut help, 60, 12), top);
        help.handle(action(Action::End));
        let bottom = draw(&mut help, 60, 12);
        assert!(bottom.contains("Typing in a panel"), "{bottom}");
        help.handle(action(Action::Down));
        assert_eq!(draw(&mut help, 60, 12), bottom, "no further than the end");
        help.handle(action(Action::Home));
        assert_eq!(draw(&mut help, 60, 12), top);
        assert!(help.handle(action(Action::Cancel)));
        assert!(help.handle(action(Action::Confirm)));
    }

    #[test]
    fn draws_keys_and_what_they_do() {
        let mut help = Help::new(&Keymap::mc(), true);
        insta::assert_snapshot!(draw(&mut help, 70, 16));
    }
}
