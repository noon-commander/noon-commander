//! The help screen: the keys of what the app can do, read from the keymap.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Colors, draw_box};
use super::keymap::{Action, Context, Keymap, Mode, Resolved};
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
    /// The keys `keymap` binds to what the app can do, by mode and context. `fuzzy_search`
    /// adds a note on matching as fzf does.
    pub(crate) fn new(keymap: &Keymap, fuzzy_search: bool) -> Self {
        let sections = [
            (Mode::Normal, Context::Global, fl!("help-everywhere")),
            (Mode::Normal, Context::Panel, fl!("help-panels")),
            (Mode::Visual, Context::Panel, fl!("help-panels-visual")),
            (Mode::Insert, Context::Input, fl!("help-text-insert")),
            (Mode::Normal, Context::Input, fl!("help-text-normal")),
            (Mode::Visual, Context::Input, fl!("help-text-visual")),
            (Mode::Insert, Context::QuickSearch, fl!("help-quick-search")),
            (Mode::Insert, Context::Rename, fl!("help-renaming-insert")),
            (Mode::Normal, Context::Rename, fl!("help-renaming")),
            (Mode::Insert, Context::CommandLine, fl!("help-command-line")),
            (
                Mode::Normal,
                Context::CommandLine,
                fl!("help-command-line-normal"),
            ),
            (Mode::Normal, Context::List, fl!("help-lists")),
            (Mode::Insert, Context::List, fl!("help-lists-filter")),
            (Mode::Normal, Context::LocationMenu, fl!("help-menu")),
            (Mode::Normal, Context::TabMenu, fl!("help-tab-menu")),
            (Mode::Normal, Context::Workspaces, fl!("help-workspaces")),
            (Mode::Normal, Context::CommandHistory, fl!("help-history")),
            (Mode::Normal, Context::PullDown, fl!("help-pulldown")),
            (Mode::Normal, Context::Dialog, fl!("help-dialogs")),
            (Mode::Normal, Context::Help, fl!("help-this-help")),
            (Mode::Insert, Context::DialogInput, fl!("help-text-fields")),
            (Mode::Insert, Context::PathInput, fl!("help-path-fields")),
            (Mode::Insert, Context::Completion, fl!("help-completion")),
            (Mode::Normal, Context::Viewer, fl!("help-viewer")),
        ];
        let mut entries = Vec::new();
        for (mode, context, title) in sections {
            let rows: Vec<Entry> = keymap
                .help(mode, context)
                .into_iter()
                .filter_map(|(action, keys)| {
                    let text = describe_in((mode, context), action)?;
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
        entries.push(Entry::Note(fl!("help-note-modes")));
        entries.push(Entry::Note(fl!("help-note-count")));
        entries.push(Entry::Note(fl!("help-note-menu")));
        if fuzzy_search {
            entries.push(Entry::Note(fl!("help-note-jump-fuzzy")));
            entries.push(Entry::Note(fl!("help-note-fuzzy")));
        } else {
            entries.push(Entry::Note(fl!("help-note-jump")));
        }
        entries.push(Entry::Note(fl!("help-note-workspaces")));
        entries.push(Entry::Note(fl!("help-note-command")));
        entries.push(Entry::Note(fl!("help-note-pulldown")));
        entries.push(Entry::Note(fl!("help-note-key-hints")));
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
            Action::HalfPageUp => self.offset.saturating_sub(self.page.div_ceil(2)),
            Action::HalfPageDown => self.offset + self.page.div_ceil(2),
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

/// `keys`, a list such as `Space, J`, in lines of at most `width` cells, broken after
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

/// What `action` does in `context` in `mode`: as [`describe`] says, but for the commands of
/// files in Visual mode, which act on the rows selected.
pub(crate) fn describe_in((mode, context): (Mode, Context), action: Action) -> Option<String> {
    let text = match (mode, context, action) {
        (Mode::Visual, Context::Panel, Action::Mark) => fl!("help-visual-mark"),
        (Mode::Visual, Context::Panel, Action::Copy) => fl!("help-visual-copy"),
        (Mode::Visual, Context::Panel, Action::Move) => fl!("help-visual-move"),
        (Mode::Visual, Context::Panel, Action::Delete) => fl!("help-visual-delete"),
        (Mode::Visual, Context::Panel, Action::Checksum) => fl!("help-visual-checksum"),
        _ => return describe(context, action),
    };
    Some(text)
}

/// What `action` does in `context`; `None` for what the app cannot do yet, which the help
/// and the key hints leave out.
pub(crate) fn describe(context: Context, action: Action) -> Option<String> {
    if let Some(text) = describe_rows(context, action)
        .or_else(|| describe_panel(context, action))
        .or_else(|| describe_tabs(context, action))
        .or_else(|| describe_completion(context, action))
        .or_else(|| describe_list(context, action))
        .or_else(|| describe_location_menu(context, action))
        .or_else(|| describe_tab_menu(context, action))
        .or_else(|| describe_help(context, action))
        .or_else(|| describe_workspaces(context, action))
        .or_else(|| describe_command_line(context, action))
        .or_else(|| describe_history(context, action))
        .or_else(|| describe_text(context, action))
    {
        return Some(text);
    }
    let text = match (context, action) {
        (Context::Panel | Context::Viewer, Action::Help) => fl!("help-help"),
        (Context::Global, Action::KeyHints) => fl!("help-key-hints"),
        (Context::Global, Action::UserScreen) => fl!("help-user-screen"),
        (Context::Global, Action::Redraw) => fl!("help-redraw"),
        (Context::Global, Action::Register) => fl!("help-register"),
        (Context::PullDown, Action::Cancel) => fl!("help-pulldown-close"),
        (Context::PullDown, Action::Up) => fl!("help-pulldown-up"),
        (Context::PullDown, Action::Down) => fl!("help-pulldown-down"),
        (Context::PullDown, Action::Left) => fl!("help-pulldown-left"),
        (Context::PullDown, Action::Right) => fl!("help-pulldown-right"),
        (Context::PullDown, Action::Home) => fl!("help-pulldown-home"),
        (Context::PullDown, Action::End) => fl!("help-pulldown-end"),
        (Context::PullDown, Action::Confirm) => fl!("help-pulldown-run"),
        (Context::QuickSearch, Action::Cancel) => fl!("help-search-end"),
        (Context::QuickSearch, Action::Abort) => fl!("help-search-abort"),
        (Context::QuickSearch, Action::QuickSearch) => fl!("help-search-next"),
        (Context::QuickSearch, Action::PrevMatch) => fl!("help-search-prev"),
        (Context::QuickSearch, Action::TextStart) => fl!("help-field-home"),
        (Context::QuickSearch, Action::TextEnd) => fl!("help-field-end"),
        (Context::Dialog | Context::DialogInput, Action::Up) => fl!("help-dialog-up"),
        (Context::Dialog | Context::DialogInput, Action::Down) => fl!("help-dialog-down"),
        (Context::Dialog, Action::Left) => fl!("help-dialog-left"),
        (Context::Dialog, Action::Right) => fl!("help-dialog-right"),
        (Context::Dialog, Action::PageUp) => fl!("help-dialog-page-up"),
        (Context::Dialog, Action::PageDown) => fl!("help-dialog-page-down"),
        (Context::Dialog, Action::Home) => fl!("help-dialog-home"),
        (Context::Dialog, Action::End) => fl!("help-dialog-end"),
        (Context::Dialog | Context::DialogInput, Action::NextField) => fl!("help-next-field"),
        (Context::Dialog | Context::DialogInput, Action::PrevField) => fl!("help-prev-field"),
        (Context::Dialog | Context::DialogInput, Action::Confirm) => fl!("help-confirm"),
        (Context::Dialog, Action::Toggle) => fl!("help-toggle"),
        (Context::Dialog | Context::DialogInput, Action::Cancel) => fl!("help-dialog-cancel"),
        (Context::Jump, Action::Confirm) => fl!("help-jump-go"),
        (Context::Jump, Action::Cancel) => fl!("help-menu-close"),
        (Context::Rename, Action::Confirm) => fl!("help-rename-confirm"),
        (Context::Rename, Action::Cancel) => fl!("help-rename-cancel"),
        (Context::Viewer, Action::Home) => fl!("help-viewer-top"),
        (Context::Viewer, Action::End) => fl!("help-viewer-end"),
        (Context::Viewer, Action::Left) => fl!("help-viewer-left"),
        (Context::Viewer, Action::Right) => fl!("help-viewer-right"),
        (Context::Viewer, Action::ToggleWrap) => fl!("help-viewer-wrap"),
        (Context::Viewer, Action::Quit) => fl!("help-viewer-quit"),
        (Context::QuickSearch, Action::NextMatch) => fl!("help-search-next-match"),
        // The F-keys of quick search do what they do in the panel.
        (Context::QuickSearch, _) => return describe(Context::Panel, action),
        _ => return None,
    };
    Some(text)
}

/// What keys do in a panel, in Normal and in Visual mode.
fn describe_panel(context: Context, action: Action) -> Option<String> {
    if context != Context::Panel {
        return None;
    }
    let text = match action {
        Action::Home => fl!("help-first-row"),
        Action::End => fl!("help-last-row"),
        Action::Enter => fl!("help-enter"),
        Action::Mark => fl!("help-mark"),
        Action::MarkUp => fl!("help-mark-up"),
        Action::InvertMarks => fl!("help-invert-marks"),
        Action::Select => fl!("help-select"),
        Action::Unselect => fl!("help-unselect"),
        Action::Parent => fl!("help-parent"),
        Action::JumpBack => fl!("help-jump-back"),
        Action::JumpForward => fl!("help-jump-forward"),
        Action::VisualMode => fl!("help-visual"),
        Action::Reselect => fl!("help-reselect"),
        Action::OtherEnd => fl!("help-other-end"),
        Action::NormalMode => fl!("help-visual-end"),
        Action::SwitchPanel => fl!("help-switch-panel"),
        Action::FocusLeft => fl!("help-focus-left"),
        Action::FocusRight => fl!("help-focus-right"),
        Action::SwapPanels => fl!("help-swap-panels"),
        Action::OtherPanelOpen => fl!("help-other-open"),
        Action::OtherPanelSync => fl!("help-other-sync"),
        Action::Reload => fl!("help-reload"),
        Action::Cancel => fl!("help-stop"),
        Action::ToggleHidden => fl!("help-toggle-hidden"),
        Action::SortByName => fl!("help-sort-name"),
        Action::SortByExtension => fl!("help-sort-extension"),
        Action::SortByTime => fl!("help-sort-time"),
        Action::SortBySize => fl!("help-sort-size"),
        Action::QuickSearch => fl!("help-quick-search-start"),
        Action::NextMatch => fl!("help-match-next"),
        Action::PrevMatch => fl!("help-match-prev"),
        Action::Shell => fl!("help-shell"),
        Action::Command => fl!("help-command"),
        Action::CommandHistory => fl!("help-command-history"),
        Action::View => fl!("help-view"),
        Action::Edit => fl!("help-edit"),
        Action::Copy => fl!("help-copy"),
        Action::Move => fl!("help-move"),
        Action::Rename => fl!("help-rename"),
        Action::Mkdir => fl!("help-mkdir"),
        Action::Delete => fl!("help-delete"),
        Action::Jobs => fl!("help-jobs"),
        Action::Checksum => fl!("help-checksum"),
        Action::LocationMenuLeft => fl!("help-menu-left"),
        Action::LocationMenuRight => fl!("help-menu-right"),
        Action::Jump => fl!("help-jump-open"),
        Action::QuickCd => fl!("help-quick-cd"),
        Action::PullDown => fl!("help-pulldown-open"),
        Action::PullDownLast => fl!("help-pulldown-last"),
        Action::Quit => fl!("help-quit"),
        _ => return None,
    };
    Some(text)
}

/// What the keys of tabs and workspaces do in a panel.
fn describe_tabs(context: Context, action: Action) -> Option<String> {
    if context != Context::Panel {
        return None;
    }
    let text = match action {
        Action::NewTab => fl!("help-new-tab"),
        Action::CloseTab => fl!("help-close-tab"),
        Action::NextTab => fl!("help-next-tab"),
        Action::PrevTab => fl!("help-prev-tab"),
        Action::TabListLeft => fl!("help-tab-list-left"),
        Action::TabListRight => fl!("help-tab-list-right"),
        Action::SaveWorkspace => fl!("help-save-workspace"),
        Action::Workspaces => fl!("help-workspaces-open"),
        _ => return None,
    };
    Some(text)
}

/// What keys do in the window of the command history.
fn describe_history(context: Context, action: Action) -> Option<String> {
    if context != Context::CommandHistory {
        return None;
    }
    let text = match action {
        Action::NextField => fl!("help-history-hosts"),
        Action::Confirm => fl!("help-history-take"),
        Action::Delete => fl!("help-history-delete"),
        _ => return None,
    };
    Some(text)
}

/// What keys do on the command line that they do not in other text fields.
fn describe_command_line(context: Context, action: Action) -> Option<String> {
    if context != Context::CommandLine {
        return None;
    }
    let text = match action {
        Action::Up => fl!("help-command-up"),
        Action::Down => fl!("help-command-down"),
        Action::TextStart => fl!("help-command-home"),
        Action::TextEnd => fl!("help-command-end"),
        Action::NewLine => fl!("help-command-new-line"),
        Action::NormalMode => fl!("help-command-normal"),
        Action::OlderCommand => fl!("help-command-older"),
        Action::NewerCommand => fl!("help-command-newer"),
        Action::CommandHistory => fl!("help-command-history"),
        Action::Confirm => fl!("help-command-run"),
        Action::Cancel => fl!("help-command-cancel"),
        Action::Abort => fl!("help-command-close"),
        _ => return None,
    };
    Some(text)
}

/// What the keys of text fields do, in vim's modes: in quick search, the field of an entry
/// renamed in its row, the text fields of dialogs, the command line, and the filters of lists.
fn describe_text(context: Context, action: Action) -> Option<String> {
    if !matches!(
        context,
        Context::Input
            | Context::QuickSearch
            | Context::Rename
            | Context::DialogInput
            | Context::PathInput
            | Context::CommandLine
            | Context::List
    ) {
        return None;
    }
    let text = match action {
        Action::Left => fl!("help-field-left"),
        Action::Right => fl!("help-field-right"),
        Action::CharForward => fl!("help-field-char-forward"),
        Action::CharBackward => fl!("help-field-char-backward"),
        Action::TextStart => fl!("help-field-home"),
        Action::FirstNonBlank => fl!("help-field-first-non-blank"),
        Action::TextEnd => fl!("help-field-end"),
        Action::WordForward => fl!("help-field-word-forward"),
        Action::WordBackward => fl!("help-field-word-backward"),
        Action::WordEnd => fl!("help-field-word-end"),
        Action::BigWordForward => fl!("help-field-big-word-forward"),
        Action::BigWordBackward => fl!("help-field-big-word-backward"),
        Action::BigWordEnd => fl!("help-field-big-word-end"),
        Action::FindChar => fl!("help-field-find"),
        Action::FindCharBack => fl!("help-field-find-back"),
        Action::TillChar => fl!("help-field-till"),
        Action::TillCharBack => fl!("help-field-till-back"),
        Action::RepeatFind => fl!("help-field-repeat-find"),
        Action::RepeatFindBack => fl!("help-field-repeat-find-back"),
        Action::Backspace => fl!("help-field-backspace"),
        Action::DeleteChar => fl!("help-field-delete"),
        Action::DeleteToStart => fl!("help-field-delete-to-start"),
        Action::DeleteToEnd => fl!("help-field-delete-to-end"),
        Action::DeleteWord => fl!("help-field-delete-word"),
        Action::InsertMode => fl!("help-field-insert"),
        Action::Append => fl!("help-field-append"),
        Action::InsertAtStart => fl!("help-field-insert-at-start"),
        Action::AppendAtEnd => fl!("help-field-append-at-end"),
        Action::VisualMode => fl!("help-field-visual"),
        Action::OtherEnd => fl!("help-other-end"),
        Action::NormalMode => fl!("help-field-normal"),
        Action::NormalOnce => fl!("help-field-normal-once"),
        Action::Substitute => fl!("help-field-substitute"),
        Action::SubstituteLine => fl!("help-field-substitute-line"),
        Action::ChangeToEnd => fl!("help-field-change-to-end"),
        Action::ReplaceChar => fl!("help-field-replace"),
        Action::ToggleCase => fl!("help-field-toggle-case"),
        Action::DeleteOperator => fl!("help-field-delete-operator"),
        Action::ChangeOperator => fl!("help-field-change-operator"),
        Action::YankOperator => fl!("help-field-yank-operator"),
        Action::PutAfter => fl!("help-field-put-after"),
        Action::PutBefore => fl!("help-field-put-before"),
        Action::Undo => fl!("help-field-undo"),
        Action::Redo => fl!("help-field-redo"),
        Action::PasteRegister => fl!("help-field-paste-register"),
        Action::InsertLiteral => fl!("help-field-literal"),
        _ => return None,
    };
    Some(text)
}

/// What the arrows and the page keys do in lists of rows.
fn describe_rows(context: Context, action: Action) -> Option<String> {
    let rows = matches!(
        context,
        Context::Panel | Context::Viewer | Context::Help | Context::List | Context::Completion
    );
    let text = match action {
        Action::Up if rows => fl!("help-row-up"),
        Action::Down if rows => fl!("help-row-down"),
        Action::PageUp if rows => fl!("help-page-up"),
        Action::PageDown if rows => fl!("help-page-down"),
        Action::HalfPageUp if rows => fl!("help-half-page-up"),
        Action::HalfPageDown if rows => fl!("help-half-page-down"),
        _ => return None,
    };
    Some(text)
}

/// What the keys that the windows listing items share do.
fn describe_list(context: Context, action: Action) -> Option<String> {
    if context != Context::List {
        return None;
    }
    let text = match action {
        Action::Home => fl!("help-first-row"),
        Action::End => fl!("help-last-row"),
        Action::QuickSearch => fl!("help-list-filter"),
        Action::NormalMode => fl!("help-list-rows"),
        Action::Confirm => fl!("help-list-take"),
        Action::Cancel => fl!("help-list-close"),
        _ => return None,
    };
    Some(text)
}

/// What `action` does in the location menu, beyond what the lists do.
fn describe_location_menu(context: Context, action: Action) -> Option<String> {
    if context != Context::LocationMenu {
        return None;
    }
    let text = match action {
        Action::Confirm => fl!("help-menu-open"),
        Action::Cancel => fl!("help-menu-close"),
        Action::EditHost => fl!("help-edit-host"),
        Action::Disconnect => fl!("help-disconnect"),
        Action::Reload => fl!("help-menu-reload"),
        _ => return None,
    };
    Some(text)
}

/// What `action` does in the tab menu, beyond what the lists do.
fn describe_tab_menu(context: Context, action: Action) -> Option<String> {
    if context != Context::TabMenu {
        return None;
    }
    let text = match action {
        Action::Confirm => fl!("help-tab-menu-show"),
        Action::Cancel => fl!("help-menu-close"),
        Action::CloseTab => fl!("help-tab-menu-close"),
        _ => return None,
    };
    Some(text)
}

/// What `action` does in this help, beyond scrolling it by rows and pages.
fn describe_help(context: Context, action: Action) -> Option<String> {
    if context != Context::Help {
        return None;
    }
    let text = match action {
        Action::Home => fl!("help-this-help-top"),
        Action::End => fl!("help-this-help-end"),
        Action::Cancel => fl!("help-this-help-close"),
        _ => return None,
    };
    Some(text)
}

/// What `action` does in the window of the saved workspaces, beyond what the lists do.
fn describe_workspaces(context: Context, action: Action) -> Option<String> {
    if context != Context::Workspaces {
        return None;
    }
    let text = match action {
        Action::SaveWorkspace => fl!("help-workspaces-save"),
        Action::Confirm => fl!("help-workspaces-restore"),
        Action::Move => fl!("help-workspaces-rename"),
        Action::Delete => fl!("help-workspaces-delete"),
        Action::Cancel => fl!("help-workspaces-close"),
        _ => return None,
    };
    Some(text)
}

/// What `action` does in path fields and the list of completions, beyond moving the cursor.
fn describe_completion(context: Context, action: Action) -> Option<String> {
    let text = match (context, action) {
        (Context::PathInput, Action::Complete) => fl!("help-complete"),
        (Context::Completion, Action::Complete) => fl!("help-complete-next"),
        (Context::Completion, Action::Confirm) => fl!("help-complete-take"),
        (Context::Completion, Action::Cancel) => fl!("help-complete-close"),
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
    fn lists_what_the_app_can_do_by_mode_and_context() {
        let help = Help::new(&Keymap::default(), true);
        let has = |keys: &str, text: &str| {
            help.entries.contains(&Entry::Keys {
                keys: keys.to_owned(),
                text: text.to_owned(),
            })
        };
        assert!(has("Z Z, F10", "Quit"));
        assert!(has("d d, F8", "Disconnect the host under the cursor"));
        assert!(has("/", "Quick search, typed as vim's search"));
        assert!(has(
            "Space",
            "Mark the rows selected, and Normal mode again"
        ));
        assert!(has("y, F5", "Copy the rows selected"));
        assert!(has(
            "n, Shift+F7",
            "The next name that matches the last quick search"
        ));
        assert!(has("Ctrl+w h", "The left panel"));
        assert!(has(
            "Ctrl+o",
            "Back to where the panel was before its last jump, as vim's jump list"
        ));
        assert!(has("Tab, Ctrl+i", "Forward again, where Ctrl+o came from"));
        assert!(has(
            "Esc, Ctrl+[, Ctrl+c, q, F10",
            "Cancel, or close this help"
        ));
        assert!(has(
            "Ctrl+g",
            "The next match; with nothing typed, the last search"
        ));
        assert!(has("u", "Undo the last change"));
        assert!(has("/", "Type in the filter, in Insert mode"));
        // One key, three meanings, by mode and context.
        let ctrl_u: Vec<&str> = help
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Keys { keys, text } if keys == "Ctrl+u" => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ctrl_u,
            [
                "Half a page up",
                "Half a page up",
                "Delete to the start of the line",
                "Half a page up"
            ]
        );
        // The help's own keys, which scroll it as less does.
        assert!(has("Space, f", "One page down"));
        assert_eq!(
            help.entries.last(),
            Some(&Entry::Note(fl!("help-note-key-hints")))
        );
        let fuzzy = Entry::Note(fl!("help-note-fuzzy"));
        assert!(help.entries.contains(&fuzzy));
        let keywords = Help::new(&Keymap::default(), false);
        assert!(!keywords.entries.contains(&fuzzy));
        assert!(
            keywords
                .entries
                .contains(&Entry::Note(fl!("help-note-jump")))
        );
    }

    #[test]
    fn heads_each_mode_and_context_with_keys() {
        let help = Help::new(&Keymap::default(), true);
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
                "Everywhere",
                "Panels",
                "Panels in Visual mode",
                "Text fields in Insert mode",
                "Text fields in Normal mode",
                "Text fields in Visual mode",
                "Quick search",
                "Renaming in place",
                "Renaming in place, in Normal mode",
                "Command line",
                "Command line in Normal mode",
                "Lists",
                "The filter of a list",
                "Location menu",
                "Tab menu",
                "Workspaces",
                "Command history",
                "Pull-down menu",
                "Dialogs and help",
                "This help",
                "Text fields of dialogs",
                "Path fields",
                "Completion list",
                "Viewer"
            ]
        );
    }

    #[test]
    fn long_lists_of_keys_go_on_to_the_next_lines() {
        assert_eq!(key_lines("F10", 16), ["F10"]);
        assert_eq!(key_lines("PgDn, Ctrl+v", 16), ["PgDn, Ctrl+v"]);
        assert_eq!(
            key_lines("Insert, Ctrl+t, Shift+Down", 16),
            ["Insert, Ctrl+t,", "Shift+Down"]
        );
        assert_eq!(key_lines("Esc, Esc Esc", 4), ["Esc,", "Esc Esc"]);
    }

    #[test]
    fn scrolls_within_its_rows_and_closes() {
        let mut help = Help::new(&Keymap::default(), true);
        let top = draw(&mut help, 60, 14);
        assert!(top.contains("Panels"), "{top}");
        assert!(!help.handle(action(Action::PageDown)));
        assert_ne!(draw(&mut help, 60, 14), top);
        help.handle(action(Action::End));
        let bottom = draw(&mut help, 60, 14);
        assert!(bottom.contains("Backspace takes back"), "{bottom}");
        help.handle(action(Action::Down));
        assert_eq!(draw(&mut help, 60, 14), bottom, "no further than the end");
        help.handle(action(Action::Home));
        assert_eq!(draw(&mut help, 60, 14), top);
        assert!(help.handle(action(Action::Cancel)));
        assert!(help.handle(action(Action::Confirm)));
    }

    #[test]
    fn draws_keys_and_what_they_do() {
        let mut help = Help::new(&Keymap::default(), true);
        insta::assert_snapshot!(draw(&mut help, 70, 16));
    }
}
