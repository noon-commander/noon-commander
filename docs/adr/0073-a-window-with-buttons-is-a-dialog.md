# 0073. A window with buttons is a dialog

- Status: Accepted
- Date: 2026-11-10
- Amends: [ADR 0061](0061-a-picker-is-a-part-of-a-dialog.md), [ADR 0063](0063-errors-in-parts-and-the-order-of-buttons.md)

## Context

Three windows had buttons but were no `Dialog`: the window of a job (`JobView`), the list of jobs (`JobsList`), and the window over the viewer while it reads a part that takes a while (`Reading`). Each drew its own box, its own line over the buttons, and its own buttons, and had its own focus of the buttons, its own keys, and its own mouse, beside those of every dialog. So none had what a dialog gains: a count before a motion, Home and End on the buttons, a double click on a row, what the parts of a dialog learn next. The window of checksums was such a window too, until it became a dialog ([ADR 0061](0061-a-picker-is-a-part-of-a-dialog.md)).

`Reading` had Stop, the default, and Close, which closed the viewer, and Esc pressed Stop, the first button, where every other dialog has its way out last, pressed by Esc ([ADR 0063](0063-errors-in-parts-and-the-order-of-buttons.md)). The list of jobs moved its rows with Up and Down while the focus was on a button, and Left and Right moved the focus of the buttons, unlike the rows of a picker in a dialog.

## Decision

- **A window with buttons is a `Dialog`**, which a type of the window wraps, as `SumsWindow` and `FindWindow` do: the wrapper keeps what is its own, such as the time left of a job, which stays as it is ([ADR 0056](0056-the-time-left-of-a-job-stays-as-it-is.md)), and gives the dialog its lines; the dialog draws them, the frame, the line, and the buttons, and takes the keys and the mouse. Windows without buttons stay windows of their own: the help, the Configuration dialog, which has no OK ([ADR 0009](0009-configuration-dialog-writes-config-toml.md)), the pull-down menu, and the lists.
- **A gauge is a part of a dialog** (`Text::Gauge`): a bar filled for how far something is, with its percentage, or, while that is not known, the faint bar that a shine crosses. The shine comes from the owner's wait, given at each frame (`Dialog::render_shining`). Lines of text change in place while the dialog is open (`Dialog::replace_text`), so that it keeps its focus and its id.
- **The window of a read has Cancel alone**: Enter, Space, Esc, and F10 press it, and it stops the read and keeps what was read. Cancel, as it gives up one thing, the part, and changes nothing; Abort is for a job, whose work done stays. Close goes: Esc closes the viewer once the window is gone.
- **The window of a job is as it was**: what it does, the entry at hand, the gauge, the count, and the time, each on a line, then Background, the default, and Abort, which Esc presses; Abort alone for a job of F4.
- **The list of jobs is a picker without a filter in a dialog**, with Show, the default, Abort, and OK, which Esc presses. Its rows start right under the title, with no line over them (`Picker::bare`), as they did. They are a stop of the focus as in every dialog: there the motions move the cursor, PageUp and PageDown a page, and Tab, Left, and Right go to the buttons; Enter, Space, and a double click on a row press Show; Alt and a letter press a button from anywhere. The wheel scrolls the rows wherever the focus is. The cursor stays on its job as jobs come and go, and on the row after it once it ends.
- **A count before a motion goes to the dialog**, which counts its moves itself, in these windows too.

## Consequences

- One way to draw buttons, move between them, and press them, in every window that has them.
- The window of a read is the same size, with one button. Closing the viewer while it reads takes Esc twice.
- The list of jobs looks as it did. Abort for the job under the cursor is Right, Right, Enter, or Alt+a, where it was Right, Enter; Up and Down on a button move the focus, not the cursor.
- The rule of the way out holds for the window of a read: its only button is its way out.
