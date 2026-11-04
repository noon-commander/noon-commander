//! Keys and the mouse in a dialog: answers, focus, and letters.

use crate::tui::combo_box::ComboEvent;
use crate::tui::edit::Registers;
use crate::tui::keymap::{Action, Extra, Mode, Resolved, Typed};
use crate::tui::mouse::{Pointer, Press};
use crate::tui::picker::Pick;
use crate::tui::search::Switch;

use super::{Dialog, DialogEvent, Focus, LabelledField, Part};

impl Dialog {
    /// Handles an action or a typed character, as typed without a count or a register.
    #[cfg(test)]
    pub(crate) fn handle(&mut self, input: Resolved) -> DialogEvent {
        self.handle_typed(Typed::from(input), &mut Registers::default())
    }

    /// Handles an action or a typed character: the focused field takes what it edits with,
    /// in its mode. Tab brings the focus to a field in Insert mode; the other keys that move it
    /// bring it there in Normal mode, but from a field in Insert mode. The open list of a combo
    /// box takes every key, Alt and a letter too, until it closes.
    pub(crate) fn handle_typed(&mut self, typed: Typed, registers: &mut Registers) -> DialogEvent {
        let Typed { resolved, extra } = typed;
        if let Some(combo) = self.open_combo_box_mut() {
            match resolved {
                Resolved::Action(action) => combo.handle(action, extra),
                Resolved::Hotkey(c) => combo.hotkey(c),
                Resolved::Insert(_) => ComboEvent::Pending,
            };
            return DialogEvent::Pending;
        }
        if let Resolved::Hotkey(c) = resolved {
            return self.hotkey(c);
        }
        if let Some(event) = self.handle_picker(resolved, extra) {
            return event;
        }
        let action = match (resolved, self.focused_field_mut()) {
            (Resolved::Insert(c), Some(field)) => {
                field.insert(c);
                return DialogEvent::Pending;
            }
            (Resolved::Hotkey(_), _) | (Resolved::Insert(_), None) => {
                return DialogEvent::Pending;
            }
            (Resolved::Action(action), Some(field)) => {
                if field.handle(action, extra, registers) {
                    return DialogEvent::Pending;
                }
                // A secret has no Normal mode: Esc closes its dialog.
                if action == Action::NormalMode {
                    return DialogEvent::Cancelled;
                }
                // A key of the dialog's is the one command after Ctrl+o, which leaves the field
                // in Insert mode, so that one that moves the focus moves it there in Insert mode.
                field.give_up();
                action
            }
            (Resolved::Action(action), None) => action,
        };
        if let Some(switch) = Switch::of(action) {
            if let Focus::Field(index) = self.focus {
                self.switch_search(index, switch);
            }
            return DialogEvent::Pending;
        }
        match action {
            Action::Confirm => match self.focus {
                Focus::Button(index) => DialogEvent::Pressed(self.buttons[index]),
                Focus::Item(index) => DialogEvent::Opened(index),
                Focus::ComboBox(_) | Focus::Field(_) | Focus::Check(_) | Focus::Picker => {
                    DialogEvent::Pressed(self.buttons[self.default])
                }
            },
            Action::Toggle => match self.focus {
                Focus::ComboBox(index) => {
                    self.combo_boxes[index].combo.open();
                    DialogEvent::Pending
                }
                Focus::Check(index) => {
                    self.checks[index].on = !self.checks[index].on;
                    DialogEvent::Pending
                }
                Focus::Button(index) => DialogEvent::Pressed(self.buttons[index]),
                Focus::Item(index) => DialogEvent::Opened(index),
                Focus::Field(_) => DialogEvent::Pending,
                // Space on a row takes it, as Enter does.
                Focus::Picker => DialogEvent::Pressed(self.buttons[self.default]),
            },
            Action::Cancel => DialogEvent::Cancelled,
            Action::NextField | Action::PrevField => {
                self.move_focus(action == Action::NextField, Mode::Insert);
                DialogEvent::Pending
            }
            // A count moves as many stops, past the fields too, as vim's `3j` moves three lines.
            Action::Right | Action::Down | Action::Left | Action::Up => {
                let mode = self.mode();
                for _ in 0..extra.times() {
                    self.move_focus(matches!(action, Action::Right | Action::Down), mode);
                }
                DialogEvent::Pending
            }
            // As vim's `g g` and `G` go to the first and the last line.
            Action::Home | Action::End | Action::PageUp | Action::PageDown => {
                self.focus_end(matches!(action, Action::End | Action::PageDown));
                DialogEvent::Pending
            }
            _ => DialogEvent::Pending,
        }
    }

