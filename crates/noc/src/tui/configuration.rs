//! The Configuration dialog (Options → Configuration…): the settings of `config.toml`, by
//! category. Categories are listed on the left, each with an icon; the settings of the chosen
//! one are on the right and scroll, with a scroll bar, when they do not fit. There is no OK:
//! every change takes effect as it is made, a text field's when the cursor leaves it.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use noc_config::{Borders, Config, MenuBar, TabBar, UiConfig, Wheel};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Colors, Field, draw_box, draw_separator};
use super::keymap::{Action, Context, Resolved};
use super::scrollbar;
use super::theme::Theme;
use crate::i18n::fl;

/// Widest and tallest the dialog gets, in cells, borders included.
const WIDTH: u16 = 76;
const HEIGHT: u16 = 20;
/// Cells between a setting's name and its value.
const LABEL_GAP: usize = 2;
/// Lines of the hint of the setting under the cursor.
const HINT_ROWS: u16 = 2;

/// Which setting of `config.toml` a row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Language,
    Theme,
    Borders,
    Icons,
    ShowHidden,
    TypeToSearch,
    FuzzySearch,
    Mouse,
    Wheel,
    MenuBar,
    TabBar,
    AtomicUpload,
    ParallelJobs,
    SshProgram,
    SshConfigFile,
    SshArgs,
    Multiplex,
    HideHosts,
    HideVolumes,
    ZoxideRecord,
    ZoxideProgram,
}

/// A value and how it is edited.
#[derive(Debug)]
enum Value {
    /// A check box.
    Toggle(bool),
    /// One of `options`, each its value in `config.toml` and its text.
    Choice {
        options: Vec<(String, String)>,
        chosen: usize,
    },
    /// A text field, and the text it had when it was last applied; lists are words in it,
    /// see [`split_words`].
    Text { field: Field, applied: String },
}

#[derive(Debug)]
struct Setting {
    key: Key,
    label: String,
    /// What it does, below the settings while it has the cursor.
    hint: String,
    value: Value,
    /// Takes effect after a restart.
    restart: bool,
}

impl Setting {
    fn new(key: Key, (label, hint): (String, String), value: Value) -> Self {
        Self {
            key,
            label,
            hint,
            value,
            restart: false,
        }
    }

    fn toggle(&mut self) {
        match &mut self.value {
            Value::Toggle(on) => *on = !*on,
            Value::Choice { options, chosen } => *chosen = (*chosen + 1) % options.len().max(1),
            Value::Text { .. } => {}
        }
    }

    /// Picks the next choice, or the one before.
    fn cycle(&mut self, forward: bool) -> bool {
        let Value::Choice { options, chosen } = &mut self.value else {
            return false;
        };
        let count = options.len().max(1);
        *chosen = if forward {
            (*chosen + 1) % count
        } else {
            (*chosen + count - 1) % count
        };
        true
    }

    fn on(&self) -> bool {
        matches!(self.value, Value::Toggle(true))
    }

    /// The value in `config.toml` of a choice or a text.
    fn text(&self) -> &str {
        match &self.value {
            Value::Choice { options, chosen } => {
                options.get(*chosen).map_or("", |(value, _)| value.as_str())
            }
            Value::Text { field, .. } => field.text(),
            Value::Toggle(_) => "",
        }
    }

