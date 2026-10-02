//! The Configuration dialog (Options → Configuration…): the settings of `config.toml`, by
//! category. Categories are listed on the left, each with an icon; the settings of the chosen
//! one are on the right and scroll, with a scroll bar, when they do not fit.

use noc_config::{Borders, MenuBar, UiConfig};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Button, Colors, Field, button_line, draw_box, draw_separator};
use super::keymap::{Action, Context, Resolved};
use super::scrollbar;
use super::theme::Theme;
use crate::i18n::fl;

/// Widest and tallest the dialog gets, in cells, borders included.
const WIDTH: u16 = 72;
const HEIGHT: u16 = 22;
/// Cells between a setting's name and its value.
const LABEL_GAP: usize = 2;
/// Lines of the hint of the setting under the cursor.
const HINT_ROWS: u16 = 2;
const BUTTONS: [Button; 2] = [Button::Ok, Button::Cancel];

/// Which setting of `config.toml` a row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Language,
    Theme,
    Borders,
    Icons,
    ShowHidden,
    TypeToSearch,
    MenuBar,
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
    Text(Field),
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
            Value::Text(_) => {}
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
            Value::Text(field) => field.text(),
            Value::Toggle(_) => "",
        }
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
    Button(usize),
}

/// What a key did to the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigEvent {
    Pending,
    /// OK: read the settings with [`Configuration::ui`].
    Accepted,
    Cancelled,
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

