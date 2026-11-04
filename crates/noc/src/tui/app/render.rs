//! Drawing the screen.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use jiff::tz::TimeZone;
use noc_config::{MenuBar, TabBar};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType};

use crate::i18n::fl;
use crate::tui::cells::{self, Align};
use crate::tui::edit::Editor;
use crate::tui::find::FindWindow;
use crate::tui::glyphs::{self};
use crate::tui::jump::JumpMenu;
use crate::tui::keymap::{Action, Context, Mode};
use crate::tui::menu::LocationMenu;
use crate::tui::panel::{HostState, Panel, View};
use crate::tui::picker::FULL_ROWS;
use crate::tui::pulldown::{self};
use crate::tui::reading::Reading;
use crate::tui::spinner::{self, Spin, Wait};
use crate::tui::tabs::{self, Bar, Tabs};
use crate::tui::which_key;
use crate::tui::zoom::{self, Grows, Stage};

use super::{
    App, Completes, Host, Job, Purpose, SHOWCMD_WIDTH, Side, Sight, Spots, Whole, choices_for,
    fkey_label, waits_for,
};

impl App {
    /// When the screen needs drawing again, after the frame drawn at `drawn`, for what changes
    /// as time passes: a spinner that is due to show, or that turns; a host that connects in the
    /// location menu; the second that the time of a working job shows, with its speed and time
    /// left, in its window or in the list of jobs.
    pub(crate) fn spin_due(&self, drawn: Instant) -> Option<Instant> {
        let turns = self.config.animations.spinners;
        let mut due: Option<Instant> = None;
        let mut at = |time: Instant| due = Some(due.map_or(time, |due| due.min(time)));
        for wait in self.waits() {
            match wait.shows() {
                Some(shows) if shows > drawn => at(shows),
                Some(_) if turns => at(drawn + wait.step_after(drawn)),
                _ => {}
            }
        }
        let connecting = self
            .hosts
            .values()
            .any(|host| matches!(host, Host::Connecting { .. }));
        // The dots of a host that connects turn in the location menu and in the titles of panels.
        if turns && connecting && self.viewing.is_none() {
            at(drawn + spinner::DOTS_FRAME);
        }
        let ticks = self
            .jobs
            .iter()
            .filter(|job| match self.sight(job) {
                Sight::Window => true,
                Sight::Row => job.view.moves_data(),
                Sight::Corner => false,
            })
            .filter_map(|job| job.view.next_tick(drawn));
        for tick in ticks {
            at(tick);
        }
        let finished = self
            .viewing
            .as_ref()
            .and_then(|viewing| viewing.finished.as_ref());
        if let Some(goes) = finished.and_then(Reading::goes) {
            at(goes);
        }
        due
    }

    /// What waits on screen, each with a spinner.
    fn waits(&self) -> Vec<&Wait> {
        let dialog = self.dialogs.front().and_then(|open| open.dialog.wait());
        if let Some(viewing) = &self.viewing {
            return viewing
                .viewer
                .loading()
                .then_some(&viewing.wait)
                .into_iter()
                .chain(viewing.reading.as_ref().and_then(Reading::wait))
                .chain(dialog)
                .collect();
        }
        let panels = [Side::Left, Side::Right].map(|side| {
            let panel = self.panel(side);
            panel.wait().filter(|_| !self.stalls(panel))
        });
        panels
            .into_iter()
            .flatten()
            .chain(self.menu.as_ref().and_then(LocationMenu::wait))
            .chain(self.jump.as_ref().and_then(JumpMenu::wait))
            .chain(self.find.as_ref().and_then(FindWindow::wait))
            .chain(self.in_front().and_then(|job| job.view.shining()))
            .chain(dialog)
            .collect()
    }

    /// Whether `panel` waits to read a directory on a host that does not answer, whose dialog
    /// asks what to do: a question, not a wait, so nothing turns until the answer.
    fn stalls(&self, panel: &Panel) -> bool {
        self.hosts
            .iter()
            .any(|(host, state)| matches!(state, Host::Stalled { .. }) && waits_for(panel, host))
    }