    /// Writes the value into `config`, or says why it is not valid.
    fn apply(&self, config: &mut Config) -> Result<(), String> {
        let text = self.text();
        let (ui, transfer, ssh) = (&mut config.ui, &mut config.transfer, &mut config.ssh);
        match self.key {
            Key::Language => {
                let language = text.trim();
                if !crate::i18n::is_valid_language(language) {
                    let text = cells::sanitize(language.as_bytes());
                    return Err(fl!("config-language-invalid", text = text));
                }
                language.clone_into(&mut ui.language);
            }
            Key::Theme => text.clone_into(&mut ui.theme),
            Key::Borders => {
                ui.borders = match text {
                    "single" => Borders::Single,
                    _ => Borders::Double,
                };
            }
            Key::Icons => ui.icons = self.on(),
            Key::ShowHidden => ui.show_hidden = self.on(),
            Key::TypeToSearch => ui.type_to_search = self.on(),
            Key::FuzzySearch => ui.fuzzy_search = self.on(),
            Key::Mouse => ui.mouse = self.on(),
            Key::Wheel => {
                ui.wheel = Wheel::parse(text.trim()).ok_or_else(|| {
                    let text = cells::sanitize(text.as_bytes());
                    fl!("config-wheel-invalid", text = text, most = Wheel::MAX_LINES)
                })?;
            }
            Key::MenuBar => {
                ui.menu_bar = match text {
                    "always" => MenuBar::Always,
                    _ => MenuBar::OnDemand,
                };
            }
            Key::TabBar => {
                ui.tab_bar = match text {
                    "frame" => TabBar::Frame,
                    _ => TabBar::Line,
                };
            }
            Key::AtomicUpload => transfer.atomic_upload = self.on(),
            Key::ParallelJobs => {
                transfer.parallel_jobs = text.trim().parse::<NonZeroUsize>().map_err(|_| {
                    let text = cells::sanitize(text.as_bytes());
                    fl!("config-parallel-jobs-invalid", text = text)
                })?;
            }
            Key::SshProgram => {
                let program = text.trim();
                if program.is_empty() {
                    return Err(fl!("config-ssh-program-empty"));
                }
                ssh.program = PathBuf::from(program);
            }
            Key::SshConfigFile => {
                let file = text.trim();
                ssh.config_file = (!file.is_empty()).then(|| PathBuf::from(file));
            }
            Key::SshArgs => {
                let args = split_words(text);
                noc_ssh::args::validate(&args).map_err(|error| {
                    let reason = cells::sanitize(error.to_string().as_bytes());
                    fl!("config-ssh-args-invalid", reason = reason)
                })?;
                ssh.args = args;
            }
            Key::Multiplex => ssh.multiplex = self.on(),
            Key::HideHosts => config.discovery.hide = split_words(text),
            Key::HideVolumes => config.volumes.hide = split_words(text),
            Key::ZoxideRecord => config.zoxide.record = self.on(),
            Key::ZoxideProgram => {
                let program = text.trim();
                if program.is_empty() {
                    return Err(fl!("config-zoxide-program-empty"));
                }
                config.zoxide.program = PathBuf::from(program);
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Category {
    /// A Nerd Font glyph.
    icon: &'static str,
    title: String,
    settings: Vec<Setting>,
}

/// Where the keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Sidebar,
    /// The setting under the cursor in the chosen category.
    Settings,
}

/// What a key did to the dialog.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ConfigEvent {
    /// The settings changed: from what they were to what they are, both as `config.toml`
    /// writes them, with `~` in paths.
    pub(crate) change: Option<(Config, Config)>,
    /// The dialog closes.
    pub(crate) closed: bool,
}

/// The Configuration dialog.
#[derive(Debug)]
pub(crate) struct Configuration {
    categories: Vec<Category>,
    /// The chosen category.
    category: usize,
    focus: Focus,
    /// The setting under the cursor, in the chosen category.
    row: usize,
    /// First row on screen, and rows on screen at the last render.
    offset: usize,
    page: usize,
    icons: bool,
    /// The settings as they were last applied, as `config.toml` writes them.
    applied: Config,
    /// Why the text field under the cursor cannot be applied, shown in place of its hint.
    error: Option<String>,
}

/// The values of a choice with their texts; `current` is chosen, and added if it is not
/// among them.
fn choice(values: &[(&str, String)], current: &str) -> Value {
    let mut options: Vec<(String, String)> = values
        .iter()
        .map(|(value, text)| ((*value).to_owned(), text.clone()))
        .collect();
    let chosen = options
        .iter()
        .position(|(value, _)| value == current)
        .unwrap_or_else(|| {
            options.push((current.to_owned(), current.to_owned()));
            options.len() - 1
        });
    Value::Choice { options, chosen }
}

fn text(value: &str) -> Value {
    Value::Text {
        field: Field::plain(value),
        applied: value.to_owned(),
    }
}

/// A path as the field shows it: under `home`, from `~`, as `config.toml` may write it.
fn path_text(path: &Path, home: &Path) -> Value {
    let shown = match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    };
    text(&shown)
}

/// The words of a list typed in a field: separated by spaces, with `"…"` around a word that
/// has spaces and `\` before a character to take it as it is, as a shell reads them.
fn split_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quoted = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    word.get_or_insert_with(String::new).push(next);
                }
            }
            '"' => {
                quoted = !quoted;
                word.get_or_insert_with(String::new);
            }
            c if c.is_whitespace() && !quoted => words.extend(word.take()),
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    words.extend(word);
    words
}

