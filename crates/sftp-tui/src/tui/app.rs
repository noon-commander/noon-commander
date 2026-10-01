//! State and drawing of the whole screen.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;

use super::keymap::Action;
use crate::i18n::fl;

/// What the TUI shows and whether it keeps running.
#[derive(Debug, Default)]
pub(crate) struct App {
    quit: bool,
}

impl App {
    /// Whether the user asked to quit.
    pub(crate) fn quits(&self) -> bool {
        self.quit
    }

    pub(crate) fn handle(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit = true,
        }
    }

    /// Two panels side by side above the key bar.
    #[expect(clippy::unused_self, reason = "the panels have no state yet")]
    pub(crate) fn render(&self, frame: &mut Frame<'_>) {
        let [panels, key_bar] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        let [left, right] = Layout::horizontal([Constraint::Fill(1); 2]).areas(panels);
        frame.render_widget(Block::bordered(), left);
        frame.render_widget(Block::bordered(), right);
        let keys = Line::from(vec![
            Span::raw("10"),
            Span::styled(fl!("fkey-quit"), Style::new().reversed()),
        ]);
        frame.render_widget(keys, key_bar);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    #[test]
    fn draws_two_panels_above_the_key_bar() {
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal.draw(|frame| App::default().render(frame)).unwrap();
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn quit_ends_the_app() {
        let mut app = App::default();
        assert!(!app.quits());
        app.handle(Action::Quit);
        assert!(app.quits());
    }
}
