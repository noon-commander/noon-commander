//! The help of F1, laid out as the Configuration dialog: its sections down the left, and the
//! text of the chosen one on the right, which scrolls. About says what the app is, what it was
//! built from, what it runs on, and where it keeps its files. Keys, which folds, has the keys of
//! each part of the app, read from the keymap, and tips.

use std::path::{Path, PathBuf};

use noc_scroll::scrollbar::{self, Bar};
use noc_scroll::scrolled;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Colors, draw_box, right_frame};
use super::glyphs::{self, categories};
use super::keymap::{Action, Context, Keymap, Mode, Resolved};
use super::label::Label;
use super::mouse::{Pointer, Press, Scroll};
use super::panel::home_text;
use super::sidebar::{self, Sidebar};
use super::theme::Theme;
use crate::about::{Build, Fact, Report, Topic, Value};
use crate::i18n::fl;

/// How much of the screen the help takes, in percent of its width and of its height.
const WIDTH_PERCENT: u16 = 70;
const HEIGHT_PERCENT: u16 = 70;
/// Widest the key column gets; longer lists of keys go on to the next lines.
const MAX_KEYS_WIDTH: usize = 16;
/// How far the groups of keys are indented under Keys in the sidebar.
const GROUP_INDENT: &str = "  ";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Heading(String),
    Keys {
        keys: String,
        text: String,
    },
    /// A fact on the About page: what it is, and its value.
    Fact {
        name: String,
        value: String,
    },
    Note(String),
    /// A point of a tip, after a bullet, its lines indented under its first.
    Point(String),
    Blank,
}

/// What the About page tells: the report of `noc debug version`, and where the files are.
#[derive(Debug, Clone)]
pub(crate) struct About {
    pub(crate) report: Report,
    /// Where the files are, once the app knows.
    pub(crate) files: Option<Files>,
}

impl About {
    /// What the app knows before it asks the system.
    pub(crate) fn new(build: Build) -> Self {
        Self {
            report: Report::new(build),
            files: None,
        }
    }
}

/// The files the app keeps, and the home directory, from which they are shown with `~`.
#[derive(Debug, Clone)]
pub(crate) struct Files {
    pub(crate) home: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) hosts: PathBuf,
    pub(crate) workspaces: PathBuf,
    pub(crate) history: PathBuf,
    pub(crate) recent: PathBuf,
    pub(crate) log: PathBuf,
}

/// A part of the app with keys of its own, a group under Keys; the tips come last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Everywhere,
    Panels,
    TextFields,
    SearchAndRenaming,
    CommandLine,
    ListsAndMenus,
    MenusAndDialogs,
    Viewer,
    Tips,
}

impl Part {
    const ALL: [Self; 9] = [
        Self::Everywhere,
        Self::Panels,
        Self::TextFields,
        Self::SearchAndRenaming,
        Self::CommandLine,
        Self::ListsAndMenus,
        Self::MenusAndDialogs,
        Self::Viewer,
        Self::Tips,
    ];

    fn title(self) -> String {
        match self {
            Self::Everywhere => fl!("help-group-everywhere"),
            Self::Panels => fl!("help-group-panels"),
            Self::TextFields => fl!("help-group-text"),
            Self::SearchAndRenaming => fl!("help-group-search"),
            Self::CommandLine => fl!("help-group-command-line"),
            Self::ListsAndMenus => fl!("help-group-lists"),
            Self::MenusAndDialogs => fl!("help-group-dialogs"),
            Self::Viewer => fl!("help-group-viewer"),
            Self::Tips => fl!("help-group-tips"),
        }
    }

    /// The modes and contexts whose keys the part lists, each under its heading.
    fn sections(self) -> Vec<(Mode, Context, String)> {
        match self {
            Self::Everywhere => vec![(Mode::Normal, Context::Global, fl!("help-everywhere"))],
            Self::Panels => vec![
                (Mode::Normal, Context::Panel, fl!("help-panels")),
                (Mode::Visual, Context::Panel, fl!("help-panels-visual")),
            ],
            Self::TextFields => vec![
                (Mode::Insert, Context::Input, fl!("help-text-insert")),
                (Mode::Normal, Context::Input, fl!("help-text-normal")),
                (Mode::Visual, Context::Input, fl!("help-text-visual")),
                (Mode::Insert, Context::DialogInput, fl!("help-text-fields")),
                (Mode::Insert, Context::PathInput, fl!("help-path-fields")),
                (Mode::Insert, Context::Completion, fl!("help-completion")),
            ],
            Self::SearchAndRenaming => vec![
                (Mode::Insert, Context::QuickSearch, fl!("help-quick-search")),
                (
                    Mode::Insert,
                    Context::SearchInput,
                    fl!("help-search-fields"),
                ),
                (Mode::Insert, Context::FindInput, fl!("help-find-fields")),
                (Mode::Normal, Context::Find, fl!("help-find")),
                (Mode::Insert, Context::Rename, fl!("help-renaming-insert")),
                (Mode::Normal, Context::Rename, fl!("help-renaming")),
            ],
            Self::CommandLine => vec![
                (Mode::Insert, Context::CommandLine, fl!("help-command-line")),
                (
                    Mode::Normal,
                    Context::CommandLine,
                    fl!("help-command-line-normal"),
                ),
                (Mode::Normal, Context::CommandHistory, fl!("help-history")),
            ],
            Self::ListsAndMenus => vec![
                (Mode::Normal, Context::List, fl!("help-lists")),
                (Mode::Insert, Context::List, fl!("help-lists-filter")),
                (Mode::Normal, Context::LocationMenu, fl!("help-menu")),
                (Mode::Normal, Context::TabMenu, fl!("help-tab-menu")),
                (Mode::Normal, Context::Workspaces, fl!("help-workspaces")),
            ],
            Self::MenusAndDialogs => vec![
                (Mode::Normal, Context::PullDown, fl!("help-pulldown")),
                (Mode::Normal, Context::Dialog, fl!("help-dialogs")),
                (Mode::Normal, Context::Help, fl!("help-this-help")),
            ],
            Self::Viewer => vec![(Mode::Normal, Context::Viewer, fl!("help-viewer"))],
            Self::Tips => Vec::new(),
        }
    }

    /// Its text: the keys `keymap` binds in each of its sections, under the section's heading
    /// when it has more than one.
    fn entries(self, keymap: &Keymap) -> Vec<Entry> {
        let sections = self.sections();
        let headed = sections.len() > 1;
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
            if rows.is_empty() {
                continue;
            }
            if !entries.is_empty() {
                entries.push(Entry::Blank);
            }
            if headed {
                entries.push(Entry::Heading(title));
            }
            entries.extend(rows);
        }
        entries
    }
}

