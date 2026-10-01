//! Key bindings: terminal keys to actions. So far only F10, Quit in mc.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Something a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    /// Quits sftp-tui.
    Quit,
}

/// The action bound to `key`, if any.
pub(crate) fn action(key: KeyEvent) -> Option<Action> {
    match (key.code, key.modifiers) {
        (KeyCode::F(10), KeyModifiers::NONE) => Some(Action::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f10_quits() {
        let key = |code, modifiers| action(KeyEvent::new(code, modifiers));
        assert_eq!(key(KeyCode::F(10), KeyModifiers::NONE), Some(Action::Quit));
        assert_eq!(key(KeyCode::F(10), KeyModifiers::SHIFT), None);
        assert_eq!(key(KeyCode::F(9), KeyModifiers::NONE), None);
        assert_eq!(key(KeyCode::Char('q'), KeyModifiers::NONE), None);
        assert_eq!(key(KeyCode::Char('c'), KeyModifiers::CONTROL), None);
    }
}
