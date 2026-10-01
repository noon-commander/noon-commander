//! State and drawing of the whole screen.

use std::path::Path;
use std::time::SystemTime;

use jiff::tz::TimeZone;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use sftp_tui_vfs::{Location, VfsError};

use super::keymap::{Action, Context, Keymap, Resolved};
use super::panel::{ListRequest, Listing, Panel};
use crate::i18n::fl;

/// One of the two panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
}

impl Side {
    fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// Work the app asks the event loop to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Effect {
    /// List a location and pass the result to [`App::listed`].
    List { side: Side, request: ListRequest },
}

/// What the TUI shows and whether it keeps running.
#[derive(Debug)]
pub(crate) struct App {
    left: Panel,
    right: Panel,
    active: Side,
    quit: bool,
    redraw: bool,
}

impl App {
    /// Both panels on the local directory `start`, and the listings to request for them. From
    /// the virtual root, the local file system opens at `home`.
    pub(crate) fn new(start: &Path, home: &Path) -> (Self, Vec<Effect>) {
        let panel = || Panel::new(Location::Local(start.to_path_buf()), home.to_path_buf());
        let (left, left_request) = panel();
        let (right, right_request) = panel();
        let app = Self {
            left,
            right,
            active: Side::Left,
            quit: false,
            redraw: false,
        };
        let effects = vec![
            Effect::List {
                side: Side::Left,
                request: left_request,
            },
            Effect::List {
                side: Side::Right,
                request: right_request,
            },
        ];
        (app, effects)
    }

    /// Whether the user asked to quit.
    pub(crate) fn quits(&self) -> bool {
        self.quit
    }

    /// Whether the whole screen must be drawn again; resets the request.
    pub(crate) fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    /// Where keys go now.
    #[expect(clippy::unused_self, reason = "dialogs and quick search come later")]
    pub(crate) fn context(&self) -> Context {
        Context::Panel
    }

    /// Whether the app does something for `action` yet; the F-key bar shows only those.
    fn supports(action: Action) -> bool {
        matches!(action, Action::Quit | Action::Redraw)
    }

    fn panel_mut(&mut self, side: Side) -> &mut Panel {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    pub(crate) fn handle(&mut self, input: Resolved) -> Vec<Effect> {
        let Resolved::Action(action) = input else {
            return Vec::new();
        };
        match action {
            Action::Quit => self.quit = true,
            Action::Redraw => self.redraw = true,
            Action::SwitchPanel => self.active = self.active.other(),
            _ => {
                let side = self.active;
                if let Some(request) = self.panel_mut(side).handle(action) {
                    return vec![Effect::List { side, request }];
                }
            }
        }
        Vec::new()
    }

    /// Takes the result of an [`Effect::List`].
    pub(crate) fn listed(
        &mut self,
        side: Side,
        generation: u64,
        result: Result<Listing, VfsError>,
    ) {
        self.panel_mut(side).listed(generation, result);
    }

    /// Two panels side by side above the F-key bar.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        keymap: &Keymap,
        now: SystemTime,
        tz: &TimeZone,
    ) {
        let [panels, key_bar] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        let [left, right] = Layout::horizontal([Constraint::Fill(1); 2]).areas(panels);
        let active = self.active;
        self.left.render(frame, left, active == Side::Left, now, tz);
        self.right
            .render(frame, right, active == Side::Right, now, tz);
        self.render_fkeys(frame, key_bar, keymap);
    }

    /// The F-key bar: ten equal slots, each the key number and the label of its action.
    fn render_fkeys(&self, frame: &mut Frame<'_>, area: Rect, keymap: &Keymap) {
        let slots = Layout::horizontal([Constraint::Fill(1); 10]).split(area);
        let actions = keymap.fkeys(self.context());
        for (number, (slot, action)) in (1..).zip(slots.iter().zip(actions)) {
            let label = action
                .filter(|action| Self::supports(*action))
                .and_then(fkey_label)
                .unwrap_or_default();
            let line = Line::from(vec![
                Span::raw(number.to_string()),
                Span::styled(label, Style::new().reversed()),
            ]);
            frame.render_widget(line, *slot);
        }
    }
}

/// The label of an action in the F-key bar.
fn fkey_label(action: Action) -> Option<String> {
    match action {
        Action::Quit => Some(fl!("fkey-quit")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    use sftp_tui_vfs::{DirEntry, FileKind, Metadata};

    use super::*;

    fn dir(name: &str) -> DirEntry {
        DirEntry {
            name: name.as_bytes().to_vec(),
            metadata: Metadata {
                kind: FileKind::Dir,
                size: Some(4096),
                permissions: Some(0o755),
                modified: Some(UNIX_EPOCH + Duration::from_secs(1_699_990_000)),
                uid: None,
                gid: None,
            },
            target_kind: None,
        }
    }

    /// An app on `/srv` whose first listings arrived.
    fn loaded() -> App {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"));
        for effect in effects {
            let Effect::List { side, request } = effect;
            assert_eq!(request.location, local("/srv"));
            let entries = vec![dir("left"), dir("right")];
            app.listed(side, request.generation, Ok(Listing::Dir(entries)));
        }
        app
    }

    fn local(path: &str) -> Location {
        Location::Local(PathBuf::from(path))
    }

    fn action(action: Action) -> Resolved {
        Resolved::Action(action)
    }

    #[test]
    fn lists_both_panels_at_start() {
        let (_, effects) = App::new(Path::new("/srv"), Path::new("/home/me"));
        let sides: Vec<Side> = effects
            .iter()
            .map(|Effect::List { side, .. }| *side)
            .collect();
        assert_eq!(sides, [Side::Left, Side::Right]);
    }

    #[test]
    fn keys_go_to_the_active_panel_and_tab_switches_it() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        let [Effect::List { side, request }] =
            app.handle(action(Action::Enter)).try_into().unwrap();
        assert_eq!((side, request.location), (Side::Left, local("/srv/left")));

        assert!(app.handle(action(Action::SwitchPanel)).is_empty());
        app.handle(action(Action::End));
        let [Effect::List { side, request }] =
            app.handle(action(Action::Enter)).try_into().unwrap();
        assert_eq!((side, request.location), (Side::Right, local("/srv/right")));

        app.handle(action(Action::SwitchPanel));
        assert_eq!(app.active, Side::Left);
    }

    #[test]
    fn replies_reach_their_own_panel() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        let [Effect::List { side, request }] =
            app.handle(action(Action::Enter)).try_into().unwrap();
        app.listed(
            side.other(),
            request.generation,
            Ok(Listing::Dir(Vec::new())),
        );
        app.listed(
            side,
            request.generation,
            Ok(Listing::Dir(vec![dir("deeper")])),
        );
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        terminal
            .draw(|frame| app.render(frame, &Keymap::mc(), now, &TimeZone::UTC))
            .unwrap();
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn handles_quit_redraw_and_text() {
        let mut app = loaded();
        assert!(app.handle(Resolved::Insert('q')).is_empty());
        assert!(!app.quits());
        assert!(!app.take_redraw());
        app.handle(action(Action::Redraw));
        assert!(app.take_redraw());
        assert!(!app.take_redraw(), "a redraw is requested once");
        app.handle(action(Action::Quit));
        assert!(app.quits());
    }

    #[test]
    fn every_supported_f_key_action_has_a_label() {
        let keymap = Keymap::mc();
        for action in keymap.fkeys(Context::Panel).into_iter().flatten() {
            if App::supports(action) {
                assert!(fkey_label(action).is_some(), "{action:?}");
            }
        }
    }
}