/// The tips, after the keys: how the modes, counts, and the windows work, each under its title
/// in points.
fn tips() -> Vec<Entry> {
    let tips = vec![
        (
            fl!("help-tip-modes"),
            vec![
                fl!("help-tip-modes-keys"),
                fl!("help-tip-modes-fields"),
                fl!("help-tip-modes-close"),
                fl!("help-tip-modes-line"),
                fl!("help-tip-modes-alt"),
                fl!("help-tip-modes-cancel"),
            ],
        ),
        (
            fl!("help-tip-counts"),
            vec![
                fl!("help-tip-counts-count"),
                fl!("help-tip-counts-register"),
            ],
        ),
        (
            fl!("help-tip-lists"),
            vec![fl!("help-tip-lists-open"), fl!("help-tip-lists-jump")],
        ),
        (
            fl!("help-tip-search"),
            vec![
                fl!("help-tip-search-where"),
                fl!("help-tip-search-plain"),
                fl!("help-tip-search-fzf"),
                fl!("help-tip-search-regex"),
            ],
        ),
        (
            fl!("help-tip-workspaces"),
            vec![
                fl!("help-tip-workspaces-what"),
                fl!("help-tip-workspaces-list"),
            ],
        ),
        (
            fl!("help-tip-command"),
            vec![fl!("help-tip-command-open"), fl!("help-tip-command-run")],
        ),
        (
            fl!("help-tip-pulldown"),
            vec![fl!("help-tip-pulldown-letters")],
        ),
        (
            fl!("help-tip-dialogs"),
            vec![fl!("help-tip-dialogs-letters")],
        ),
        (
            fl!("help-tip-key-hints"),
            vec![
                fl!("help-tip-key-hints-show"),
                fl!("help-tip-key-hints-keys"),
                fl!("help-tip-key-hints-list"),
            ],
        ),
    ];
    let mut entries = Vec::new();
    for (title, points) in tips {
        if !entries.is_empty() {
            entries.push(Entry::Blank);
        }
        entries.push(Entry::Heading(title));
        entries.extend(points.into_iter().map(Entry::Point));
    }
    entries
}

/// The About page: what the app is, every fact of the report of `noc debug version` under
/// the title of its topic, and where the files are.
fn about_entries(about: &About) -> Vec<Entry> {
    let report = &about.report;
    let build = &report.build;
    let fact = |name: String, value: Value| Entry::Fact {
        name,
        value: match value {
            Value::Text(text) => text,
            Value::Unknown => fl!("help-about-unknown"),
            Value::Missing => fl!("help-about-missing"),
        },
    };
    let mut entries = vec![
        Entry::Heading(fl!("help-about-name", version = build.version)),
        Entry::Note(fl!("help-about-description")),
        Entry::Blank,
        fact(
            fl!("help-about-license"),
            Value::Text(build.license.to_owned()),
        ),
        fact(
            fl!("help-about-source"),
            Value::Text(build.repository.to_owned()),
        ),
    ];
    let mut topic = None;
    for each in Fact::ALL {
        let Some(name) = fact_name(each) else {
            continue;
        };
        if topic != Some(each.topic()) {
            topic = Some(each.topic());
            entries.push(Entry::Blank);
            entries.extend(topic_title(each.topic()).map(Entry::Heading));
        }
        entries.push(fact(name, report.value(each)));
    }
    if let Some(files) = &about.files {
        let path = |name: String, path: &Path| Entry::Fact {
            name,
            value: home_text(path, &files.home),
        };
        entries.extend([
            Entry::Blank,
            Entry::Heading(fl!("help-about-files")),
            path(fl!("help-about-config"), &files.config),
            path(fl!("help-about-hosts"), &files.hosts),
            path(fl!("help-about-workspaces"), &files.workspaces),
            path(fl!("help-about-history"), &files.history),
            path(fl!("help-about-recent"), &files.recent),
            path(fl!("help-about-log"), &files.log),
        ]);
    }
    entries
}

/// The name of `fact` on the About page; `None` for the name and the version, which its first
/// title has.
fn fact_name(fact: Fact) -> Option<String> {
    Some(match fact {
        Fact::Name | Fact::Version => return None,
        Fact::Features => fl!("help-about-features"),
        Fact::Commit => fl!("help-about-commit"),
        Fact::Dirty => fl!("help-about-dirty"),
        Fact::CommitTime => fl!("help-about-commit-time"),
        Fact::Target => fl!("help-about-target"),
        Fact::Profile => fl!("help-about-profile"),
        Fact::Os => fl!("help-about-os"),
        Fact::Kernel => fl!("help-about-kernel"),
        Fact::Arch => fl!("help-about-arch"),
        Fact::Terminal => fl!("help-about-terminal-program"),
        Fact::Term => fl!("help-about-term"),
        Fact::Multiplexer => fl!("help-about-multiplexer"),
        Fact::Colors => fl!("help-about-colors"),
        Fact::Keyboard => fl!("help-about-keyboard"),
        Fact::Editor => fl!("help-about-editor"),
        Fact::Zoxide => fl!("help-about-zoxide"),
        Fact::Ripgrep => fl!("help-about-ripgrep"),
        Fact::Fd => fl!("help-about-fd"),
        Fact::Language => fl!("help-about-language"),
        Fact::Formats => fl!("help-about-formats"),
    })
}

/// The title of `topic` on the About page; `None` for the name and the version.
fn topic_title(topic: Topic) -> Option<String> {
    Some(match topic {
        Topic::App => return None,
        Topic::Build => fl!("help-about-build"),
        Topic::System => fl!("help-about-system"),
        Topic::Terminal => fl!("help-about-terminal"),
        Topic::Tools => fl!("help-about-tools"),
        Topic::Region => fl!("help-about-region"),
    })
}

/// Where the help was when it closed, to open there again: the section, whether Keys was open,
/// what had the keys, and how far down the text was. At first, the top of About, with Keys
/// open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Spot {
    row: Row,
    group: usize,
    keys_open: bool,
    focus: Focus,
    offset: usize,
}

impl Default for Spot {
    fn default() -> Self {
        Self {
            row: Row::About,
            group: 0,
            keys_open: true,
            focus: Focus::Text,
            offset: 0,
        }
    }
}

/// A group of keys under Keys: its title in the sidebar, with its letter, and its text.
#[derive(Debug)]
struct Group {
    title: Label,
    entries: Vec<Entry>,
}

/// A row of the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    About,
    Keys,
    /// A group of keys, by its index, shown while Keys is open.
    Group(usize),
}

/// Where the keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Sidebar,
    Text,
}

/// The help, its sections and the text of the one chosen, scrolled to `offset`.
#[derive(Debug)]
pub(crate) struct Help {
    about: Vec<Entry>,
    groups: Vec<Group>,
    /// The row of the sidebar with the cursor: a group only while Keys is open.
    row: Row,
    /// The group of keys chosen last, which shows while Keys has the cursor.
    group: usize,
    keys_open: bool,
    focus: Focus,
    /// First screen row of the text shown.
    offset: usize,
    /// Rows the text had at the last render, the one that marks its end among them, and how
    /// many fit on screen.
    rows: usize,
    page: usize,
    /// The scroll bar of the last render, if the text did not fit.
    bar: Option<Bar>,
    /// Where the last render drew the rows of the sidebar and the text, for the mouse.
    drawn_rows: Vec<(Row, Rect)>,
    drawn_text: Rect,
}