    /// Gives a key to the picker, if there is one. `/` brings the focus to its rows, typing in
    /// its filter, if it has one. While they have the focus they take every key but Enter,
    /// Space, Tab, and Shift+Tab, which leave them: the motions move the cursor, as many rows as
    /// a count says, and stop at the first row and the last, Left and Right go to the stop before
    /// or after while the filter takes nothing, and Esc on the rows closes the dialog. `None`
    /// for a key the dialog takes.
    fn handle_picker(&mut self, resolved: Resolved, extra: Extra) -> Option<DialogEvent> {
        let filters = self.picker.as_ref()?.has_filter();
        if self.focus != Focus::Picker {
            if resolved != Resolved::Action(Action::QuickSearch)
                || !filters
                || self.focused_field().is_some()
            {
                return None;
            }
            self.focus_on(Focus::Picker, Mode::Normal);
        }
        let picker = self.picker.as_mut()?;
        let times = match resolved {
            Resolved::Action(
                Action::Confirm | Action::Toggle | Action::NextField | Action::PrevField,
            ) => return None,
            // A count moves as many rows, or pages, as vim's `3j` moves three lines.
            Resolved::Action(Action::Up | Action::Down) => extra.times(),
            Resolved::Action(Action::PageUp | Action::PageDown) => {
                picker.count_pages(extra.times());
                1
            }
            _ => 1,
        };
        let mut pick = Pick::Pending;
        for _ in 0..times {
            pick = picker.handle(resolved);
        }
        match pick {
            Pick::Closed => Some(DialogEvent::Cancelled),
            Pick::Other(Action::Left | Action::Right) => None,
            Pick::Pending | Pick::Filtered | Pick::Chosen(_) | Pick::Other(_) => {
                Some(DialogEvent::Pending)
            }
        }
    }

    /// Alt and `c`, as mc and Far take them: a button with the letter is pressed, a check box
    /// switched, the list of a combo box opened, and an item opened, each with the focus; a
    /// field with it, or under a prompt with it, gets the focus in Insert mode. Nothing else has
    /// it.
    fn hotkey(&mut self, c: char) -> DialogEvent {
        let usable = self.fields.first().is_some_and(|field| !field.disabled);
        let found = if self.message.has(c) && usable {
            Some(Focus::Field(0))
        } else {
            self.stops().into_iter().find(|&stop| match stop {
                Focus::ComboBox(index) => self.combo_boxes[index].label.has(c),
                Focus::Field(index) => self.fields[index].label.has(c),
                Focus::Check(index) => self.checks[index].label.has(c),
                Focus::Item(index) => self.items[index].label.has(c),
                Focus::Picker => false,
                Focus::Button(index) => self.buttons[index].label().has(c),
            })
        };
        let Some(stop) = found else {
            return DialogEvent::Pending;
        };
        if let Focus::Button(index) = stop {
            return DialogEvent::Pressed(self.buttons[index]);
        }
        self.give_up();
        self.focus_on(stop, Mode::Insert);
        match stop {
            Focus::ComboBox(index) => self.combo_boxes[index].combo.open(),
            Focus::Check(index) => self.checks[index].on = !self.checks[index].on,
            Focus::Item(index) => return DialogEvent::Opened(index),
            Focus::Field(_) | Focus::Picker | Focus::Button(_) => {}
        }
        DialogEvent::Pending
    }

    /// The letters of everything Alt and a letter act on, the prompt of a lone field first, in
    /// the order of the stops; one for each that has a letter.
    #[cfg(test)]
    pub(crate) fn letters(&self) -> Vec<char> {
        let prompt = (!self.fields.is_empty()).then(|| self.message.letter());
        let stops = self.stops().into_iter().map(|stop| match stop {
            Focus::ComboBox(index) => self.combo_boxes[index].label.letter(),
            Focus::Field(index) => self.fields[index].label.letter(),
            Focus::Check(index) => self.checks[index].label.letter(),
            Focus::Item(index) => self.items[index].label.letter(),
            Focus::Picker => None,
            Focus::Button(index) => self.buttons[index].label().letter(),
        });
        prompt.into_iter().chain(stops).flatten().collect()
    }