impl Configuration {
    /// The dialog with the settings `ui` has; `icons` puts the categories' icons in front of
    /// their names.
    pub(crate) fn new(ui: &UiConfig, themes: &[&str], icons: bool) -> Self {
        let themes: Vec<(&str, String)> = themes
            .iter()
            .map(|name| (*name, (*name).to_owned()))
            .collect();
        let borders = match ui.borders {
            Borders::Double => "double",
            Borders::Single => "single",
        };
        let menu_bar = match ui.menu_bar {
            MenuBar::OnDemand => "on-demand",
            MenuBar::Always => "always",
        };
        let interface = Category {
            icon: "󰍹",
            title: fl!("config-interface"),
            settings: vec![
                Setting {
                    restart: true,
                    ..Setting::new(
                        Key::Language,
                        (fl!("config-language"), fl!("config-language-hint")),
                        Value::Text(Field::plain(&ui.language)),
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
            ],
        };
        Self {
            categories: vec![interface],
            category: 0,
            focus: Focus::Settings,
            row: 0,
            offset: 0,
            page: 1,
            icons,
        }
    }

    /// `base` with the settings as the dialog has them.
    pub(crate) fn ui(&self, base: &UiConfig) -> UiConfig {
        let mut ui = base.clone();
        for setting in self
            .categories
            .iter()
            .flat_map(|category| &category.settings)
        {
            match setting.key {
                Key::Language => setting.text().trim().clone_into(&mut ui.language),
                Key::Theme => setting.text().clone_into(&mut ui.theme),
                Key::Borders => {
                    ui.borders = match setting.text() {
                        "single" => Borders::Single,
                        _ => Borders::Double,
                    };
                }
                Key::Icons => ui.icons = setting.on(),
                Key::ShowHidden => ui.show_hidden = setting.on(),
                Key::TypeToSearch => ui.type_to_search = setting.on(),
                Key::MenuBar => {
                    ui.menu_bar = match setting.text() {
                        "always" => MenuBar::Always,
                        _ => MenuBar::OnDemand,
                    };
                }
            }
        }
        ui
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
            Value::Text(field) => Some(field),
            Value::Toggle(_) | Value::Choice { .. } => None,
        }
    }

    /// The keymap context for the next key: `DialogInput` in a text field.
    pub(crate) fn context(&self) -> Context {
        let text = self.focus == Focus::Settings
            && matches!(
                self.settings().get(self.row).map(|setting| &setting.value),
                Some(Value::Text(_))
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

    /// Takes a key. Tab moves between the categories, the settings, and the buttons; Up and
    /// Down move within them; Space switches a check box or picks the next choice, and Left
    /// and Right pick choices; Enter presses OK, or the button with the focus; Esc cancels.
    pub(crate) fn handle(&mut self, input: Resolved) -> ConfigEvent {
        let action = match (input, self.field_mut()) {
            (Resolved::Insert(c), Some(field)) => {
                field.insert(c);
                return ConfigEvent::Pending;
            }
            (Resolved::Insert(_), None) => return ConfigEvent::Pending,
            (Resolved::Action(action), Some(field)) => {
                if field.edit(action) {
                    return ConfigEvent::Pending;
                }
                action
            }
            (Resolved::Action(action), None) => action,
        };
        let rows = self.settings().len();
        let last = rows.saturating_sub(1);
        let page = self.page.max(1);
        match (self.focus, action) {
            (Focus::Button(1), Action::Confirm | Action::Toggle) | (_, Action::Cancel) => {
                return ConfigEvent::Cancelled;
            }
            (_, Action::Confirm) | (Focus::Button(_), Action::Toggle) => {
                return ConfigEvent::Accepted;
            }
            (_, Action::NextField) => {
                self.focus = match self.focus {
                    Focus::Sidebar if rows > 0 => Focus::Settings,
                    Focus::Sidebar | Focus::Settings => Focus::Button(0),
                    Focus::Button(0) => Focus::Button(1),
                    Focus::Button(_) => Focus::Sidebar,
                };
            }
            (_, Action::PrevField) => {
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::Button(1),
                    Focus::Button(0) if rows > 0 => Focus::Settings,
                    Focus::Settings | Focus::Button(0) => Focus::Sidebar,
                    Focus::Button(_) => Focus::Button(0),
                };
            }
            (Focus::Sidebar, Action::Up) => self.choose_category(self.category.saturating_sub(1)),
            (Focus::Sidebar, Action::Down) => {
                let last = self.categories.len() - 1;
                self.choose_category((self.category + 1).min(last));
            }
            (Focus::Sidebar, Action::Right | Action::Toggle) if rows > 0 => {
                self.focus = Focus::Settings;
            }
            (Focus::Settings, Action::Up) => self.row = self.row.saturating_sub(1),
            (Focus::Settings, Action::Down) => self.row = (self.row + 1).min(last),
            (Focus::Settings, Action::PageUp) => self.row = self.row.saturating_sub(page),
            (Focus::Settings, Action::PageDown) => self.row = (self.row + page).min(last),
            (Focus::Settings, Action::Home) => self.row = 0,
            (Focus::Settings, Action::End) => self.row = last,
            (Focus::Settings, Action::Toggle) => {
                if let Some(setting) = self.setting_mut() {
                    setting.toggle();
                }
            }
            (Focus::Settings, Action::Left | Action::Right) => {
                let forward = action == Action::Right;
                let cycled = self
                    .setting_mut()
                    .is_some_and(|setting| setting.cycle(forward));
                if !cycled && !forward {
                    self.focus = Focus::Sidebar;
                }
            }
            (Focus::Button(_), Action::Up) if rows > 0 => self.focus = Focus::Settings,
            (Focus::Button(_), Action::Left) => self.focus = Focus::Button(0),
            (Focus::Button(_), Action::Right) => self.focus = Focus::Button(1),
            _ => {}
        }
        ConfigEvent::Pending
    }

    /// Draws the dialog centered in `area`, with the terminal cursor in a focused text field.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let colors = Colors::of(theme, false);
        let size = (
            WIDTH.min(area.width.saturating_sub(4)),
            HEIGHT.min(area.height.saturating_sub(2)),
        );
        let inner = draw_box(frame, area, size, &fl!("config-title"), colors, theme);
        // The body, a line, two lines of hint, a line, and the buttons.
        if inner.height < HINT_ROWS + 4 || inner.width < 20 {
            return;
        }
        let body = Rect {
            height: inner.height - HINT_ROWS - 3,
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

        let hint = match self.settings().get(self.row) {
            Some(setting) if self.focus == Focus::Settings && setting.restart => {
                format!("{} {}", setting.hint, fl!("config-restart"))
            }
            Some(setting) if self.focus == Focus::Settings => setting.hint.clone(),
            _ => String::new(),
        };
        let style = theme.dialog.patch(theme.menu_disabled);
        let lines = cells::wrap(&hint, usize::from(inner.width));
        for (row, line) in (0..HINT_ROWS).zip(lines) {
            let line = cells::fit(&line, usize::from(inner.width), Align::Left);
            let area = Rect::new(inner.x, body.bottom() + 1 + row, inner.width, 1);
            frame.render_widget(Line::styled(line, style), area);
        }
        let labels: Vec<String> = BUTTONS.iter().map(|button| button.label()).collect();
        let focus = match self.focus {
            Focus::Button(index) => Some(index),
            Focus::Sidebar | Focus::Settings => None,
        };
        draw_separator(frame, inner, inner.bottom() - 2, colors, theme);
        frame.render_widget(
            button_line(&labels, 0, focus, colors),
            Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
        );
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
            let label_style = if focused && !matches!(setting.value, Value::Text(_)) {
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
                Value::Text(field) => {
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

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    const THEMES: &[&str] = &["mc-classic", "terminal"];

    fn action(action: Action) -> Resolved {
        Resolved::Action(action)
    }

    fn dialog() -> Configuration {
        Configuration::new(&UiConfig::default(), THEMES, false)
    }

    fn draw(dialog: &mut Configuration, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| dialog.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn reads_back_what_it_shows_and_what_was_changed() {
        let ui = UiConfig {
            theme: "solarized".to_owned(),
            ..UiConfig::default()
        };
        let dialog = Configuration::new(&ui, THEMES, false);
        assert_eq!(dialog.ui(&ui), ui, "an unknown value is kept");

        let mut dialog = self::dialog();
        assert_eq!(dialog.context(), Context::DialogInput, "on the language");
        dialog.handle(Resolved::Insert('d'));
        dialog.handle(Resolved::Insert('e'));
        dialog.handle(action(Action::Down));
        assert_eq!(dialog.context(), Context::Dialog);
        dialog.handle(action(Action::Right));
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::Toggle));
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::Toggle));
        dialog.handle(action(Action::End));
        dialog.handle(action(Action::Left));
        assert_eq!(
            dialog.ui(&UiConfig::default()),
            UiConfig {
                language: "de".to_owned(),
                theme: "terminal".to_owned(),
                borders: Borders::Single,
                icons: false,
                menu_bar: MenuBar::Always,
                ..UiConfig::default()
            }
        );
        assert_eq!(
            dialog.handle(action(Action::Confirm)),
            ConfigEvent::Accepted
        );
        assert_eq!(
            dialog.handle(action(Action::Cancel)),
            ConfigEvent::Cancelled
        );
    }

    #[test]
    fn tab_moves_between_the_categories_the_settings_and_the_buttons() {
        let mut dialog = dialog();
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.focus, Focus::Button(0));
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.focus, Focus::Button(1));
        assert_eq!(
            dialog.handle(action(Action::Toggle)),
            ConfigEvent::Cancelled
        );
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.focus, Focus::Sidebar);
        dialog.handle(action(Action::Right));
        assert_eq!(dialog.focus, Focus::Settings);
        dialog.handle(action(Action::PrevField));
        dialog.handle(action(Action::PrevField));
        assert_eq!(dialog.focus, Focus::Button(1));
        dialog.handle(action(Action::Left));
        assert_eq!(
            dialog.handle(action(Action::Confirm)),
            ConfigEvent::Accepted
        );
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
    }

    #[test]
    fn scrolls_to_the_cursor_with_a_scroll_bar_when_the_settings_do_not_fit() {
        let mut dialog = dialog();
        let tall = draw(&mut dialog, 72, 24);
        assert!(!tall.contains('█'), "{tall}");
        // Home and End in the language field move its cursor.
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::End));
        let short = draw(&mut dialog, 72, 11);
        assert!(
            short.contains("Menu bar") && !short.contains("Language"),
            "{short}"
        );
        assert!(short.contains('█') && short.contains('░'), "{short}");
        dialog.handle(action(Action::Home));
        let top = draw(&mut dialog, 72, 11);
        assert!(
            top.contains("Language") && !top.contains("Menu bar"),
            "{top}"
        );
        assert!(top.contains("Takes effect after a restart"), "{top}");
    }

    #[test]
    fn draws_the_categories_and_the_settings() {
        let mut dialog = Configuration::new(&UiConfig::default(), THEMES, true);
        dialog.handle(action(Action::Down));
        insta::assert_snapshot!(draw(&mut dialog, 72, 16));
    }
}