impl Help {
    /// The help with the keys `keymap` binds to what the app can do, by part of the app, and
    /// `about` on its About page. It opens at the top of About, with Keys open and the text
    /// having the keys.
    pub(crate) fn new(keymap: &Keymap, about: &About) -> Self {
        let groups = Part::ALL
            .into_iter()
            .map(|part| Group {
                title: Label::parse(&part.title()),
                entries: match part {
                    Part::Tips => tips(),
                    part => part.entries(keymap),
                },
            })
            .collect();
        Self {
            about: about_entries(about),
            groups,
            row: Row::About,
            group: 0,
            keys_open: true,
            focus: Focus::Text,
            offset: 0,
            rows: 0,
            page: 1,
            bar: None,
            drawn_rows: Vec::new(),
            drawn_text: Rect::default(),
        }
    }

    /// Opens at `spot`, where it was when it closed last.
    pub(crate) fn at(mut self, spot: Spot) -> Self {
        let Spot {
            row,
            group,
            keys_open,
            focus,
            offset,
        } = spot;
        self.group = group.min(self.groups.len().saturating_sub(1));
        self.keys_open = keys_open;
        self.row = match row {
            Row::Group(_) => self.keys_row(),
            row => row,
        };
        self.focus = focus;
        self.offset = offset;
        self
    }

    /// Where it is, to open there the next time.
    pub(crate) fn spot(&self) -> Spot {
        Spot {
            row: self.row,
            group: self.group,
            keys_open: self.keys_open,
            focus: self.focus,
            offset: self.offset,
        }
    }

    /// The row of the group chosen last: its own while Keys is open, else that of Keys.
    fn keys_row(&self) -> Row {
        if self.keys_open {
            Row::Group(self.group)
        } else {
            Row::Keys
        }
    }

    /// The rows the sidebar shows.
    fn sidebar_rows(&self) -> Vec<Row> {
        let mut rows = vec![Row::About, Row::Keys];
        if self.keys_open {
            rows.extend((0..self.groups.len()).map(Row::Group));
        }
        rows
    }

    /// The text shown: the About page, or a group of keys.
    fn entries(&self) -> &[Entry] {
        match self.row {
            Row::About => &self.about,
            Row::Keys | Row::Group(_) => self
                .groups
                .get(self.group)
                .map_or(&[], |group| group.entries.as_slice()),
        }
    }

    /// Puts the cursor of the sidebar on `row`, and the text at its top if it shows another.
    fn choose(&mut self, row: Row) {
        let shown = self.shown();
        self.row = row;
        if let Row::Group(group) = row {
            self.group = group;
        }
        if self.shown() != shown {
            self.offset = 0;
        }
    }

    /// What the text shows: the About page, or the group of keys by its index.
    fn shown(&self) -> Option<usize> {
        match self.row {
            Row::About => None,
            Row::Keys | Row::Group(_) => Some(self.group),
        }
    }

    /// Opens Keys under the cursor, or gives the keys to the text.
    fn open(&mut self) {
        if self.row == Row::Keys && !self.keys_open {
            self.keys_open = true;
        } else {
            self.focus = Focus::Text;
        }
    }

    /// Goes up from a group to Keys, or folds Keys.
    fn fold(&mut self) {
        match self.row {
            Row::Group(_) => self.choose(Row::Keys),
            Row::Keys => self.keys_open = false,
            Row::About => {}
        }
    }

    /// Moves the cursor of the sidebar `by` rows, up when negative, within them.
    fn move_by(&mut self, by: isize) {
        let rows = self.sidebar_rows();
        let at = rows.iter().position(|row| *row == self.row).unwrap_or(0);
        let to = at.saturating_add_signed(by).min(rows.len() - 1);
        self.choose(rows[to]);
    }