/// A list as [`split_words`] reads it back.
fn join_words(words: &[String]) -> String {
    words
        .iter()
        .map(|word| {
            let escaped = word.replace('\\', "\\\\").replace('"', "\\\"");
            if word.is_empty() || word.chars().any(char::is_whitespace) {
                format!("\"{escaped}\"")
            } else {
                escaped
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Configuration {
    /// The dialog with the settings `config` has, which shows paths under `home` from `~`;
    /// `themes` are the themes to choose from, and `icons` puts the categories' icons in front
    /// of their names.
    pub(crate) fn new(config: &Config, home: &Path, themes: &[&str], icons: bool) -> Self {
        let mut dialog = Self {
            categories: vec![
                interface(config, themes),
                transfers(config),
                ssh(config, home),
                volumes(config),
                zoxide(config, home),
            ],
            category: 0,
            focus: Focus::Settings,
            row: 0,
            offset: 0,
            page: 1,
            icons,
            applied: config.clone(),
            error: None,
        };
        if let Ok(applied) = dialog.config() {
            dialog.applied = applied;
        }
        dialog
    }

    /// Applies the settings as the dialog has them: the change, if there is one, or, if a text
    /// field holds something invalid, `Err` with the cursor on it and the reason shown.
    fn apply(&mut self) -> Result<Option<(Config, Config)>, ()> {
        let config = match self.config() {
            Ok(config) => config,
            Err((category, row, message)) => {
                self.category = category;
                self.row = row;
                self.focus = Focus::Settings;
                self.error = Some(message);
                return Err(());
            }
        };
        self.error = None;
        for setting in self
            .categories
            .iter_mut()
            .flat_map(|category| &mut category.settings)
        {
            if let Value::Text { field, applied } = &mut setting.value {
                field.text().clone_into(applied);
            }
        }
        if config == self.applied {
            return Ok(None);
        }
        let old = std::mem::replace(&mut self.applied, config.clone());
        Ok(Some((old, config)))
    }

    /// Puts back what the text field under the cursor had when it was last applied.
    fn revert(&mut self) {
        self.error = None;
        if let Some(Setting {
            value: Value::Text { field, applied },
            ..
        }) = self.setting_mut()
        {
            *field = Field::plain(applied);
        }
    }

    /// The settings as the dialog has them, or the category and row of one that is not valid,
    /// and why.
    fn config(&self) -> Result<Config, (usize, usize, String)> {
        let mut config = self.applied.clone();
        for (index, category) in self.categories.iter().enumerate() {
            for (row, setting) in category.settings.iter().enumerate() {
                setting
                    .apply(&mut config)
                    .map_err(|message| (index, row, message))?;
            }
        }
        Ok(config)
    }

    fn settings(&self) -> &[Setting] {
        &self.categories[self.category].settings
    }

    fn setting_mut(&mut self) -> Option<&mut Setting> {
        let row = self.row;
        self.categories[self.category].settings.get_mut(row)
    }

    /// The text field under the cursor, if the settings have the focus.
    fn field_mut(&mut self) -> Option<&mut Field> {
        if self.focus != Focus::Settings {
            return None;
        }
        match &mut self.setting_mut()?.value {
            Value::Text { field, .. } => Some(field),
            Value::Toggle(_) | Value::Choice { .. } => None,
        }
    }

    /// The keymap context for the next key: `DialogInput` in a text field.
    pub(crate) fn context(&self) -> Context {
        let text = self.focus == Focus::Settings
            && matches!(
                self.settings().get(self.row).map(|setting| &setting.value),
                Some(Value::Text { .. })
            );
        if text {
            Context::DialogInput
        } else {
            Context::Dialog
        }
    }

    fn choose_category(&mut self, index: usize) {
        if index != self.category {
            self.category = index;
            self.row = 0;
            self.offset = 0;
        }
    }

    /// Takes a key. Tab moves between the categories and the settings; Up and Down move
    /// within them; Space or Enter switches a check box or picks the next choice, and Left and
    /// Right pick choices, each applied at once. A text field is applied when the cursor leaves
    /// it or on Enter, and keeps the cursor while it holds something invalid. Esc closes the
    /// dialog, applying the text field under the cursor, or putting it back if invalid.
    pub(crate) fn handle(&mut self, input: Resolved) -> ConfigEvent {
        let mut event = ConfigEvent::default();
        let in_text = self.field_mut().is_some();
        let action = match (input, self.field_mut()) {
            (Resolved::Insert(c), Some(field)) => {
                field.insert(c);
                return event;
            }
            (Resolved::Insert(_), None) => return event,
            (Resolved::Action(action), Some(field)) => {
                if field.edit(action) {
                    return event;
                }
                action
            }
            (Resolved::Action(action), None) => action,
        };
        if action == Action::Cancel {
            if in_text {
                match self.apply() {
                    Ok(change) => event.change = change,
                    Err(()) => self.revert(),
                }
            }
            event.closed = true;
            return event;
        }
        let leaves = matches!(
            action,
            Action::Up
                | Action::Down
                | Action::PageUp
                | Action::PageDown
                | Action::NextField
                | Action::PrevField
                | Action::Confirm
        );
        if in_text && leaves {
            match self.apply() {
                Ok(change) => event.change = change,
                Err(()) => return event,
            }
            if action == Action::Confirm {
                return event;
            }
        }
        let rows = self.settings().len();
        let last = rows.saturating_sub(1);
        let page = self.page.max(1);
        match (self.focus, action) {
            (
                Focus::Sidebar,
                Action::NextField | Action::Right | Action::Toggle | Action::Confirm,
            ) if rows > 0 => {
                self.focus = Focus::Settings;
            }
            (Focus::Settings, Action::NextField | Action::PrevField)
            | (Focus::Sidebar, Action::PrevField) => {
                self.focus = match self.focus {
                    Focus::Sidebar if rows > 0 => Focus::Settings,
                    Focus::Sidebar | Focus::Settings => Focus::Sidebar,
                };
            }
            (Focus::Sidebar, Action::Up) => self.choose_category(self.category.saturating_sub(1)),
            (Focus::Sidebar, Action::Down) => {
                let last = self.categories.len() - 1;
                self.choose_category((self.category + 1).min(last));
            }
            (Focus::Sidebar, Action::Home | Action::PageUp) => self.choose_category(0),
            (Focus::Sidebar, Action::End | Action::PageDown) => {
                self.choose_category(self.categories.len() - 1);
            }
            (Focus::Settings, Action::Up) => self.row = self.row.saturating_sub(1),
            (Focus::Settings, Action::Down) => self.row = (self.row + 1).min(last),
            (Focus::Settings, Action::PageUp) => self.row = self.row.saturating_sub(page),
            (Focus::Settings, Action::PageDown) => self.row = (self.row + page).min(last),
            (Focus::Settings, Action::Home) => self.row = 0,
            (Focus::Settings, Action::End) => self.row = last,
            (Focus::Settings, Action::Toggle | Action::Confirm) => {
                if let Some(setting) = self.setting_mut() {
                    setting.toggle();
                }
                event.change = self.apply().unwrap_or_default();
            }
            (Focus::Settings, Action::Left | Action::Right) => {
                let forward = action == Action::Right;
                let cycled = self
                    .setting_mut()
                    .is_some_and(|setting| setting.cycle(forward));
                if cycled {
                    event.change = self.apply().unwrap_or_default();
                } else if !forward {
                    self.focus = Focus::Sidebar;
                }
            }
            _ => {}
        }
        event
    }

    /// Draws the dialog centered in `area`, with the terminal cursor in a focused text field.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let colors = Colors::of(theme, false);
        let size = (
            WIDTH.min(area.width.saturating_sub(4)),
            HEIGHT.min(area.height.saturating_sub(2)),
        );
        let inner = draw_box(frame, area, size, &fl!("config-title"), colors, theme);
        // The body, a line, and two lines of hint.
        if inner.height < HINT_ROWS + 2 || inner.width < 20 {
            return;
        }
        let body = Rect {
            height: inner.height - HINT_ROWS - 1,
            ..inner
        };
        let sidebar = self.render_sidebar(frame, body, theme, colors);
        let x = sidebar.right();
        for y in body.y..body.bottom() {
            frame.render_widget(Line::styled("│", theme.dialog), Rect::new(x, y, 1, 1));
        }
        let content = Rect::new(
            x + 2,
            body.y,
            body.right().saturating_sub(x + 2),
            body.height,
        );
        self.render_settings(frame, content, theme, colors);
        draw_separator(frame, inner, body.bottom(), colors, theme);
        // The line between the categories and the settings meets it.
        frame.render_widget(
            Line::styled("┴", theme.dialog),
            Rect::new(x, body.bottom(), 1, 1),
        );

        let (hint, style) = match (&self.error, self.settings().get(self.row)) {
            (Some(error), _) => (error.clone(), theme.error_dialog),
            (None, Some(setting)) if self.focus == Focus::Settings && setting.restart => (
                format!("{} {}", setting.hint, fl!("config-restart")),
                theme.dialog.patch(theme.menu_disabled),
            ),
            (None, Some(setting)) if self.focus == Focus::Settings => (
                setting.hint.clone(),
                theme.dialog.patch(theme.menu_disabled),
            ),
            _ => (String::new(), theme.dialog),
        };
        let lines = cells::wrap(&hint, usize::from(inner.width));
        for (row, line) in (0..HINT_ROWS).zip(lines) {
            let line = cells::fit(&line, usize::from(inner.width), Align::Left);
            let area = Rect::new(inner.x, body.bottom() + 1 + row, inner.width, 1);
            frame.render_widget(Line::styled(line, style), area);
        }
    }

    /// The categories down the left of `body`; returns where they are.
    fn render_sidebar(
        &self,
        frame: &mut Frame<'_>,
        body: Rect,
        theme: &Theme,
        colors: Colors,
    ) -> Rect {
        let names: Vec<String> = self
            .categories
            .iter()
            .map(|category| {
                if self.icons {
                    format!(" {} {} ", category.icon, category.title)
                } else {
                    format!(" {} ", category.title)
                }
            })
            .collect();
        let width = names
            .iter()
            .map(|name| cells::width(name))
            .max()
            .unwrap_or(0);
        let width = u16::try_from(width).unwrap_or(u16::MAX).min(body.width / 3);
        let area = Rect { width, ..body };
        for (index, name) in names.iter().enumerate() {
            let Ok(y) = u16::try_from(index) else { break };
            if y >= area.height {
                break;
            }
            let style = match (index == self.category, self.focus == Focus::Sidebar) {
                (true, true) => colors.focused_style(),
                (true, false) => theme.dialog_title.bold(),
                (false, _) => theme.dialog,
            };
            let text = cells::fit(name, usize::from(width), Align::Left);
            frame.render_widget(
                Line::styled(text, style),
                Rect::new(area.x, area.y + y, width, 1),
            );
        }
        area
    }

    /// The settings of the chosen category in `area`, scrolled to the cursor, with a scroll
    /// bar in its last column when they do not fit.
    fn render_settings(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        colors: Colors,
    ) {
        let page = usize::from(area.height).max(1);
        let total = self.settings().len();
        self.row = self.row.min(total.saturating_sub(1));
        self.offset = self
            .offset
            .min(self.row)
            .max((self.row + 1).saturating_sub(page))
            .min(total.saturating_sub(page));
        self.page = page;
        let bar = Rect::new(area.right().saturating_sub(1), area.y, 1, area.height);
        scrollbar::render(frame, bar, (total, page, self.offset), theme.dialog);
        let room = usize::from(area.width.saturating_sub(2));
        let label_width = self
            .settings()
            .iter()
            .map(|setting| cells::width(&setting.label))
            .max()
            .unwrap_or(0)
            .min(room / 2);
        let value_width = room.saturating_sub(label_width + LABEL_GAP);
        let mut cursor = None;
        for (index, setting) in self
            .settings()
            .iter()
            .enumerate()
            .skip(self.offset)
            .take(page)
        {
            let y = area.y + u16::try_from(index - self.offset).unwrap_or(0);
            let focused = self.focus == Focus::Settings && index == self.row;
            let label = cells::fit(&setting.label, label_width + LABEL_GAP, Align::Left);
            let label_style = if focused && !matches!(setting.value, Value::Text { .. }) {
                colors.focused_style()
            } else {
                theme.dialog
            };
            let value = match &setting.value {
                Value::Toggle(on) => {
                    let mark = if *on { 'x' } else { ' ' };
                    Span::styled(format!("[{mark}]"), label_style)
                }
                Value::Choice { options, chosen } => {
                    let text = options.get(*chosen).map_or("", |(_, text)| text.as_str());
                    let text = cells::fit(text, value_width.saturating_sub(4), Align::Left);
                    Span::styled(format!("< {} >", text.trim_end()), label_style)
                }
                Value::Text { field, .. } => {
                    let (text, column) = field.visible(value_width);
                    if focused {
                        let label = u16::try_from(label_width + LABEL_GAP).unwrap_or(0);
                        let column = u16::try_from(column).unwrap_or(0);
                        cursor = Some(Position::new(area.x + label + column, y));
                    }
                    let style = if field.fresh() {
                        theme.dialog_input_fresh
                    } else {
                        theme.dialog_input
                    };
                    Span::styled(cells::fit(&text, value_width, Align::Left), style)
                }
            };
            let line =
                Line::from(vec![Span::styled(label, label_style), value]).style(theme.dialog);
            frame.render_widget(line, Rect::new(area.x, y, area.width.saturating_sub(2), 1));
        }
        if let Some(position) = cursor {
            frame.set_cursor_position(position);
        }
    }
}

/// Interface, `[ui]`.
fn interface(config: &Config, themes: &[&str]) -> Category {
    let ui = &config.ui;
    let themes: Vec<(&str, String)> = themes
        .iter()
        .map(|name| (*name, (*name).to_owned()))
        .collect();
    let borders = match ui.borders {
        Borders::Double => "double",
        Borders::Single => "single",
    };
    let mut settings = vec![
        Setting {
            restart: true,
            ..Setting::new(
                Key::Language,
                (fl!("config-language"), fl!("config-language-hint")),
                text(&ui.language),
            )
        },
        Setting::new(
            Key::Theme,
            (fl!("config-theme"), fl!("config-theme-hint")),
            choice(&themes, &ui.theme),
        ),
        Setting::new(
            Key::Borders,
            (fl!("config-borders"), fl!("config-borders-hint")),
            choice(
                &[
                    ("double", fl!("config-borders-double")),
                    ("single", fl!("config-borders-single")),
                ],
                borders,
            ),
        ),
        Setting::new(
            Key::Icons,
            (fl!("config-icons"), fl!("config-icons-hint")),
            Value::Toggle(ui.icons),
        ),
        Setting::new(
            Key::ShowHidden,
            (fl!("config-show-hidden"), fl!("config-show-hidden-hint")),
            Value::Toggle(ui.show_hidden),
        ),
        Setting::new(
            Key::TypeToSearch,
            (
                fl!("config-type-to-search"),
                fl!("config-type-to-search-hint"),
            ),
            Value::Toggle(ui.type_to_search),
        ),
        Setting::new(
            Key::FuzzySearch,
            (fl!("config-fuzzy-search"), fl!("config-fuzzy-search-hint")),
            Value::Toggle(ui.fuzzy_search),
        ),
        Setting::new(
            Key::Mouse,
            (fl!("config-mouse"), fl!("config-mouse-hint")),
            Value::Toggle(ui.mouse),
        ),
        Setting::new(
            Key::Wheel,
            (fl!("config-wheel"), fl!("config-wheel-hint")),
            text(&ui.wheel.to_string()),
        ),
    ];
    settings.extend(bars(ui));
    Category {
        icon: "󰍹",
        title: fl!("config-interface"),
        settings,
    }
}

/// The settings of the menu bar and the tab bar.
fn bars(ui: &UiConfig) -> [Setting; 2] {
    let menu_bar = match ui.menu_bar {
        MenuBar::OnDemand => "on-demand",
        MenuBar::Always => "always",
    };
    let tab_bar = match ui.tab_bar {
        TabBar::Line => "line",
        TabBar::Frame => "frame",
    };
    [
        Setting::new(
            Key::MenuBar,
            (fl!("config-menu-bar"), fl!("config-menu-bar-hint")),
            choice(
                &[
                    ("on-demand", fl!("config-menu-bar-on-demand")),
                    ("always", fl!("config-menu-bar-always")),
                ],
                menu_bar,
            ),
        ),
        Setting::new(
            Key::TabBar,
            (fl!("config-tab-bar"), fl!("config-tab-bar-hint")),
            choice(
                &[
                    ("line", fl!("config-tab-bar-line")),
                    ("frame", fl!("config-tab-bar-frame")),
                ],
                tab_bar,
            ),
        ),
    ]
}

/// Transfers, `[transfer]`.
fn transfers(config: &Config) -> Category {
    let transfer = &config.transfer;
    Category {
        icon: "󰓡",
        title: fl!("config-transfers"),
        settings: vec![
            Setting::new(
                Key::AtomicUpload,
                (
                    fl!("config-atomic-upload"),
                    fl!("config-atomic-upload-hint"),
                ),
                Value::Toggle(transfer.atomic_upload),
            ),
            Setting::new(
                Key::ParallelJobs,
                (
                    fl!("config-parallel-jobs"),
                    fl!("config-parallel-jobs-hint"),
                ),
                text(&transfer.parallel_jobs.to_string()),
            ),
        ],
    }
}

/// SSH, `[ssh]` and `[discovery]`: how ssh runs, and which hosts are listed.
fn ssh(config: &Config, home: &Path) -> Category {
    let ssh = &config.ssh;
    let config_file = ssh
        .config_file
        .as_deref()
        .map_or_else(|| text(""), |file| path_text(file, home));
    Category {
        icon: "󰣀",
        title: fl!("config-ssh"),
        settings: vec![
            Setting::new(
                Key::SshProgram,
                (fl!("config-ssh-program"), fl!("config-ssh-program-hint")),
                path_text(&ssh.program, home),
            ),
            Setting::new(
                Key::SshConfigFile,
                (
                    fl!("config-ssh-config-file"),
                    fl!("config-ssh-config-file-hint"),
                ),
                config_file,
            ),
            Setting::new(
                Key::SshArgs,
                (fl!("config-ssh-args"), fl!("config-ssh-args-hint")),
                text(&join_words(&ssh.args)),
            ),
            Setting::new(
                Key::Multiplex,
                (fl!("config-multiplex"), fl!("config-multiplex-hint")),
                Value::Toggle(ssh.multiplex),
            ),
            Setting::new(
                Key::HideHosts,
                (fl!("config-hide-hosts"), fl!("config-hide-hosts-hint")),
                text(&join_words(&config.discovery.hide)),
            ),
        ],
    }
}

/// Volumes, `[volumes]`.
fn volumes(config: &Config) -> Category {
    Category {
        icon: "󰋊",
        title: fl!("config-volumes"),
        settings: vec![Setting::new(
            Key::HideVolumes,
            (fl!("config-hide-volumes"), fl!("config-hide-volumes-hint")),
            text(&join_words(&config.volumes.hide)),
        )],
    }
}

/// zoxide, `[zoxide]`.
fn zoxide(config: &Config, home: &Path) -> Category {
    let zoxide = &config.zoxide;
    Category {
        icon: "󰥨",
        title: fl!("config-zoxide"),
        settings: vec![
            Setting::new(
                Key::ZoxideRecord,
                (
                    fl!("config-zoxide-record"),
                    fl!("config-zoxide-record-hint"),
                ),
                Value::Toggle(zoxide.record),
            ),
            Setting::new(
                Key::ZoxideProgram,
                (
                    fl!("config-zoxide-program"),
                    fl!("config-zoxide-program-hint"),
                ),
                path_text(&zoxide.program, home),
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    const THEMES: &[&str] = &["mc-classic", "terminal"];

    fn action(action: Action) -> Resolved {
        Resolved::Action(action)
    }

    const HOME: &str = "/home/me";

    fn dialog_of(config: &Config) -> Configuration {
        Configuration::new(config, Path::new(HOME), THEMES, false)
    }

    fn dialog() -> Configuration {
        dialog_of(&Config::default())
    }

    fn draw(dialog: &mut Configuration, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| dialog.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    fn typed(dialog: &mut Configuration, text: &str) {
        for c in text.chars() {
            dialog.handle(Resolved::Insert(c));
        }
    }

    /// Opens the category `index`, with the cursor on its first setting.
    fn category(dialog: &mut Configuration, index: usize) {
        if dialog.focus != Focus::Sidebar {
            dialog.handle(action(Action::PrevField));
        }
        assert_eq!(dialog.focus, Focus::Sidebar, "{:?}", dialog.error);
        dialog.handle(action(Action::Home));
        for _ in 0..index {
            dialog.handle(action(Action::Down));
        }
        dialog.handle(action(Action::Right));
    }

    #[test]
    fn shows_the_settings_as_the_file_writes_them() {
        let mut config = Config::default();
        config.ui.theme = "solarized".to_owned();
        config.ssh.program = PathBuf::from("/home/me/bin/ssh");
        config.ssh.args = vec!["-o".to_owned(), "ServerAliveInterval=15".to_owned()];
        config.volumes.hide = vec!["/Volumes/My Disk".to_owned()];
        let dialog = dialog_of(&config);
        let shown = dialog.config().unwrap();
        assert_eq!(shown, dialog.applied, "nothing changed");
        assert_eq!(shown.ui.theme, "solarized", "an unknown value is kept");
        assert_eq!(shown.ssh.program, Path::new("~/bin/ssh"), "paths from ~");
        assert_eq!(shown.ssh.args, config.ssh.args);
        assert_eq!(shown.volumes.hide, config.volumes.hide);
    }

    #[test]
    fn each_change_takes_effect_at_once() {
        let mut dialog = dialog();
        assert_eq!(dialog.context(), Context::DialogInput, "on the language");
        let event = dialog.handle(action(Action::Down));
        assert_eq!(event, ConfigEvent::default(), "the language did not change");
        let event = dialog.handle(action(Action::Right));
        let (old, new) = event.change.unwrap();
        assert_eq!(
            (old.ui.theme.as_str(), new.ui.theme.as_str()),
            ("mc-classic", "terminal")
        );
        dialog.handle(action(Action::Down));
        let (old, new) = dialog.handle(action(Action::Confirm)).change.unwrap();
        assert_eq!(old.ui.theme, "terminal", "the change before counts");
        assert_eq!(
            new.ui.borders,
            Borders::Single,
            "Enter picks the next choice"
        );

        // A text field takes effect when the cursor leaves it.
        category(&mut dialog, 1);
        dialog.handle(action(Action::Down));
        assert_eq!(dialog.handle(Resolved::Insert('4')), ConfigEvent::default());
        let (_, new) = dialog.handle(action(Action::Up)).change.unwrap();
        assert_eq!(new.transfer.parallel_jobs.get(), 4);
        // Or on Enter, which keeps the cursor there; or as the dialog closes.
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::DeleteToStart));
        typed(&mut dialog, "5");
        let event = dialog.handle(action(Action::Confirm));
        assert_eq!(event.change.unwrap().1.transfer.parallel_jobs.get(), 5);
        assert_eq!(dialog.row, 1);
        dialog.handle(action(Action::DeleteToStart));
        typed(&mut dialog, "3");
        let event = dialog.handle(action(Action::Cancel));
        assert!(event.closed);
        assert_eq!(event.change.unwrap().1.transfer.parallel_jobs.get(), 3);
    }

    #[test]
    fn reads_every_category() {
        let mut dialog = dialog();
        typed(&mut dialog, "de");
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::Right));
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::Toggle));
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::Toggle));
        for _ in 0..4 {
            dialog.handle(action(Action::Down));
        }
        dialog.handle(action(Action::Toggle));
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::DeleteToStart));
        typed(&mut dialog, "page");
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::End));
        dialog.handle(action(Action::Left));
        category(&mut dialog, 1);
        dialog.handle(action(Action::Toggle));
        dialog.handle(action(Action::Down));
        typed(&mut dialog, "4");
        category(&mut dialog, 2);
        dialog.handle(action(Action::Down));
        typed(&mut dialog, "~/.ssh/work");
        dialog.handle(action(Action::Down));
        typed(&mut dialog, "-o \"SetEnv A=b c\"");
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::Toggle));
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::DeleteToStart));
        category(&mut dialog, 3);
        typed(&mut dialog, "/mnt/*");
        category(&mut dialog, 4);
        dialog.handle(action(Action::Toggle));
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::DeleteToStart));
        typed(&mut dialog, "~/.cargo/bin/zoxide");
        assert!(dialog.handle(action(Action::Cancel)).closed);
        let mut expected = Config::default();
        expected.ui.language = "de".to_owned();
        expected.ui.theme = "terminal".to_owned();
        expected.ui.borders = Borders::Single;
        expected.ui.icons = false;
        expected.ui.tab_bar = TabBar::Frame;
        expected.ui.mouse = false;
        expected.ui.wheel = Wheel::Page;
        expected.transfer.atomic_upload = false;
        expected.transfer.parallel_jobs = NonZeroUsize::new(4).unwrap();
        expected.ssh.config_file = Some(PathBuf::from("~/.ssh/work"));
        expected.ssh.args = vec!["-o".to_owned(), "SetEnv A=b c".to_owned()];
        expected.ssh.multiplex = false;
        expected.discovery.hide = Vec::new();
        expected.volumes.hide = vec!["/mnt/*".to_owned()];
        expected.zoxide.record = false;
        expected.zoxide.program = PathBuf::from("~/.cargo/bin/zoxide");
        assert_eq!(dialog.applied, expected);
    }

    #[test]
    fn an_invalid_text_keeps_the_cursor_and_says_why() {
        let cases: [(usize, usize, &str, &str); 7] = [
            (0, 0, "?", "is not auto or a language tag"),
            (0, 8, "0", "is not a step of the wheel"),
            (0, 8, "pages", "is not a step of the wheel"),
            (1, 1, "x", "is not a number of jobs"),
            (2, 2, "-F other_config", "Invalid extra ssh arguments"),
            (2, 0, "", "The ssh program cannot be empty"),
            (4, 1, " ", "The zoxide program cannot be empty"),
        ];
        for (index, row, text, message) in cases {
            let mut dialog = dialog();
            category(&mut dialog, index);
            for _ in 0..row {
                dialog.handle(action(Action::Down));
            }
            let before = dialog.config().unwrap();
            dialog.handle(action(Action::DeleteToStart));
            dialog.handle(action(Action::DeleteToEnd));
            typed(&mut dialog, text);
            for key in [Action::Down, Action::NextField, Action::Confirm] {
                assert_eq!(
                    dialog.handle(action(key)),
                    ConfigEvent::default(),
                    "{message}"
                );
                assert_eq!((dialog.category, dialog.row), (index, row), "{message}");
                assert_eq!(dialog.focus, Focus::Settings);
            }
            let error = dialog.error.clone().unwrap();
            assert!(error.contains(message), "{error}");
            assert!(draw(&mut dialog, 76, 20).contains(message), "{message}");
            // Esc puts back what was there and closes.
            let event = dialog.handle(action(Action::Cancel));
            assert_eq!(event.change, None);
            assert!(event.closed);
            assert_eq!(dialog.config().unwrap(), before);
            assert_eq!(dialog.error, None);
        }
    }

    #[test]
    fn lists_are_words_as_a_shell_reads_them() {
        let words = |text: &str| split_words(text);
        assert_eq!(words("  a b\tc "), ["a", "b", "c"]);
        assert_eq!(words(r#"-o "SetEnv A=b c""#), ["-o", "SetEnv A=b c"]);
        assert_eq!(words(r#"a\ b \"q\" "" x\\"#), ["a b", "\"q\"", "", "x\\"]);
        assert_eq!(words(""), Vec::<String>::new());
        let list: Vec<String> = ["/Volumes/My Disk", "a\"b", "", "c\\d", "plain"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(
            join_words(&list),
            r#""/Volumes/My Disk" a\"b "" c\\d plain"#
        );
        assert_eq!(split_words(&join_words(&list)), list);
    }

    #[test]
    fn tab_moves_between_the_categories_and_the_settings() {
        let mut dialog = dialog();
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.focus, Focus::Sidebar);
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.focus, Focus::Settings);
        dialog.handle(action(Action::PrevField));
        assert_eq!(dialog.focus, Focus::Sidebar);
        dialog.handle(action(Action::PrevField));
        assert_eq!(dialog.focus, Focus::Settings);
        // Left on a check box goes back to the categories; in a text field it moves the
        // cursor.
        let mut dialog = self::dialog();
        dialog.handle(action(Action::Left));
        assert_eq!(dialog.focus, Focus::Settings);
        for _ in 0..3 {
            dialog.handle(action(Action::Down));
        }
        dialog.handle(action(Action::Left));
        assert_eq!(dialog.focus, Focus::Sidebar);
        assert!(dialog.handle(action(Action::Cancel)).closed);
    }

    #[test]
    fn scrolls_to_the_cursor_with_a_scroll_bar_when_the_settings_do_not_fit() {
        let mut dialog = dialog();
        let tall = draw(&mut dialog, 72, 24);
        assert!(!tall.contains('█'), "{tall}");
        // Home and End in the language field move its cursor.
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::End));
        let short = draw(&mut dialog, 72, 9);
        assert!(
            short.contains("Menu bar") && !short.contains("Language"),
            "{short}"
        );
        assert!(short.contains('█') && short.contains('░'), "{short}");
        dialog.handle(action(Action::Home));
        let top = draw(&mut dialog, 72, 9);
        assert!(
            top.contains("Language") && !top.contains("Menu bar"),
            "{top}"
        );
        assert!(top.contains("Takes effect after a restart"), "{top}");
    }

    #[test]
    fn draws_the_categories_and_the_settings() {
        let mut dialog = Configuration::new(&Config::default(), Path::new(HOME), THEMES, true);
        dialog.handle(action(Action::Down));
        insta::assert_snapshot!(draw(&mut dialog, 72, 16));
    }
}
