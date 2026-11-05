//! The colors of a window by its tone.

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::tui::theme::Theme;

/// The colors of a window: a dialog, a list, or any other.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Colors {
    pub(crate) body: Style,
    pub(crate) title: Style,
    pub(crate) button: Style,
    pub(crate) focused: Style,
    /// The marks of text cut short, over `body`.
    pub(crate) mark: Style,
    /// The letters of labels, over the rest, and over what has the focus.
    hotkey: Style,
    hotkey_focused: Style,
    /// Text fields and combo boxes, and the lists the boxes open.
    pub(crate) input: Style,
    /// The letters of the choices of those lists, over `input`.
    input_hotkey: Style,
    /// The brackets of check boxes and combo boxes, and their checks and arrows, over `body`
    /// and over `input`.
    bracket: Style,
    symbol: Style,
}

impl Colors {
    /// The style of the button with the focus.
    pub(crate) fn focused_style(self) -> Style {
        self.focused
    }

    /// The style of text fields and combo boxes.
    pub(crate) fn input_style(self) -> Style {
        self.input
    }

    /// The style of the letters of the choices in the list of a combo box, over
    /// [`input_style`](Self::input_style).
    pub(crate) fn input_hotkey_style(self) -> Style {
        self.input_hotkey
    }

    /// The styles of the brackets and of the symbols of a combo box, over `input`.
    pub(crate) fn input_marks(self) -> (Style, Style) {
        (
            self.input.patch(self.bracket),
            self.input.patch(self.symbol),
        )
    }

    /// The colors of the choice under the cursor of the list, and of a box with the focus, over
    /// those of text fields: the focus's, as on a button; where fields have the focus's colors,
    /// as in mc-classic and the terminal theme, those of fields reversed, as Far shows the text
    /// selected in a field.
    pub(crate) fn selected_style(self) -> Style {
        if self.focused != self.input {
            self.focused
        } else if self.input.add_modifier.contains(Modifier::REVERSED) {
            self.input.not_reversed()
        } else {
            self.input.reversed()
        }
    }

    /// The style of the letters on [`selected_style`](Self::selected_style): as on what has the
    /// focus where that has its colors, else as on the command under the cursor of a menu,
    /// where the color of the letters of dialogs may not show.
    pub(crate) fn selected_hotkey_style(self, theme: &Theme) -> Style {
        if self.focused == self.input {
            theme.menu_hotkey_selected
        } else {
            self.hotkey_focused
        }
    }

    /// The style of the letters of labels, over the others: over what has the focus if
    /// `focused`, where the focus's colors may hide those of the others.
    pub(crate) fn hotkey_style(self, focused: bool) -> Style {
        if focused {
            self.hotkey_focused
        } else {
            self.hotkey
        }
    }

    /// The same colors, for what cannot be used: the body faint.
    pub(crate) fn disabled(self, theme: &Theme) -> Self {
        Self {
            body: self.body.patch(theme.disabled),
            ..self
        }
    }

    /// The body's style, and that of the marks of text cut short over it.
    pub(crate) fn body_styles(self) -> (Style, Style) {
        (self.body, self.mark)
    }

    /// A check box, `[✓]` while `on` and `[ ]` else, in the body's colors, its brackets faint
    /// and its check in the color of titles; or, `focused`, in the colors of the focus alone.
    pub(crate) fn check_box(self, on: bool, focused: bool, theme: &Theme) -> Vec<Span<'static>> {
        let (bracket, symbol) = if focused {
            (self.focused, self.focused)
        } else {
            (self.body.patch(self.bracket), self.body.patch(self.symbol))
        };
        let check = if on { theme.glyphs.check } else { ' ' };
        vec![
            Span::styled("[", bracket),
            Span::styled(check.to_string(), symbol),
            Span::styled("]", bracket),
        ]
    }

    /// The colors of a window in plain tone, as every list has them.
    pub(crate) fn plain(theme: &Theme) -> Self {
        Self::of(theme, Tone::Plain)
    }

    /// The colors of a window in `tone`: errors red, as mc draws them, and warnings on a
    /// background of their own, each with its own fields, brackets, and checks.
    pub(crate) fn of(theme: &Theme, tone: Tone) -> Self {
        match tone {
            Tone::Plain => Self {
                body: theme.dialog,
                title: theme.dialog_title,
                button: theme.dialog_button,
                focused: theme.dialog_button_focused,
                mark: theme.truncated_dialog,
                hotkey: theme.dialog_hotkey,
                hotkey_focused: theme.dialog_hotkey_focused,
                input: theme.dialog_input,
                input_hotkey: theme.dialog_hotkey,
                bracket: theme.dialog_bracket,
                symbol: theme.dialog_symbol,
            },
            Tone::Warning => Self {
                body: theme.warning_dialog,
                title: theme.warning_title,
                button: theme.warning_dialog,
                focused: theme.warning_button_focused,
                mark: theme.truncated_warning,
                hotkey: theme.warning_hotkey,
                hotkey_focused: theme.warning_hotkey_focused,
                input: theme.warning_input,
                input_hotkey: theme.warning_input_hotkey,
                bracket: theme.warning_bracket,
                symbol: theme.warning_symbol,
            },
            Tone::Error => Self {
                body: theme.error_dialog,
                title: theme.error_title,
                button: theme.error_dialog,
                focused: theme.error_button_focused,
                mark: theme.truncated_error,
                hotkey: theme.error_hotkey,
                hotkey_focused: theme.error_hotkey_focused,
                input: theme.error_input,
                input_hotkey: theme.error_input_hotkey,
                bracket: theme.error_bracket,
                symbol: theme.error_symbol,
            },
        }
    }
}

/// What a window says, which its colors show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    /// Anything else.
    Plain,
    /// Something went wrong.
    Error,
    /// A question before something that cannot be undone, such as deleting or writing over a
    /// file, or something to heed, such as that there was nothing to do.
    Warning,
}
