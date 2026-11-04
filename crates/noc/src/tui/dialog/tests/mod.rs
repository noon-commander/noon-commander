use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use secrecy::ExposeSecret as _;

use super::field::SECRET_CAPACITY;
use super::*;
use crate::tui::keymap::{Extra, Typed};
use crate::tui::mouse::{Pointer, Press};
use crate::tui::picker::MenuLine;
use crate::tui::spinner::Shine;
use crate::tui::theme::Theme;

mod basics;
mod colors;
mod combo_box;
mod failures;
mod fields;
mod forms;
mod letters;
mod parts;
mod pickers;
mod pointer;
mod rows;
mod skeleton;

fn action(action: Action) -> Resolved {
    Resolved::Action(action)
}

fn typed(dialog: &mut Dialog, text: &str) {
    for c in text.chars() {
        assert!(matches!(
            dialog.handle(Resolved::Insert(c)),
            DialogEvent::Pending
        ));
    }
}

/// Presses Enter and returns what ssh would get.
fn confirm(dialog: &mut Dialog) -> Option<String> {
    let event = dialog.handle(action(Action::Confirm));
    assert_ne!(event, DialogEvent::Pending);
    dialog
        .answer(event)
        .map(|answer| answer.expose_secret().to_owned())
}

fn draw(dialog: &Dialog, width: u16, height: u16) -> Terminal<TestBackend> {
    draw_themed(dialog, width, height, &Theme::terminal())
}

fn draw_themed(dialog: &Dialog, width: u16, height: u16, theme: &Theme) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| dialog.render(frame, frame.area(), theme))
        .unwrap();
    terminal
}

fn secret() -> Dialog {
    Dialog::prompt("web", "deploy@10.0.0.5's password: ", PromptKind::Secret)
}

/// The column where `title` starts on the dialog's top row, row 1.
fn title_column(buffer: &Buffer, title: &str) -> u16 {
    let row: String = (0..buffer.area.width)
        .map(|x| buffer[(x, 1)].symbol())
        .collect();
    let start = row.find(title).unwrap();
    u16::try_from(row[..start].chars().count()).unwrap()
}

fn form() -> Dialog {
    let checks = [
        ("Files only".to_owned(), false),
        ("Case sensitive".to_owned(), true),
    ];
    Dialog::form("Select", "", "*", &checks).with_width(50)
}

/// Presses the mouse on the first cell of `text` where `dialog` is drawn at 40 by 12.
fn press_on(dialog: &mut Dialog, press: Press, text: &str) -> DialogEvent {
    let terminal = draw(dialog, 40, 12);
    let at = crate::tui::mouse::find(terminal.backend().buffer(), text);
    dialog.pointer(Pointer { press, at })
}

fn picker() -> Dialog {
    let choices = ["&one".to_owned(), "&two".to_owned(), "t&hree".to_owned()];
    Dialog::fields("Pick", &[], &[], vec![Button::Ok, Button::Cancel])
        .with_width(40)
        .with_combo_box("&Count:", &choices, 0)
        .with_check("&Verbose", false)
}

/// A question with two items between lines, and a combo box.
fn with_items() -> Dialog {
    let buttons = vec![Button::Overwrite, Button::Skip, Button::Abort];
    Dialog::question("File exists", "/srv/a", buttons, 1, Tone::Warning)
        .with_line()
        .with_item("&New", "10 bytes  2026-01-02 03:04:05")
        .with_item("E&xisting", "5 bytes  2025-01-02 03:04:05")
        .with_line()
        .with_combo_box("&The rest:", &["Ask each time".to_owned()], 0)
        .with_focus_on_default()
}

fn host_form() -> Dialog {
    let fields = [
        ("Label:".to_owned(), "Prod".to_owned()),
        ("Remote directory:".to_owned(), "/var/www".to_owned()),
        ("Other directory:".to_owned(), String::new()),
    ];
    let checks = [("Remember".to_owned(), true)];
    let buttons = vec![Button::Ok, Button::UseCurrent, Button::Cancel];
    Dialog::fields("Host web", &fields, &checks, buttons).with_width(60)
}