    /// Two panels side by side above the F-key bar; the menu bar of F9 above them with
    /// `ui.menu_bar`, else over their top line while a menu is open. Dates are as of `now`;
    /// windows that open or close grow or shrink as of `at`, with `animations.windows`, and
    /// spinners turn.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, at: Instant, tz: &TimeZone) {
        self.spots = Spots::default();
        let always = self.config.ui.menu_bar == MenuBar::Always;
        let bar_height = u16::from(always);
        let screen = frame.area();
        let mut stage = std::mem::take(&mut self.stage);
        let scene = (screen, self.viewing.is_some());
        stage.begin(self.config.animations.windows, scene, &self.theme, at);
        let prompt = self.command_prompt(screen.width);
        let prompt_text = cells::text_of(&prompt);
        let line_height = self.line_height(&prompt_text, screen);
        let [menu_bar, panels, full, command_line, key_bar] = Layout::vertical([
            Constraint::Length(bar_height),
            Constraint::Fill(1),
            Constraint::Length(self.full_height()),
            Constraint::Length(line_height),
            Constraint::Length(1),
        ])
        .areas(screen);
        let menu_bar = Rect {
            height: 1.min(frame.area().height),
            ..menu_bar
        };
        let [(left_bar, left), (right_bar, right)] = self.panel_areas(panels);
        self.sync_connected();
        let active = self.active;
        let states: HashMap<String, HostState> = self
            .hosts
            .keys()
            .chain(&self.failed)
            .chain(self.addresses.keys())
            .map(|host| (host.clone(), self.host_state(host)))
            .collect();
        let hosts = |host: &str| states.get(host).cloned().unwrap_or_default();
        let formats = noc_locale::current();
        let spin = self.spin(at);
        let view = View {
            hosts: &hosts,
            decor: self.decor,
            theme: &self.theme,
            spin,
            tz,
            formats: &formats,
        };
        let root_theme = self.theme.for_root();
        let root_view = View {
            theme: &root_theme,
            ..view
        };
        let roots = [Side::Left, Side::Right].map(|side| self.root_at(self.panel(side).location()));
        if let Some(viewing) = &mut self.viewing {
            viewing.render(frame, panels, (&self.theme, self.decor), &spin);
        } else {
            let sides = [
                (Side::Left, &mut self.left, left_bar, left),
                (Side::Right, &mut self.right, right_bar, right),
            ];
            for ((side, tabs, line, area), root) in sides.into_iter().zip(roots) {
                let focused = active == side;
                let view = if root { &root_view } else { &view };
                let spots = render_side(frame, tabs, (line, area), focused, view, &self.home);
                let whole = line.map_or(area, |line| line.union(area));
                self.spots.sides.push((side, whole));
                let tabs = spots.into_iter().map(|(index, spot)| (side, index, spot));
                self.spots.tabs.extend(tabs);
            }
            self.render_full(frame, full);
        }
        self.spots.fkeys = self.render_fkeys(frame, key_bar);
        self.render_command_line(frame, command_line, &prompt);
        if always {
            self.spots.menu_bar = pulldown::render_idle(frame, menu_bar, &self.theme);
        }
        if !self.viewer_over() {
            self.render_pulldown(frame, menu_bar, &mut stage);
        }
        if self.viewing.is_none() {
            // On the menu bar, or on the top line of the panels without it.
            self.render_jobs(frame, menu_bar);
            self.render_menus(frame, panels, (&hosts, &spin), &mut stage);
        }
        self.render_windows(frame, panels, &spin, &mut stage);
        self.render_command_choices(frame, command_line, &prompt_text);
        if let Some(window) = &mut self.key_hints {
            stage.show(
                frame,
                (zoom::Window::KeyHints, Grows::FromMiddle),
                |frame| {
                    window.render(frame, panels, &self.theme);
                },
            );
        }
        stage.end(frame);
        self.stage = stage;
        if let Some(hints) = &self.hints {
            which_key::render(frame, panels, hints, &self.theme);
        }
    }

    /// The spinners as of `at`, counted from the first frame drawn.
    fn spin(&self, at: Instant) -> Spin {
        Spin::new(
            at,
            self.epoch.since(at),
            self.config.animations.spinners,
            self.theme.glyphs.spinners,
        )
    }

    /// The windows of a panel, centered over `panels` as every window is: the location menu,
    /// the tab menu, and the zoxide window, which is the active panel's; they grow and shrink
    /// on `stage`.
    fn render_menus(
        &mut self,
        frame: &mut Frame<'_>,
        panels: Rect,
        (hosts, spin): (&dyn Fn(&str) -> HostState, &Spin),
        stage: &mut Stage,
    ) {
        let (theme, decor) = (&self.theme, self.decor);
        if let Some(menu) = &mut self.menu {
            let side = menu.side();
            stage.show(
                frame,
                (zoom::Window::Menu(side), Grows::FromMiddle),
                |frame| {
                    menu.render(frame, panels, theme, decor, hosts, spin);
                },
            );
        }
        if let Some(menu) = &mut self.tab_menu {
            let side = menu.side();
            stage.show(
                frame,
                (zoom::Window::TabMenu(side), Grows::FromMiddle),
                |frame| {
                    menu.render(frame, panels, theme);
                },
            );
        }
        if let Some(jump) = &mut self.jump {
            let side = self.active;
            stage.show(
                frame,
                (zoom::Window::Jump(side), Grows::FromMiddle),
                |frame| {
                    jump.render(frame, panels, theme, spin);
                },
            );
        }
    }

    /// The pull-down menu of F9, if it is open: its bar at `menu_bar`, its menu below, which
    /// drops from it on `stage`.
    fn render_pulldown(&self, frame: &mut Frame<'_>, menu_bar: Rect, stage: &mut Stage) {
        if let Some(pulldown) = &self.pulldown {
            let screen = Rect {
                height: frame.area().height.saturating_sub(1),
                ..frame.area()
            };
            stage.show(frame, (zoom::Window::PullDown, Grows::Down), |frame| {
                pulldown.render(frame, (menu_bar, screen), &self.theme, &|command| {
                    self.command_status(command)
                });
            });
        }
    }

    /// The rows of the frame below the panels with the whole name under the cursor, while `K`
    /// asks for it.
    fn full_height(&self) -> u16 {
        if self.whole == Whole::Shown && self.viewing.is_none() {
            FULL_ROWS + 2
        } else {
            0
        }
    }

    /// The whole name under the cursor of the active panel, in a frame at `area`, below the
    /// panels, as `K` asks for it: single lines, in the colors of dialogs, which set it apart
    /// from the panels.
    fn render_full(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.height < 3 {
            return;
        }
        let theme = &self.theme;
        let block = Block::bordered()
            .border_type(BorderType::Plain)
            .style(theme.dialog)
            .border_style(theme.dialog);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let name = self.panel(self.active).full_name().unwrap_or_default();
        let size = (usize::from(inner.width), usize::from(inner.height));
        let styles = (theme.dialog, theme.truncated_dialog);
        let rows = cells::whole(&name, size, None, styles, theme.glyphs.marks);
        for (y, row) in (inner.y..inner.bottom()).zip(rows) {
            frame.render_widget(row, Rect::new(inner.x, y, inner.width, 1));
        }
    }

    /// The windows centered over `panels`, those in front last: the workspaces, the
    /// Configuration dialog, the help, the jobs, the checksums, and the dialogs, those for one
    /// panel too; they grow and shrink on `stage`. A viewer for a question about a taken name
    /// has only the dialogs over it.
    fn render_windows(
        &mut self,
        frame: &mut Frame<'_>,
        panels: Rect,
        spin: &Spin,
        stage: &mut Stage,
    ) {
        if self.viewer_over() {
            self.render_dialogs(frame, panels, spin, stage);
            return;
        }
        let middle = |window| (window, Grows::FromMiddle);
        let theme = &self.theme;
        if let Some(window) = &mut self.workspaces_window {
            stage.show(frame, middle(zoom::Window::Workspaces), |frame| {
                window.render(frame, panels, theme);
            });
        }
        if let Some(window) = &mut self.history_window {
            stage.show(frame, middle(zoom::Window::History), |frame| {
                window.render(frame, panels, theme);
            });
        }
        if let Some(window) = &mut self.find
            && self.viewing.is_none()
        {
            stage.show(frame, middle(zoom::Window::Find), |frame| {
                window.render(frame, panels, theme, spin);
            });
        }
        if let Some(configuration) = &mut self.configuration {
            stage.show(frame, middle(zoom::Window::Configuration), |frame| {
                configuration.render(frame, panels, theme);
            });
        }
        if let Some(help) = &mut self.help {
            stage.show(frame, middle(zoom::Window::Help), |frame| {
                help.render(frame, panels, theme);
            });
        }
        if let Some(list) = &self.jobs_list {
            let rows = self.job_rows();
            stage.show(frame, middle(zoom::Window::JobsList), |frame| {
                list.render(frame, panels, theme, &rows);
            });
        }
        if let Some(job) = self.in_front() {
            stage.show(frame, middle(zoom::Window::Job(job.id)), |frame| {
                job.view.render(frame, panels, theme, (spin.at(), spin));
            });
        }
        if let Some(results) = self.results.front_mut() {
            stage.show(frame, middle(zoom::Window::Results(results.id)), |frame| {
                results.window.render(frame, panels, theme);
            });
        }
        self.render_dialogs(frame, panels, spin, stage);
    }

    /// The dialog in front, over the one it is about if it is an error about it, and the list of
    /// completions of its field.
    fn render_dialogs(
        &mut self,
        frame: &mut Frame<'_>,
        panels: Rect,
        spin: &Spin,
        stage: &mut Stage,
    ) {
        let middle = |window| (window, Grows::FromMiddle);
        let theme = &self.theme;
        if let Some(open) = self.dialogs.front() {
            if matches!(open.purpose, Purpose::InfoOver)
                && let Some(under) = self.dialogs.get(1)
            {
                stage.show(
                    frame,
                    middle(zoom::Window::Dialog(under.dialog.id())),
                    |frame| under.dialog.render_under(frame, panels, theme),
                );
            }
            stage.show(
                frame,
                middle(zoom::Window::Dialog(open.dialog.id())),
                |frame| {
                    open.dialog.render_spun(frame, panels, theme, spin);
                },
            );
            if let Some(choices) = choices_for(&mut self.completion, Completes::Field)
                && let Some(field) = open.dialog.field_area()
            {
                choices.render(frame, field, panels, &self.theme);
            }
        }
    }

    /// The list of completions over the command line in `line`, after `prompt`, if it shows:
    /// its rows start where the names completed start on the line.
    fn render_command_choices(&mut self, frame: &mut Frame<'_>, line: Rect, prompt: &str) {
        if !self.typing_command() {
            return;
        }
        let Some(command_line) = &self.command_line else {
            return;
        };
        let Some(completing) = self
            .completion
            .as_mut()
            .filter(|completing| completing.target == Completes::CommandLine)
        else {
            return;
        };
        let column = command_line.column_of(prompt, line.width, completing.keep.chars().count());
        let Some(choices) = &mut completing.choices else {
            return;
        };
        let width = choices.width().min(line.width.saturating_sub(2));
        // On the screen, with room for the frame on both sides.
        let x = line
            .x
            .saturating_add(column)
            .min(line.right().saturating_sub(width + 1))
            .max(line.x + 1);
        let field = Rect::new(x, line.y, width, 1);
        // Above the first row of the line.
        let screen = frame.area();
        let above = Rect::new(screen.x, screen.y, screen.width, field.bottom() - screen.y);
        choices.render(frame, field, above, &self.theme);
    }

    /// Where the panels go in `area`, left side first, each with the line of its tabs if
    /// `ui.tab_bar` puts them on one. Both sides keep their panels level: a line of tabs above
    /// one goes above the other too.
    fn panel_areas(&self, area: Rect) -> [(Option<Rect>, Rect); 2] {
        let [mut left, mut right] = Layout::horizontal([Constraint::Fill(1); 2]).areas(area);
        if self.swapped {
            std::mem::swap(&mut left, &mut right);
        }
        let lines =
            self.config.ui.tab_bar == TabBar::Line && (self.left.len() > 1 || self.right.len() > 1);
        [left, right].map(|area| {
            if !lines || area.height < 2 {
                return (None, area);
            }
            let line = Rect { height: 1, ..area };
            let below = Rect {
                y: area.y + 1,
                height: area.height - 1,
                ..area
            };
            (Some(line), below)
        })
    }

    /// What the corner shows of the jobs in the background: how many, and how far they are
    /// together, in percent; nothing without any, or while the viewer covers the panels.
    pub(super) fn corner(&self) -> Option<(usize, u64)> {
        if self.viewing.is_some() {
            return None;
        }
        let behind: Vec<&Job> = self.jobs.iter().filter(|job| job.background).collect();
        if behind.is_empty() {
            return None;
        }
        let ratio = behind.iter().map(|job| job.view.ratio()).sum::<f64>();
        // A few jobs, each at 0 … 1.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let percent = (ratio / behind.len() as f64 * 100.0).round() as u64;
        Some((behind.len(), percent))
    }

    /// The jobs in the background at the top right of `area`, where Far has its clock: how
    /// many, and how far they are together.
    fn render_jobs(&self, frame: &mut Frame<'_>, area: Rect) {
        let Some((count, percent)) = self.corner() else {
            return;
        };
        let text = format!(
            " {} ",
            fl!("jobs-running", count = count, percent = percent)
        );
        // Inside the corner of the right panel's frame.
        let room = usize::from(area.width.saturating_sub(2));
        let width = cells::width(&text).min(room);
        let styles = (self.theme.panel_title_active, self.theme.truncated_cursor);
        let text = cells::fit(&text, width, Align::Left, styles, self.theme.glyphs.marks);
        let width = u16::try_from(width).unwrap_or(0);
        let x = area.right().saturating_sub(width + 1);
        let row = Rect::new(x, area.y, width, 1.min(area.height));
        frame.render_widget(Line::from(text), row);
    }

    /// The rows above the F-key bar on `screen`, as vim's command line: the command line's,
    /// after `prompt`, while it has the keys; else the line of the mode and of the command
    /// typed.
    fn line_height(&self, prompt: &str, screen: Rect) -> u16 {
        match &self.command_line {
            Some(line) if self.typing_command() => line.height(prompt, screen.width, screen.height),
            _ => 1,
        }
    }

    /// Whether the command line is open and has the keys, and so takes the line of the mode.
    fn typing_command(&self) -> bool {
        self.command_line.is_some()
            && matches!(
                self.context(),
                Context::CommandLine | Context::CommandCompletion
            )
    }

    /// The command line in `area`, after `prompt`, while it has the keys; else the mode and
    /// what the pending command has typed, on the line it takes.
    fn render_command_line(&mut self, frame: &mut Frame<'_>, area: Rect, prompt: &[Span<'static>]) {
        let typing = self.typing_command();
        match &mut self.command_line {
            Some(line) if typing => {
                let bar = self.theme.glyphs.scroll_bar;
                line.render(frame, area, (prompt, Style::default()), bar);
            }
            _ => self.render_mode(frame, area),
        }
    }

    /// The mode in capitals, as lualine shows it, then the window that has the keys, in `room`
    /// cells: the window goes first where they are too few, then the mode is cut. Returns how
    /// many cells they take whole. Every mode shows, so that the line always says where keys
    /// go: Normal mode and quick search too, which vim's `showmode` does not.
    fn mode_spans(&self, room: usize) -> (Vec<Span<'static>>, usize) {
        let once = self.focused_editor().is_some_and(Editor::once);
        let mode = match self.place() {
            (Mode::Insert, _) => fl!("mode-insert"),
            (Mode::Normal, _) if once => fl!("mode-insert-once"),
            (Mode::Visual, _) if once => fl!("mode-insert-visual"),
            (Mode::Visual, Context::Panel) if self.panel(self.active).visual_line() => {
                fl!("mode-visual-line")
            }
            (Mode::Visual, _) => fl!("mode-visual"),
            (Mode::Normal, _) => fl!("mode-normal"),
        };
        // While the command of Ctrl+x has the keys, as vim says that Insert mode comes back.
        let other =
            (self.returns).and_then(|_| self.keymap.short_key(Context::Panel, Action::OtherPanel));
        let mode = match other {
            Some(key) => fl!("mode-other", key = key, mode = mode),
            None => mode,
        };
        let bold = Style::new().bold();
        let mode_width = cells::width(&mode);
        let window = window_kind(self.context())
            .map(|kind| format!(" {} {kind}", glyphs::MODE_SEPARATOR))
            .filter(|window| mode_width + cells::width(window) <= room);
        if let Some(window) = window {
            let width = mode_width + cells::width(&window);
            return (vec![Span::styled(mode, bold), Span::raw(window)], width);
        }
        let styles = (bold, self.theme.truncated);
        let spans = cells::fit(&mode, room, Align::Left, styles, self.theme.glyphs.marks);
        (spans, mode_width)
    }

    /// The mode and the window that has the keys on the left of `area`, and what the pending
    /// command has typed, as vim's `showcmd` shows it, on the right.
    fn render_mode(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.is_empty() {
            return;
        }
        let editor = self.focused_editor();
        let mut typed = format!(
            "{}{}",
            self.pending_operator().unwrap_or_default(),
            self.showcmd
        );
        // With nothing typed, Visual mode shows how much it selects, as vim's `showcmd` does.
        if typed.is_empty() && self.mode() == Mode::Visual {
            let size = match editor {
                Some(editor) => editor.selection_size(),
                None => self.panel(self.active).visual_size(),
            };
            typed = size
                .and_then(|size| u64::try_from(size).ok())
                .map(|size| noc_locale::current().integer(size))
                .unwrap_or_default();
        }
        // As vim's, the last ten cells of what is typed.
        let typed: String = typed
            .chars()
            .rev()
            .take(SHOWCMD_WIDTH)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let width = usize::from(area.width);
        let typed = cells::sanitize(typed.as_bytes());
        let shown = cells::width(&typed).min(width);
        let room = width.saturating_sub(shown + 1);
        // A message takes the place of the mode, as on vim's command line.
        let marks = self.theme.glyphs.marks;
        let left_width = if let Some(message) = &self.message {
            let styles = (self.theme.message, self.theme.truncated);
            let message_line = cells::fit(message, room, Align::Left, styles, marks);
            frame.render_widget(Line::from(message_line), area);
            cells::width(message)
        } else {
            let (mode_line, left_width) = self.mode_spans(room);
            frame.render_widget(Line::from(mode_line), area);
            left_width
        };
        // Left of the cells of what is typed, as long as it fits after the mode or message.
        let who = self.mode_who();
        let who_width = cells::width(&cells::text_of(&who));
        let reserved = SHOWCMD_WIDTH + 1;
        if who_width > 0 && left_width + 1 + who_width + reserved <= width {
            let end = area.right() - u16::try_from(reserved).unwrap_or(0);
            let x = end - u16::try_from(who_width).unwrap_or(0);
            frame.render_widget(Line::from(who), Rect::new(x, area.y, end - x, 1));
        }
        let x = area
            .right()
            .saturating_sub(u16::try_from(shown).unwrap_or(0));
        let right = Rect::new(x, area.y, area.right() - x, 1);
        frame.render_widget(Line::raw(typed), right);
    }

    /// What F1 … F10 do now, each with its label on the F-key bar; `None` for a key that does
    /// nothing.
    pub(super) fn fkey_actions(&self) -> [Option<(Action, String)>; 10] {
        let (mode, context) = self.place();
        self.keymap.fkeys(mode, context).map(|action| {
            let action = action.filter(|action| self.supports(*action))?;
            let label = match (context, action) {
                (Context::Workspaces, Action::Move) => Some(fl!("fkey-rename")),
                _ => fkey_label(action),
            }?;
            Some((action, label))
        })
    }

    /// The F-key bar: ten equal slots, each the key number and the label of its action.
    /// Returns the slots.
    fn render_fkeys(&self, frame: &mut Frame<'_>, area: Rect) -> Vec<Rect> {
        let slots = Layout::horizontal([Constraint::Fill(1); 10]).split(area);
        let actions = self.fkey_actions();
        for (number, (slot, action)) in (1..).zip(slots.iter().zip(actions)) {
            let label = action.map(|(_, label)| label).unwrap_or_default();
            let number = number.to_string();
            // The label's color fills its slot, as in mc.
            let room = usize::from(slot.width).saturating_sub(number.len());
            let styles = (self.theme.fkey_label, self.theme.truncated_bar);
            let mut line = Line::from(Span::styled(number, self.theme.fkey_number));
            let label = cells::fit(&label, room, Align::Left, styles, self.theme.glyphs.marks);
            line.spans.extend(label);
            frame.render_widget(line, *slot);
        }
        slots.to_vec()
    }
}

