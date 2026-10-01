//! State and drawing of the whole screen.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;

use super::keymap::{Action, Context, Keymap, Resolved};
use crate::i18n::fl;

/// What the TUI shows and whether it keeps running.
#[derive(Debug, Default)]
pub(crate) struct App {
    quit: bool,
    redraw: bool,
}

impl App {
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

    pub(crate) fn handle(&mut self, input: Resolved) {
        match input {
            Resolved::Action(Action::Quit) => self.quit = true,
            Resolved::Action(Action::Redraw) => self.redraw = true,
            Resolved::Action(_) | Resolved::Insert(_) => {}
        }
    }

    /// Two panels side by side above the F-key bar.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, keymap: &Keymap) {
        let [panels, key_bar] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        let [left, right] = Layout::horizontal([Constraint::Fill(1); 2]).areas(panels);
        frame.render_widget(Block::bordered(), left);
        frame.render_widget(Block::bordered(), right);
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    #[test]
    fn draws_two_panels_above_the_key_bar() {
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        let keymap = Keymap::mc();
        terminal
            .draw(|frame| App::default().render(frame, &keymap))
            .unwrap();
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn handles_quit_and_redraw() {
        let mut app = App::default();
        app.handle(Resolved::Insert('q'));
        app.handle(Resolved::Action(Action::Down));
        assert!(!app.quits());
        assert!(!app.take_redraw());
        app.handle(Resolved::Action(Action::Redraw));
        assert!(app.take_redraw());
        assert!(!app.take_redraw(), "a redraw is requested once");
        app.handle(Resolved::Action(Action::Quit));
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