    /// Takes a press of the mouse, where the dialog was drawn last: a click opens the list of a
    /// combo box, puts the cursor in a field or on a row of the list, switches a check box,
    /// opens an item, or presses a button. A click on a row of the picker puts its cursor there,
    /// and a double click presses the default button too, as Enter does there. A click on a glyph at the end of a field that searches gives it the focus and
    /// switches whether case counts, or how it matches, as the keys after Ctrl+s do there. The
    /// open list of a combo box takes every press, and the second click of a double click that
    /// closed it does nothing.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> DialogEvent {
        let mut spent = false;
        for labelled in &mut self.combo_boxes {
            spent |= labelled.combo.spent(pointer);
        }
        if let Some(combo) = self.open_combo_box_mut() {
            combo.pointer(pointer);
            return DialogEvent::Pending;
        }
        if spent {
            return DialogEvent::Pending;
        }
        let Pointer { press, at } = pointer;
        let glyph = self
            .glyphs
            .borrow()
            .iter()
            .find_map(|(index, drawn)| Some((*index, drawn.action_at(at)?)));
        if let Some((index, action)) = glyph {
            if press == Press::Click {
                if self.focus != Focus::Field(index) {
                    self.focus_on(Focus::Field(index), Mode::Insert);
                }
                if let Some(switch) = Switch::of(action) {
                    self.switch_search(index, switch);
                }
            }
            return DialogEvent::Pending;
        }
        if self.picker_area.get().is_some_and(|area| area.contains(at)) {
            return self.press_picker(pointer);
        }
        let spot = self
            .spots
            .borrow()
            .iter()
            .find(|(_, area)| area.contains(at))
            .copied();
        let Some((focus, area)) = spot else {
            return DialogEvent::Pending;
        };
        if !matches!(press, Press::Click | Press::DoubleClick) {
            return DialogEvent::Pending;
        }
        if focus != self.focus {
            self.leave_field();
        }
        self.focus = focus;
        match focus {
            Focus::ComboBox(index) => self.combo_boxes[index].combo.open(),
            Focus::Field(index) => {
                if let Some(LabelledField { field, .. }) = self.fields.get_mut(index) {
                    let column = usize::from(at.x - area.x);
                    field.click(column, usize::from(area.width));
                }
            }
            Focus::Check(index) => self.checks[index].on = !self.checks[index].on,
            Focus::Picker => {}
            // The first click pressed it, or opened it.
            Focus::Button(_) | Focus::Item(_) if press == Press::DoubleClick => {}
            Focus::Button(index) => return DialogEvent::Pressed(self.buttons[index]),
            Focus::Item(index) => return DialogEvent::Opened(index),
        }
        DialogEvent::Pending
    }

    /// A press of the mouse on the picker: a click on a row puts the cursor there, and a double
    /// click presses the default button too, as Enter does there; a click on a glyph of how its
    /// filter matches switches it. Either gives its rows the focus.
    fn press_picker(&mut self, pointer: Pointer) -> DialogEvent {
        if !matches!(pointer.press, Press::Click | Press::DoubleClick) {
            return DialogEvent::Pending;
        }
        let Some(picker) = &mut self.picker else {
            return DialogEvent::Pending;
        };
        let row = picker.row_at(pointer.at);
        let action = picker.pointer(pointer);
        let switch = action.and_then(Switch::of);
        if !row && switch.is_none() {
            return DialogEvent::Pending;
        }
        if self.focus != Focus::Picker {
            self.leave_field();
            self.focus = Focus::Picker;
        }
        if let (Some(action), Some(_), Some(picker)) = (action, switch, &mut self.picker) {
            picker.handle(Resolved::Action(action));
        }
        if row && pointer.press == Press::DoubleClick && !self.buttons.is_empty() {
            return DialogEvent::Pressed(self.buttons[self.default]);
        }
        DialogEvent::Pending
    }

    /// Where the focus can be: the combo boxes, the fields that are not disabled, the check
    /// boxes, the items, and the rows of the picker, top to bottom, then the buttons.
    pub(super) fn stops(&self) -> Vec<Focus> {
        self.parts
            .iter()
            .filter_map(|&part| match part {
                Part::ComboBox(index) => Some(Focus::ComboBox(index)),
                Part::Field(index) if !self.fields[index].disabled => Some(Focus::Field(index)),
                Part::Check(index) => Some(Focus::Check(index)),
                Part::Item(index) => Some(Focus::Item(index)),
                Part::Picker => self.picker.as_ref().map(|_| Focus::Picker),
                Part::Field(_) | Part::Text(_) | Part::Separator | Part::Line => None,
            })
            .chain((0..self.buttons.len()).map(Focus::Button))
            .collect()
    }

    /// Moves the focus through the stops, round; a field it comes to is in `mode`, Insert or
    /// Normal.
    fn move_focus(&mut self, forward: bool, mode: Mode) {
        let stops = self.stops();
        let current = stops
            .iter()
            .position(|stop| *stop == self.focus)
            .unwrap_or(0);
        let step = if forward { 1 } else { stops.len() - 1 };
        self.focus_on(stops[(current + step) % stops.len()], mode);
    }

    /// Moves the focus to the first stop, or the last; a field it comes to is in Normal mode.
    fn focus_end(&mut self, last: bool) {
        let stops = self.stops();
        let stop = if last { stops.last() } else { stops.first() };
        if let Some(&stop) = stop
            && stop != self.focus
        {
            self.focus_on(stop, Mode::Normal);
        }
    }

    /// Moves the focus to `stop`; a field there is in `mode`, Insert or Normal.
    fn focus_on(&mut self, stop: Focus, mode: Mode) {
        self.leave_field();
        self.focus = stop;
        let mode = if mode == Mode::Insert {
            Mode::Insert
        } else {
            Mode::Normal
        };
        if let Some(field) = self.focused_field_mut() {
            field.set_mode(mode);
        }
    }
}