/// What kind of window has the keys in `context`, as the line of the mode names it after the
/// mode: the kind says which keys work there, so windows with the same keys share it (ADR
/// 0032). None in the panels, which the mode alone stands for, on the command line, which
/// shows itself, and in the contexts keys never go to alone.
fn window_kind(context: Context) -> Option<String> {
    let kind = match context {
        Context::QuickSearch => fl!("window-quick-search"),
        Context::Rename => fl!("window-rename"),
        Context::LocationMenu
        | Context::TabMenu
        | Context::Jump
        | Context::Workspaces
        | Context::CommandHistory
        | Context::KeyHints => fl!("window-list"),
        Context::PullDown => fl!("window-menu"),
        Context::Dialog
        | Context::DialogInput
        | Context::PathInput
        | Context::Completion
        | Context::Find
        | Context::FindInput
        | Context::Checksums => fl!("window-dialog"),
        Context::ComboBox => fl!("window-combo-box"),
        Context::Help => fl!("window-help"),
        Context::Viewer => fl!("window-viewer"),
        Context::Panel
        | Context::CommandLine
        | Context::CommandCompletion
        | Context::Global
        | Context::List
        | Context::Input
        | Context::SearchInput
        | Context::UserScreen => return None,
    };
    Some(kind)
}

/// Draws the tab that shows of `tabs` in `area`, and its tabs on `line` if there is one, its
/// frame joined to it, else in its frame if it has more than one; `focused` if the side has the keys.
/// Returns where each tab shown is, by its index.
fn render_side(
    frame: &mut Frame<'_>,
    tabs: &mut Tabs,
    (line, area): (Option<Rect>, Rect),
    focused: bool,
    view: &View<'_>,
    home: &Path,
) -> Vec<(usize, Rect)> {
    tabs.active_mut().panel.render(frame, area, focused, view);
    let names = tabs.names(home, line.is_none());
    let bar = Bar {
        names: &names,
        active: tabs.index(),
        focused,
        theme: view.theme,
    };
    match line {
        Some(line) => {
            let spots = bar.render_line(frame, line);
            tabs::join_frame(frame, area, view.theme);
            spots
        }
        None if tabs.len() > 1 => {
            let spots = bar.render_frame(frame, area);
            tabs.active().panel.join_columns(frame, area, view.theme);
            spots
        }
        None => Vec::new(),
    }
}
