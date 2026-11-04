//! Quick search in a panel.

use crate::i18n::fl;
use crate::tui::keymap::{Action, Resolved};
use crate::tui::panel::SearchEnd;
use crate::tui::search::{Slot, Switch};

use super::{App, Side};

impl App {
    /// Gives a key to quick search in the active panel, if it runs; the action that the panel
    /// does next, if any. Characters go nowhere else: typing in a panel does nothing.
    pub(super) fn search_key(&mut self, input: Resolved) -> Option<Action> {
        let side = self.active;
        let searching = self.panel(side).searching();
        if searching
            && let Resolved::Action(action) = input
            && let Some(switch) = Switch::of(action)
        {
            self.panel_mut(side).switch_search(switch);
            return None;
        }
        match input {
            Resolved::Insert(c) => {
                if searching {
                    self.panel_mut(side).search_type(c);
                }
                None
            }
            Resolved::Hotkey(_) => None,
            Resolved::Action(action) if searching => match action {
                // Unlike vim's search, Backspace and Delete with nothing typed keep it open, as
                // on the command line: a Backspace too many would go to the parent directory.
                Action::Left
                | Action::Right
                | Action::TextStart
                | Action::TextEnd
                | Action::WordForward
                | Action::WordBackward
                | Action::Backspace
                | Action::DeleteChar
                | Action::DeleteToStart
                | Action::DeleteToEnd
                | Action::DeleteWord => {
                    self.panel_mut(side).search_edit(action);
                    None
                }
                Action::QuickSearch => {
                    self.quick_search();
                    None
                }
                // As in vim's search, Ctrl+r types the text of a register.
                Action::PasteRegister => {
                    let text = self.registers.text(self.extra.char).unwrap_or_default();
                    let text: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
                    for c in text {
                        self.panel_mut(side).search_type(c);
                    }
                    None
                }
                Action::NextMatch | Action::PrevMatch => {
                    let forward = action == Action::NextMatch;
                    self.panel_mut(side).search_step(forward);
                    None
                }
                // As vim's Enter, it says when the search went round or found nothing.
                Action::Cancel => {
                    if let Some(end) = self.end_search(side) {
                        self.message = match end.found {
                            None => Some(fl!("message-search-not-found", text = end.text)),
                            Some(true) if end.back => Some(fl!("message-search-top")),
                            Some(true) => Some(fl!("message-search-bottom")),
                            Some(false) => None,
                        };
                    }
                    None
                }
                // What it found is not kept, as vim keeps no pattern its Esc gives up.
                Action::Abort => {
                    self.panel_mut(side).abort_search();
                    None
                }
                // Any other key ends the search, then does what it does.
                _ => {
                    self.end_search(side);
                    Some(action)
                }
            },
            Resolved::Action(action) => Some(action),
        }
    }

    /// Starts quick search in the active panel, or, as Ctrl+g in it, jumps to the next match;
    /// with nothing typed yet, searches for what the last search found, as mc's Ctrl+s Ctrl+s
    /// does.
    pub(super) fn quick_search(&mut self) {
        let input = self.search_opens(Slot::QuickSearch);
        let last = self.last_search.clone();
        let count = self.extra.times();
        let panel = self.panel_mut(self.active);
        let typed = panel.search_text().is_some_and(|text| !text.is_empty());
        match last {
            Some((last, _, input)) if panel.searching() && !typed => {
                panel.search_resume(&last, input);
            }
            _ if panel.searching() => panel.search_step(true),
            _ => panel.search_down(count, input),
        }
    }

    /// Moves the cursor of the active panel to the next name, or the previous one, that matches
    /// what the last quick search found, `next` the way it went; where it was goes to the jump
    /// list.
    pub(super) fn search_again(&mut self, next: bool) {
        let Some((last, back, input)) = self.last_search.clone() else {
            self.message = Some(fl!("message-search-none"));
            return;
        };
        let forward = next != back;
        let times = self.extra.times();
        let round = self
            .panel_mut(self.active)
            .search_again((&last, input), forward, times);
        self.message = match round {
            None => Some(fl!("message-search-not-found", text = last)),
            Some(true) if forward => Some(fl!("message-search-bottom")),
            Some(true) => Some(fl!("message-search-top")),
            Some(false) => None,
        };
    }

    /// The side of the panel drawn on the left, or on the right: swapped panels are drawn on
    /// each other's sides.
    pub(super) fn drawn_on(&self, left: bool) -> Side {
        if left == self.swapped {
            Side::Right
        } else {
            Side::Left
        }
    }

    /// Ends quick search in the panel on `side`, and keeps what it searched for for `n` and
    /// `N`, with the way it went, found or not, as vim keeps its pattern; how it ended.
    pub(super) fn end_search(&mut self, side: Side) -> Option<SearchEnd> {
        let end = self.panel_mut(side).end_search()?;
        self.last_search = Some((end.text.clone(), end.back, end.input));
        Some(end)
    }
}