    /// The scroll bar of the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.bar
    }

    /// Scrolls the text as a press on the scroll bar asks, never past the end at the bottom.
    pub(crate) fn scroll(&mut self, scroll: Scroll) {
        self.offset = match scroll {
            Scroll::By(motion) => scrolled(motion, self.offset, self.rows, self.page),
            Scroll::To(offset) => offset.min(self.rows.saturating_sub(self.page)),
        };
    }

    /// Scrolls the text as a step of the wheel does, whatever has the keys.
    pub(crate) fn wheel(&mut self, action: Action) {
        if let Some(motion) = action.motion() {
            self.scroll(Scroll::By(motion));
        }
    }

    /// Takes a key; `true` when the help should close. Tab moves between the sidebar and the
    /// text. In the sidebar, Up and Down choose a section, Right and Enter open Keys or give
    /// the keys to the text, and Left goes up to Keys and folds it; the other motions scroll
    /// the text. In the text, the motions scroll it, never past its end at the bottom, Left
    /// goes back to the sidebar, and Enter closes. Alt and the letter of a section show it,
    /// from either.
    pub(crate) fn handle(&mut self, input: Resolved) -> bool {
        let action = match input {
            Resolved::Action(action) => action,
            Resolved::Hotkey(c) => {
                self.hotkey(c);
                return false;
            }
            Resolved::Insert(_) => return false,
        };
        match (self.focus, action) {
            (_, Action::Cancel) | (Focus::Text, Action::Confirm) => return true,
            (Focus::Sidebar, Action::NextField | Action::PrevField) => self.focus = Focus::Text,
            (Focus::Text, Action::NextField | Action::PrevField | Action::Left) => {
                self.focus = Focus::Sidebar;
            }
            (Focus::Sidebar, Action::Up) => self.move_by(-1),
            (Focus::Sidebar, Action::Down) => self.move_by(1),
            (Focus::Sidebar, Action::Home | Action::PageUp) => self.move_by(isize::MIN),
            (Focus::Sidebar, Action::End | Action::PageDown) => self.move_by(isize::MAX),
            (Focus::Sidebar, Action::Right | Action::Confirm | Action::Toggle) => self.open(),
            (Focus::Sidebar, Action::Left) => self.fold(),
            (_, action) => {
                if let Some(motion) = action.motion() {
                    self.scroll(Scroll::By(motion));
                }
            }
        }
        false
    }

    /// Alt and `c`, as in the dialogs: the section with that letter shows, Keys opening for one
    /// of its groups, and its text has the keys, to scroll it. Nothing else has it.
    fn hotkey(&mut self, c: char) {
        let (about, keys) = (fl!("help-section-about"), fl!("help-section-keys"));
        let row = if Label::parse(&about).has(c) {
            Row::About
        } else if Label::parse(&keys).has(c) {
            Row::Keys
        } else if let Some(group) = self.groups.iter().position(|group| group.title.has(c)) {
            Row::Group(group)
        } else {
            return;
        };
        if row != Row::About {
            self.keys_open = true;
        }
        self.choose(row);
        self.focus = Focus::Text;
    }

    /// Takes a press of the mouse, where the help was drawn last: a click on a section shows
    /// it, and one on Keys also opens or folds it; a click on the text gives it the keys.
    pub(crate) fn pointer(&mut self, Pointer { press, at }: Pointer) {
        if !matches!(press, Press::Click | Press::DoubleClick) {
            return;
        }
        let clicked = self
            .drawn_rows
            .iter()
            .find(|(_, area)| area.contains(at))
            .map(|(row, _)| *row);
        if let Some(row) = clicked {
            self.focus = Focus::Sidebar;
            if row == Row::Keys {
                self.choose(Row::Keys);
                self.keys_open = !self.keys_open;
            } else {
                self.choose(row);
            }
        } else if self.drawn_text.contains(at) {
            self.focus = Focus::Text;
        }
    }

    /// The rows of the sidebar as it draws them, and the cells the widest of them takes, of all
    /// the rows it can show, so that it keeps its width as Keys opens and folds.
    fn sidebar_texts(&self, theme: &Theme) -> (Vec<Label>, usize) {
        let icons = theme.glyphs.icons;
        let top = |icon: &str, title: String| {
            if icons {
                format!(" {icon} {title} ")
            } else {
                format!(" {title} ")
            }
        };
        let (closed, open) = theme.glyphs.folds;
        let fold = if self.keys_open { open } else { closed };
        let about = top(categories::ABOUT, fl!("help-section-about"));
        let keys = format!("{}{fold} ", top(categories::KEYS, fl!("help-section-keys")));
        let groups: Vec<Label> = self
            .groups
            .iter()
            .map(|group| {
                let mut title = group.title.clone();
                let indent = GROUP_INDENT.chars().count();
                title.text = format!("{GROUP_INDENT}{} ", title.text);
                title.hotkey = title.hotkey.map(|(at, letter)| (at + indent, letter));
                title
            })
            .collect();
        let (about, keys) = (Label::parse(&about), Label::parse(&keys));
        let width = sidebar::width(&groups)
            .max(cells::width(&about.text))
            .max(cells::width(&keys.text));
        let mut texts = vec![about, keys];
        if self.keys_open {
            texts.extend(groups);
        }
        (texts, width)
    }

    /// Draws the help centered in `area`, over [`WIDTH_PERCENT`] of its width and
    /// [`HEIGHT_PERCENT`] of its height, with the scroll bar of its text on the right of the
    /// frame.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let width = percent(area.width, WIDTH_PERCENT).max(20);
        let height = percent(area.height, HEIGHT_PERCENT).max(3);
        let colors = Colors::of(theme, false);
        let size = (width, height);
        let inner = draw_box(frame, area, size, &fl!("help-title"), colors, theme);
        let rows = self.sidebar_rows();
        let (texts, sidebar_width) = self.sidebar_texts(theme);
        let sidebar = Sidebar {
            rows: &texts,
            width: sidebar_width,
            chosen: rows.iter().position(|row| *row == self.row).unwrap_or(0),
            focused: self.focus == Focus::Sidebar,
        };
        let drawn = sidebar::render(frame, inner, sidebar, theme, colors);
        self.drawn_rows = drawn
            .rows
            .iter()
            .filter_map(|(index, area)| Some((*rows.get(*index)?, *area)))
            .collect();
        let text = drawn.content;
        self.drawn_text = text;
        let lines = self.lines(usize::from(text.width), theme);
        self.rows = lines.len();
        self.page = usize::from(inner.height).max(1);
        self.offset = self.offset.min(self.rows.saturating_sub(self.page));
        let position = (self.rows, self.page, self.offset);
        let bar = right_frame(inner, inner.y, inner.height);
        self.bar = scrollbar::render(frame, bar, position, theme.dialog, theme.glyphs.scroll_bar);
        self.render_position(frame, inner, theme);
        for (index, line) in lines
            .into_iter()
            .skip(self.offset)
            .take(self.page)
            .enumerate()
        {
            let y = text.y + u16::try_from(index).unwrap_or(u16::MAX);
            frame.render_widget(line, Rect::new(text.x, y, text.width, 1));
        }
    }

    /// The lines of the text shown, in `width` cells.
    fn lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let entries = self.entries();
        let column = |names: &mut dyn Iterator<Item = &String>, most: usize| {
            names
                .map(|name| cells::width(name))
                .max()
                .unwrap_or(0)
                .min(most)
                .min(width / 2)
        };
        let keys_width = column(
            &mut entries.iter().filter_map(|entry| match entry {
                Entry::Keys { keys, .. } => Some(keys),
                _ => None,
            }),
            MAX_KEYS_WIDTH,
        );
        let names_width = column(
            &mut entries.iter().filter_map(|entry| match entry {
                Entry::Fact { name, .. } => Some(name),
                _ => None,
            }),
            usize::MAX,
        );
        let marks = theme.glyphs.marks;
        let title = (theme.keys, theme.truncated_dialog);
        let body = (theme.dialog, theme.truncated_dialog);
        entries
            .iter()
            .flat_map(|entry| match entry {
                Entry::Heading(heading) => vec![heading_line(heading, width, theme)],
                Entry::Keys { keys, text } => {
                    let room = width.saturating_sub(keys_width + 1);
                    let text = cells::fit(text, room, Align::Left, body, marks);
                    key_lines(keys, keys_width)
                        .into_iter()
                        .zip(std::iter::once(text).chain(std::iter::repeat(Vec::new())))
                        .map(|(keys, text)| {
                            let mut spans =
                                cells::fit(&keys, keys_width, Align::Left, title, marks);
                            spans.push(Span::raw(" "));
                            spans.extend(text);
                            Line::from(spans)
                        })
                        .collect()
                }
                Entry::Fact { name, value } => {
                    let room = width.saturating_sub(names_width + 1);
                    let mut spans = cells::fit(name, names_width, Align::Left, title, marks);
                    spans.push(Span::raw(" "));
                    spans.extend(cells::fit(value, room, Align::Left, body, marks));
                    vec![Line::from(spans)]
                }
                Entry::Note(text) => cells::wrap(text, width)
                    .into_iter()
                    .map(Line::raw)
                    .collect(),
                Entry::Point(text) => point_lines(text, width, theme),
                Entry::Blank => vec![Line::default()],
            })
            .collect()
    }

    /// How far down the text is, in the frame below `inner`, on the right: the last row of text
    /// on screen in percent, or that the end shows.
    fn render_position(&self, frame: &mut Frame<'_>, inner: Rect, theme: &Theme) {
        let bottom = self.offset + self.page;
        let position = if bottom >= self.rows {
            fl!("help-position-end")
        } else {
            let percent = (bottom * 100).checked_div(self.rows).unwrap_or(100);
            fl!("help-position", percent = percent)
        };
        let position = format!(" {position} ");
        let width = u16::try_from(cells::width(&position)).unwrap_or(u16::MAX);
        if width > inner.width || inner.bottom() >= frame.area().bottom() {
            return;
        }
        let x = inner.right() - width;
        let line = Line::styled(position, theme.dialog);
        frame.render_widget(line, Rect::new(x, inner.bottom(), width, 1));
    }
}

