use super::*;

#[test]
fn a_failure_shows_what_failed_the_path_and_why_under_a_line() {
    let path = "/home/user/projects/far/too/deep/for/the/dialog/to/show/whole/report.pdf";
    let reason = Reason {
        text: "Permission denied".to_owned(),
        code: Some(Code::Os(13)),
    };
    let problem = Problem::new("Cannot read".to_owned(), reason).at(path.to_owned());
    let buttons = vec![Button::Retry, Button::Skip, Button::Abort];
    let dialog = Dialog::failure(&problem, buttons, 0);
    insta::assert_snapshot!(draw(&dialog, 60, 12).backend());
}

#[test]
fn a_failure_of_a_job_asks_about_the_rest_between_lines() {
    let reason = Reason {
        text: "No space left on device".to_owned(),
        code: Some(Code::Os(28)),
    };
    let problem = Problem::new("Cannot write to".to_owned(), reason).at("/mnt/a".to_owned());
    let buttons = vec![Button::Retry, Button::Skip, Button::Abort];
    let rests = [
        "Ask each time".to_owned(),
        "Skip every failure".to_owned(),
        "Skip failures with the same code".to_owned(),
    ];
    let dialog = Dialog::failure(&problem, buttons, 0)
        .with_line()
        .with_combo_box("&The rest:", &rests, 0)
        .with_focus_on_default();
    insta::assert_snapshot!(draw(&dialog, 60, 14).backend());
}

#[test]
fn a_long_reason_wraps_under_the_line() {
    let reason = "ssh: connect to host db.example.com port 22: Operation timed out";
    let problem = Problem::new("Cannot connect to db".to_owned(), reason.to_owned());
    let dialog = Dialog::failure_titled(
        "Connection",
        &problem,
        vec![Button::Retry, Button::Cancel],
        0,
    );
    insta::assert_snapshot!(draw(&dialog, 40, 12).backend());
}