/// The lines of a point of a tip in `width` cells: a bullet in the color of keys, and the text
/// wrapped beside it, its lines after the first indented as far.
fn point_lines(text: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    const INDENT: usize = 2;
    let bullet = format!("{:<INDENT$}", theme.glyphs.bullet);
    cells::wrap(text, width.saturating_sub(INDENT).max(1))
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let lead = if index == 0 {
                Span::styled(bullet.clone(), theme.keys)
            } else {
                Span::raw(" ".repeat(INDENT))
            };
            Line::from(vec![lead, Span::raw(line)])
        })
        .collect()
}

/// `title` on a line across `width` cells, as `── Panels ──────`: the line as the frame is, the
/// title in bold.
fn heading_line(title: &str, width: usize, theme: &Theme) -> Line<'static> {
    let styles = (theme.dialog_title.bold(), theme.truncated_dialog);
    let marks = theme.glyphs.marks;
    let lead = format!("{0}{0} ", glyphs::LINE);
    // The lead, and a cell after the title.
    let room = width.saturating_sub(cells::width(&lead) + 1);
    if room == 0 {
        return Line::from(cells::fit(title, width, Align::Left, styles, marks));
    }
    let title = cells::fit(
        title,
        cells::width(title).min(room),
        Align::Left,
        styles,
        marks,
    );
    let used = cells::width(&lead) + title.iter().map(Span::width).sum::<usize>();
    let rest = width.saturating_sub(used + 1);
    let mut spans = vec![Span::styled(lead, theme.dialog)];
    spans.extend(title);
    spans.push(Span::styled(
        format!(" {}", glyphs::LINE.repeat(rest)),
        theme.dialog,
    ));
    Line::from(spans)
}

/// `percent` of `cells`, rounded down.
fn percent(cells: u16, percent: u16) -> u16 {
    u16::try_from(u32::from(cells) * u32::from(percent) / 100).unwrap_or(cells)
}

/// `keys`, a list such as `Space, Shift+Down`, in lines of at most `width` cells, broken after
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
        (Mode::Visual, Context::Panel, Action::SystemOpen) => fl!("help-visual-system-open"),
        (Mode::Visual, Context::CommandLine, Action::Up) => fl!("help-command-visual-up"),
        (Mode::Visual, Context::CommandLine, Action::Down) => fl!("help-command-visual-down"),
        _ => return describe(context, action),
    };
    Some(text)
}

/// What `action` does in `context`; `None` for what the app cannot do yet, which the help
/// and the key hints leave out.
pub(crate) fn describe(context: Context, action: Action) -> Option<String> {
    if let Some(text) = describe_find(context, action)
        .or_else(|| describe_rows(context, action))
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
        (Context::SearchInput, Action::SearchMode) => fl!("help-search-mode"),
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
        (Context::KeyHints, Action::Confirm) => fl!("help-key-hints-run"),
        (Context::KeyHints, Action::Cancel) => fl!("help-key-hints-close"),
        (Context::Rename, Action::Confirm) => fl!("help-rename-confirm"),
        (Context::Rename, Action::Cancel) => fl!("help-rename-cancel"),
        (Context::Viewer, Action::Home) => fl!("help-viewer-top"),
        (Context::Viewer, Action::End) => fl!("help-viewer-end"),
        (Context::Viewer, Action::Left) => fl!("help-viewer-left"),
        (Context::Viewer, Action::Right) => fl!("help-viewer-right"),
        (Context::Viewer, Action::ScrollLeft) => fl!("help-viewer-scroll-left"),
        (Context::Viewer, Action::ScrollRight) => fl!("help-viewer-scroll-right"),
        (Context::Viewer, Action::ScrollHalfLeft) => fl!("help-viewer-scroll-half-left"),
        (Context::Viewer, Action::ScrollHalfRight) => fl!("help-viewer-scroll-half-right"),
        (Context::Viewer, Action::ScrollStart) => fl!("help-viewer-scroll-start"),
        (Context::Viewer, Action::ScrollEnd) => fl!("help-viewer-scroll-end"),
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
        Action::ScrollLeft => fl!("help-names-left"),
        Action::ScrollRight => fl!("help-names-right"),
        Action::ScrollHalfLeft => fl!("help-names-half-left"),
        Action::ScrollHalfRight => fl!("help-names-half-right"),
        Action::ScrollStart => fl!("help-names-start"),
        Action::ScrollEnd => fl!("help-names-end"),
        Action::ShowFull => fl!("help-show-full"),
        Action::InvertMarks => fl!("help-invert-marks"),
        Action::Select => fl!("help-select"),
        Action::Unselect => fl!("help-unselect"),
        Action::Parent => fl!("help-parent"),
        Action::JumpBack => fl!("help-jump-back"),
        Action::JumpForward => fl!("help-jump-forward"),
        Action::VisualMode => fl!("help-visual"),
        Action::VisualLineMode => fl!("help-visual-line"),
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
        Action::QuickSearchBack => fl!("help-quick-search-back"),
        Action::NextMatch => fl!("help-match-next"),
        Action::PrevMatch => fl!("help-match-prev"),
        Action::FindFile => fl!("help-find-open"),
        Action::FindNames => fl!("help-find-names"),
        Action::Shell => fl!("help-shell"),
        Action::Command => fl!("help-command"),
        Action::CommandHistory => fl!("help-command-history"),
        Action::View => fl!("help-view"),
        Action::Edit => fl!("help-edit"),
        Action::SystemOpen => fl!("help-system-open"),
        Action::Copy => fl!("help-copy"),
        Action::Duplicate => fl!("help-duplicate"),
        Action::Move => fl!("help-move"),
        Action::Rename => fl!("help-rename"),
        Action::Mkdir => fl!("help-mkdir"),
        Action::Delete => fl!("help-delete"),
        Action::Jobs => fl!("help-jobs"),
        Action::Checksum => fl!("help-checksum"),
        Action::LocationMenuLeft => fl!("help-menu-left"),
        Action::LocationMenuRight => fl!("help-menu-right"),
        Action::Jump => fl!("help-jump-open"),
        Action::GoToDir => fl!("help-goto"),
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

/// What keys do in the dialog of the search of a whole tree, besides those of dialogs.
fn describe_find(context: Context, action: Action) -> Option<String> {
    if !matches!(context, Context::Find | Context::FindInput) {
        return None;
    }
    let text = match action {
        Action::View => fl!("help-find-view"),
        Action::Edit => fl!("help-find-edit"),
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
        Action::PrevLine => fl!("help-field-prev-line"),
        Action::NextLine => fl!("help-field-next-line"),
        Action::ThisLine => fl!("help-field-this-line"),
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
        Context::Panel | Context::Viewer | Context::Help | Context::List | Context::Find
    );
    let text = match action {
        Action::Up if rows => fl!("help-row-up"),
        Action::Down if rows => fl!("help-row-down"),
        Action::PageUp if rows => fl!("help-page-up"),
        Action::PageDown if rows => fl!("help-page-down"),
        Action::WindowUp if rows => fl!("help-window-up"),
        Action::WindowDown if rows => fl!("help-window-down"),
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

/// What Tab does in path fields and on the command line, and the keys of the list of
/// completions.
fn describe_completion(context: Context, action: Action) -> Option<String> {
    let text = match (context, action) {
        (Context::PathInput, Action::Complete) => fl!("help-complete"),
        (Context::CommandLine, Action::Complete) => fl!("help-command-complete"),
        (Context::Completion, Action::Down) => fl!("help-complete-next"),
        (Context::Completion, Action::Up) => fl!("help-complete-prev"),
        (Context::Completion, Action::PageDown) => fl!("help-complete-page-down"),
        (Context::Completion, Action::PageUp) => fl!("help-complete-page-up"),
        (Context::Completion, Action::Left) => fl!("help-complete-parent"),
        (Context::Completion, Action::Right) => fl!("help-complete-into"),
        (Context::Completion, Action::AcceptCompletion) => fl!("help-complete-take"),
        (Context::Completion, Action::CancelCompletion) => fl!("help-complete-close"),
        _ => return None,
    };
    Some(text)
}

#[cfg(test)]
mod tests {
    use noc_scroll::Motion;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Position;

    use super::*;
    use crate::about::{self, Detected, Keyboard, System};
    use crate::tui::ColorDepth;

    fn action(action: Action) -> Resolved {
        Resolved::Action(action)
    }

    fn build() -> Build {
        Build {
            version: "1.2.3",
            features: &[("forwarding", false)],
            commit: "0123456789ab",
            dirty: "yes",
            commit_time: "2026-10-12T14:03:27Z",
            target: "aarch64-apple-darwin",
            profile: "release",
            license: "GPL-3.0-or-later",
            repository: "https://example.com/noc",
        }
    }

    fn help() -> Help {
        Help::new(&Keymap::default(), &About::new(build()))
    }

    /// The help on the group of keys `group`, with Keys open.
    fn on(group: usize) -> Help {
        help().at(Spot {
            row: Row::Group(group),
            group,
            ..Spot::default()
        })
    }

    fn draw(help: &mut Help, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| help.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    fn all_entries(help: &Help) -> impl Iterator<Item = &Entry> {
        help.groups.iter().flat_map(|group| &group.entries)
    }

    fn headings(entries: &[Entry]) -> Vec<&str> {
        entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Heading(title) => Some(title.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn lists_what_the_app_can_do_by_mode_and_context() {
        let help = help();
        let has = |keys: &str, text: &str| {
            all_entries(&help).any(|entry| {
                *entry
                    == Entry::Keys {
                        keys: keys.to_owned(),
                        text: text.to_owned(),
                    }
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
            "The next name that matches the last quick search, the way it went"
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
        // Tab completes in path fields and on the command line, and goes on in the list.
        assert!(has("Tab, Ctrl+i", &fl!("help-complete")));
        assert!(has("Tab, Ctrl+i", &fl!("help-command-complete")));
        assert!(has("Tab, Ctrl+i, Ctrl+n, Down", &fl!("help-complete-next")));
        assert!(has("Shift+Tab, Ctrl+p, Up", &fl!("help-complete-prev")));
        assert!(has("Ctrl+y", "Keep the match, and close the list"));
        assert!(has("Ctrl+e", &fl!("help-complete-close")));
        assert!(has("/, i", &fl!("help-list-filter")));
        // One key, three meanings, by mode and context.
        let ctrl_u: Vec<&str> = all_entries(&help)
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
                "Half a page up",
                "Half a page up"
            ]
        );
        // The help's own keys, which scroll it as less does.
        assert!(has("Space, f", "A whole page down, as less's Space and f"));
        let tips = &help.groups.last().unwrap().entries;
        assert_eq!(
            tips.last(),
            Some(&Entry::Point(fl!("help-tip-key-hints-list")))
        );
        assert!(tips.contains(&Entry::Heading(fl!("help-tip-search"))));
        assert!(tips.contains(&Entry::Point(fl!("help-tip-lists-jump"))));
    }

    #[test]
    fn tips_have_titles_and_points_under_them() {
        let help = help();
        let tips = &help.groups.last().unwrap().entries;
        assert_eq!(
            headings(tips),
            [
                "Modes",
                "Counts and registers",
                "Lists",
                "Search modes",
                "Workspaces",
                "Command line",
                "Pull-down menu",
                "Dialogs",
                "Key hints"
            ]
        );
        // Each title has points, and a blank line goes before the next.
        for (index, entry) in tips.iter().enumerate() {
            match entry {
                Entry::Heading(_) => assert!(matches!(tips[index + 1], Entry::Point(_))),
                Entry::Blank => assert!(matches!(tips[index + 1], Entry::Heading(_))),
                Entry::Point(_) => {}
                entry => panic!("{entry:?}"),
            }
        }
        let theme = Theme::terminal();
        let lines: Vec<String> = point_lines("one two three four", 9, &theme)
            .into_iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();
        assert_eq!(lines, ["- one two", "  three", "  four"]);
    }

    #[test]
    fn groups_the_keys_by_part_of_the_app() {
        let help = help();
        let groups: Vec<(&str, Vec<&str>)> = help
            .groups
            .iter()
            .map(|group| (group.title.text.as_str(), headings(&group.entries)))
            .collect();
        assert_eq!(
            groups,
            [
                ("Everywhere", vec![]),
                ("Panels", vec!["Panels", "Panels in Visual mode"]),
                (
                    "Text fields",
                    vec![
                        "Text fields in Insert mode",
                        "Text fields in Normal mode",
                        "Text fields in Visual mode",
                        "Text fields of dialogs",
                        "Path fields",
                        "Completion list"
                    ]
                ),
                (
                    "Search and renaming",
                    vec![
                        "Quick search",
                        "Every field that searches, while it has the focus",
                        "Search of a whole tree, in its fields",
                        "Search of a whole tree",
                        "Renaming in place",
                        "Renaming in place, in Normal mode"
                    ]
                ),
                (
                    "Command line",
                    vec![
                        "Command line",
                        "Command line in Normal mode",
                        "Command history"
                    ]
                ),
                (
                    "Lists and menus",
                    vec![
                        "Lists",
                        "The filter of a list",
                        "Location menu",
                        "Tab menu",
                        "Workspaces"
                    ]
                ),
                (
                    "Menus and dialogs",
                    vec!["Pull-down menu", "Dialogs and help", "This help"]
                ),
                ("Viewer", vec![]),
                (
                    "Tips",
                    vec![
                        "Modes",
                        "Counts and registers",
                        "Lists",
                        "Search modes",
                        "Workspaces",
                        "Command line",
                        "Pull-down menu",
                        "Dialogs",
                        "Key hints"
                    ]
                ),
            ]
        );
        assert!(help.groups.iter().all(|group| !group.entries.is_empty()));
    }

    #[test]
    fn every_section_has_a_letter_of_its_own() {
        let (texts, _) = help().sidebar_texts(&Theme::terminal());
        let letters: Vec<char> = texts.iter().filter_map(Label::letter).collect();
        assert_eq!(
            letters,
            ['a', 'k', 'e', 'p', 't', 's', 'c', 'l', 'm', 'v', 'i'],
            "one for each section, each its own"
        );
        let mut help = help();
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| help.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        let underlined: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.modifier.contains(ratatui::style::Modifier::UNDERLINED))
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert_eq!(underlined, "AKEPTSCLMVi", "drawn apart");
    }

    #[test]
    fn alt_and_a_letter_show_its_section_with_the_keys_in_its_text() {
        let mut help = help();
        help.handle(action(Action::NextField));
        assert_eq!(help.focus, Focus::Sidebar);
        assert!(!help.handle(Resolved::Hotkey('v')));
        assert_eq!(
            (help.row, help.shown(), help.focus),
            (Row::Group(7), Some(7), Focus::Text)
        );
        help.handle(action(Action::Left));
        help.handle(action(Action::Left));
        help.handle(action(Action::Left));
        assert!(!help.keys_open, "folded");
        help.handle(Resolved::Hotkey('I'));
        assert_eq!(
            (help.row, help.keys_open),
            (Row::Group(8), true),
            "Keys opens"
        );
        help.handle(Resolved::Hotkey('a'));
        assert_eq!((help.row, help.shown()), (Row::About, None));
        help.handle(Resolved::Hotkey('k'));
        assert_eq!(
            (help.row, help.shown()),
            (Row::Keys, Some(8)),
            "the group of before"
        );
        help.handle(Resolved::Hotkey('z'));
        assert_eq!(
            (help.row, help.focus),
            (Row::Keys, Focus::Text),
            "nothing has z"
        );
        assert!(!draw(&mut help, 120, 30).contains('&'));
    }

    #[test]
    fn opens_at_the_top_of_about_and_then_where_it_was_left() {
        let first = help();
        assert_eq!((first.row, first.shown()), (Row::About, None));
        assert_eq!(first.focus, Focus::Text);
        assert_eq!(first.spot(), Spot::default());
        let mut help = on(7);
        draw(&mut help, 120, 30);
        help.handle(action(Action::PageDown));
        help.handle(action(Action::NextField));
        let spot = help.spot();
        let mut again = self::help().at(spot);
        assert_eq!(
            (again.row, again.shown(), again.focus, again.offset),
            (Row::Group(7), Some(7), Focus::Sidebar, help.offset)
        );
        assert_eq!(draw(&mut again, 120, 30), draw(&mut help, 120, 30));
    }

    #[test]
    fn keys_stay_folded_and_show_the_group_chosen_last() {
        let mut help = on(7);
        help.handle(action(Action::NextField));
        help.handle(action(Action::Left));
        help.handle(action(Action::Left));
        let mut help = self::help().at(help.spot());
        assert_eq!(help.row, Row::Keys);
        assert_eq!(help.shown(), Some(7));
        assert_eq!(help.sidebar_rows(), [Row::About, Row::Keys]);
        let screen = draw(&mut help, 120, 30);
        assert!(!screen.contains("Panels"), "{screen}");
        assert!(screen.contains("Keys ▸"), "{screen}");
        assert!(!help.keys_open);
    }

    #[test]
    fn the_sidebar_chooses_opens_and_folds_sections() {
        let mut help = on(1);
        assert_eq!(help.row, Row::Group(1));
        help.handle(action(Action::NextField));
        assert_eq!(help.focus, Focus::Sidebar);
        help.handle(action(Action::Down));
        assert_eq!((help.row, help.shown()), (Row::Group(2), Some(2)));
        // Left goes up to Keys, which shows the group chosen last; Left again folds it.
        help.handle(action(Action::Left));
        assert_eq!((help.row, help.shown()), (Row::Keys, Some(2)));
        help.handle(action(Action::Left));
        assert!(!help.keys_open);
        assert_eq!(help.sidebar_rows(), [Row::About, Row::Keys]);
        help.handle(action(Action::Up));
        assert_eq!((help.row, help.shown()), (Row::About, None));
        help.handle(action(Action::End));
        assert_eq!(help.row, Row::Keys);
        // Right opens Keys, and then gives the keys to the text.
        help.handle(action(Action::Right));
        assert!(help.keys_open);
        assert_eq!(help.focus, Focus::Sidebar);
        help.handle(action(Action::End));
        assert_eq!(help.row, Row::Group(8));
        assert!(!help.handle(action(Action::Confirm)));
        assert_eq!(help.focus, Focus::Text);
        // In the text, Left goes back to the sidebar, and Enter closes.
        help.handle(action(Action::Left));
        assert_eq!(help.focus, Focus::Sidebar);
        help.handle(action(Action::Home));
        assert_eq!(help.row, Row::About);
        help.handle(action(Action::NextField));
        assert!(help.handle(action(Action::Confirm)));
        assert!(help.handle(action(Action::Cancel)));
    }

    #[test]
    fn another_section_shows_from_its_top() {
        let mut help = on(1);
        draw(&mut help, 100, 20);
        help.handle(action(Action::PageDown));
        assert!(help.offset > 0);
        help.handle(action(Action::Left));
        help.handle(action(Action::Down));
        assert_eq!(help.offset, 0);
        // Keys shows the same group as its last one, where it was.
        help.handle(action(Action::Up));
        help.handle(action(Action::PageDown));
        let offset = help.offset;
        help.handle(action(Action::Left));
        assert_eq!((help.row, help.offset), (Row::Keys, offset));
    }

    #[test]
    fn scrolls_within_its_rows_and_closes() {
        let mut help = on(1);
        let top = draw(&mut help, 100, 20);
        assert!(top.contains("Panels"), "{top}");
        assert!(top.contains("% ") && top.contains('█'), "{top}");
        assert!(!help.handle(action(Action::PageDown)));
        assert_ne!(draw(&mut help, 100, 20), top);
        help.handle(action(Action::End));
        let bottom = draw(&mut help, 100, 20);
        assert!(bottom.contains("The right panel"), "{bottom}");
        assert!(
            !bottom.contains("EOF") && bottom.contains(" End "),
            "no line after the text, and the frame says it ends: {bottom}"
        );
        for key in [
            Action::Down,
            Action::PageDown,
            Action::HalfPageDown,
            Action::WindowDown,
        ] {
            help.handle(action(key));
            assert_eq!(draw(&mut help, 100, 20), bottom, "{key:?} stops at the end");
        }
        // The bar is at the bottom with the last line of text, just above the arrow down.
        let bar_at_bottom = |screen: &str| {
            let rows: Vec<&str> = screen.lines().collect();
            let frame = rows.iter().position(|row| row.contains('╚')).unwrap();
            let edge = |row: &str| row.trim_end_matches(['"', ' ']).chars().last();
            edge(rows[frame - 1]) == Some('▼') && edge(rows[frame - 2]) == Some('█')
        };
        assert!(bar_at_bottom(&bottom), "{bottom}");
        help.handle(action(Action::Up));
        let above = draw(&mut help, 100, 20);
        assert!(!above.contains(" End "), "{above}");
        assert!(!bar_at_bottom(&above), "{above}");
        help.handle(action(Action::Home));
        assert_eq!(draw(&mut help, 100, 20), top);
        assert!(help.handle(action(Action::Cancel)));
        assert!(help.handle(action(Action::Confirm)));
    }

    #[test]
    fn the_sidebar_scrolls_the_text_by_the_keys_it_does_not_take() {
        let mut help = on(1);
        draw(&mut help, 100, 20);
        help.handle(action(Action::NextField));
        help.handle(action(Action::WindowDown));
        assert!(help.offset > 0);
        assert_eq!(help.row, Row::Group(1));
        let offset = help.offset;
        help.wheel(Action::Down);
        assert_eq!(help.offset, offset + 1, "the wheel scrolls the text");
        assert_eq!(help.row, Row::Group(1));
    }

    #[test]
    fn scrolls_as_its_scroll_bar_is_pressed_and_dragged() {
        let mut help = on(1);
        let top = draw(&mut help, 100, 20);
        let bar = help.bar().unwrap();
        help.scroll(Scroll::By(Motion::PageDown));
        assert_eq!(help.offset, help.page - 2, "the page less vim's two rows");
        help.scroll(Scroll::To(bar.dragged(0, u16::MAX)));
        assert_eq!(
            help.offset,
            help.rows - help.page,
            "the last line of text at the bottom"
        );
        help.scroll(Scroll::To(usize::MAX));
        draw(&mut help, 100, 20);
        assert_eq!(
            help.offset,
            help.rows - help.page,
            "no further than the end"
        );
        help.scroll(Scroll::To(0));
        assert_eq!(draw(&mut help, 100, 20), top);
    }

    #[test]
    fn clicks_choose_sections_and_fold_keys() {
        let mut help = on(1);
        let screen = draw(&mut help, 120, 30);
        let at = |text: &str| {
            let (y, row) = screen
                .lines()
                .enumerate()
                .find(|(_, row)| row.contains(text))
                .unwrap();
            let x = row[..row.find(text).unwrap()].chars().count() - 1;
            Position::new(u16::try_from(x).unwrap(), u16::try_from(y).unwrap())
        };
        let click = |at| Pointer {
            press: Press::Click,
            at,
        };
        help.pointer(click(at(" Viewer ")));
        assert_eq!((help.row, help.focus), (Row::Group(7), Focus::Sidebar));
        help.pointer(click(at(" About ")));
        assert_eq!(help.row, Row::About);
        help.pointer(click(at(" Keys ")));
        assert_eq!(help.row, Row::Keys);
        assert!(!help.keys_open);
        assert_eq!(help.shown(), Some(7));
        help.pointer(click(Position::new(80, 15)));
        assert_eq!(help.focus, Focus::Text);
        draw(&mut help, 120, 30);
        help.pointer(click(at(" Keys ")));
        assert!(help.keys_open);
    }

    #[test]
    fn takes_a_share_of_the_screen() {
        let mut help = on(1);
        let screen = draw(&mut help, 160, 40);
        let rows: Vec<Vec<char>> = screen
            .lines()
            .map(|row| row.trim_matches('"').chars().collect())
            .collect();
        let top = rows.iter().position(|row| row.contains(&'╔')).unwrap();
        let bottom = rows.iter().position(|row| row.contains(&'╚')).unwrap();
        let left = rows[top].iter().position(|c| *c == '╔').unwrap();
        let right = rows[top].iter().position(|c| *c == '╗').unwrap();
        // 70% of 160 columns and of 40 rows.
        assert_eq!(right - left + 1, 112, "{screen}");
        assert_eq!(bottom - top + 1, 28, "{screen}");
        assert!(
            screen.contains("Open the directory under the cursor"),
            "{screen}"
        );
    }

    #[test]
    fn titles_are_lines_across_and_keys_have_a_color_of_their_own() {
        let theme = Theme::terminal();
        let text = |line: Line<'_>| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        };
        let line = heading_line("Panels", 20, &theme);
        assert_eq!(text(line.clone()), "── Panels ──────────");
        assert!(
            line.spans[1]
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        assert_eq!(
            text(heading_line("Panels in Visual mode", 12, &theme)),
            "── Pane»ode "
        );
        assert_eq!(text(heading_line("Panels", 4, &theme)), "Pa»s");

        let theme = Theme::mc_classic();
        let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
        let mut help = on(1);
        terminal
            .draw(|frame| help.render(frame, frame.area(), &theme))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let at = super::super::mouse::find(buffer, "Ctrl+p");
        assert_eq!(buffer[(at.x, at.y)].fg, theme.keys.fg.unwrap());
        let at = super::super::mouse::find(buffer, "Panels ─");
        assert_eq!(buffer[(at.x, at.y)].fg, theme.dialog_title.fg.unwrap());
        assert_ne!(theme.keys.fg, theme.dialog_title.fg);
    }

    #[test]
    fn draws_keys_and_what_they_do() {
        let mut help = on(1);
        insta::assert_snapshot!(draw(&mut help, 120, 26));
    }

    #[test]
    fn draws_the_sidebar_with_the_keys_and_keys_folded() {
        let mut help = on(7);
        help.handle(action(Action::NextField));
        help.handle(action(Action::Left));
        help.handle(action(Action::Left));
        insta::assert_snapshot!(draw(&mut help, 120, 26));
    }

    #[test]
    fn draws_the_tips() {
        let mut help = on(8);
        insta::assert_snapshot!(draw(&mut help, 120, 40));
    }

    #[test]
    fn draws_what_the_app_is_and_where_it_keeps_its_files() {
        let report = Report {
            build: build(),
            system: System {
                os: Some("macOS 26.0".to_owned()),
                kernel: Some("Darwin 25.0.0".to_owned()),
                arch: None,
            },
            terminal: about::Terminal {
                program: Some("kitty 0.35.2".to_owned()),
                term: Some("xterm-kitty".to_owned()),
                multiplexer: "tmux",
                colors: ColorDepth::TrueColor,
                keyboard: Keyboard::Kitty,
            },
            editor: Some("nvim -p".to_owned()),
            zoxide: Detected::Missing,
            ripgrep: Detected::Found("ripgrep 15.2.0".to_owned()),
            fd: Detected::Found("fd 10.5.0".to_owned()),
            language: Some("en-US".to_owned()),
            formats: Some("iso".to_owned()),
        };
        let about = About {
            report,
            files: Some(Files {
                home: PathBuf::from("/home/me"),
                config: PathBuf::from("/home/me/.config/noc/config.toml"),
                hosts: PathBuf::from("/home/me/.config/noc/hosts.toml"),
                workspaces: PathBuf::from("/home/me/.local/share/noc/workspaces.toml"),
                history: PathBuf::from("/home/me/.local/state/noc/history.toml"),
                recent: PathBuf::from("/home/me/.local/state/noc/recent.toml"),
                log: PathBuf::from("/var/log/noc/noc.log"),
            }),
        };
        let mut help = Help::new(&Keymap::default(), &about);
        help.handle(action(Action::NextField));
        help.handle(action(Action::Home));
        insta::assert_snapshot!(draw(&mut help, 120, 68));
    }
}
