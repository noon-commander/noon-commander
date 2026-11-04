use std::time::{Duration, Instant, UNIX_EPOCH};

use noc_scroll::Motion;
use noc_vfs::{FileKind, Metadata};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::style::{Color, Modifier, Style};

use super::*;
use crate::tui::keymap::Mode;
use crate::tui::mouse::Scroll;
use crate::tui::search::{Case, Kind, Switch, Way};
use crate::tui::spinner;

mod listing;
mod marks;
mod moving;
mod quick_search;
mod rename;
mod render;
mod sorting;

/// Quick search in `way`, with smart case.
fn quick(way: Way) -> Input {
    Input::new(Kind::QUICK_SEARCH, way)
}

/// 2023-11-14 22:13:20 UTC.
const NOW: u64 = 1_700_000_000;

const HOME: &str = "/home/me";

fn entry(name: &str, kind: FileKind, size: u64) -> DirEntry {
    DirEntry {
        name: name.as_bytes().to_vec(),
        metadata: Metadata {
            kind,
            size: Some(size),
            permissions: Some(0o644),
            modified: Some(UNIX_EPOCH + Duration::from_secs(NOW - 3600)),
            uid: Some(501),
            gid: Some(20),
            inode: None,
        },
        target_kind: None,
        link_target: None,
    }
}

fn link_to_dir(name: &str) -> DirEntry {
    DirEntry {
        target_kind: Some(FileKind::Dir),
        link_target: Some(b"/opt".to_vec()),
        ..entry(name, FileKind::Symlink, 4)
    }
}

fn listing() -> Vec<DirEntry> {
    vec![
        entry("zeta.txt", FileKind::File, 12_345),
        entry("Beta", FileKind::Dir, 4096),
        entry(".hidden", FileKind::File, 1),
        link_to_dir("alpha-link"),
        entry("Alpha.md", FileKind::File, 10_000_000),
        entry("bin", FileKind::Dir, 4096),
    ]
}

fn local(path: &str) -> Location {
    Location::Local(PathBuf::from(path))
}

fn remote(host: &str, path: &str) -> Location {
    Location::Remote {
        host: host.to_owned(),
        path: RemotePath::from(path),
    }
}

/// Answers `request` with `listing` from the location it asked for.
fn answer(panel: &mut Panel, request: &ListRequest, listing: Listing) {
    let location = request.location.clone();
    panel.listed(
        request.generation,
        Ok(Listed {
            location,
            listing,
            space: None,
        }),
    );
}

/// A panel on `location` whose first listing arrived.
fn loaded_at(location: Location, listing: Listing) -> Panel {
    let (mut panel, request) = Panel::new(location, PathBuf::from(HOME), true);
    answer(&mut panel, &request, listing);
    panel
}

fn loaded(path: &str, entries: Vec<DirEntry>) -> Panel {
    loaded_at(local(path), Listing::Dir(entries))
}

fn names(panel: &Panel) -> Vec<String> {
    (0..panel.rows())
        .map(|row| match panel.row(row) {
            Some(Row::Parent) => "..".to_owned(),
            Some(Row::Entry(entry)) => entry.display_name().into_owned(),
            None => unreachable!(),
        })
        .collect()
}

fn under_cursor(panel: &Panel) -> String {
    names(panel)[panel.cursor].clone()
}

/// Draws `panel` with mc's markers, or icons with `decor`, in the `terminal` theme.
fn render_with(
    panel: &mut Panel,
    (width, height): (u16, u16),
    active: bool,
    hosts: &dyn Fn(&str) -> HostState,
    decor: Decor,
) -> Terminal<TestBackend> {
    render_themed(
        panel,
        (width, height),
        active,
        hosts,
        decor,
        &Theme::terminal(),
    )
}

fn render_themed(
    panel: &mut Panel,
    (width, height): (u16, u16),
    active: bool,
    hosts: &dyn Fn(&str) -> HostState,
    decor: Decor,
    theme: &Theme,
) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    // Drawn again once what it waits for shows, with a still spinner.
    let start = Instant::now();
    for at in [start, start + spinner::DELAY] {
        let view = View {
            hosts,
            decor,
            theme,
            spin: Spin::new(at, start, false, theme.glyphs.spinners),
            tz: &TimeZone::UTC,
            formats: &Formats::iso(),
        };
        terminal
            .draw(|frame| panel.render(frame, frame.area(), active, &view))
            .unwrap();
    }
    terminal
}

fn render(
    panel: &mut Panel,
    width: u16,
    height: u16,
    active: bool,
    hosts: &dyn Fn(&str) -> HostState,
) -> TestBackend {
    let terminal = render_with(panel, (width, height), active, hosts, Decor::new(false));
    terminal.backend().clone()
}

fn draw(panel: &mut Panel, width: u16, height: u16, active: bool) -> TestBackend {
    render(panel, width, height, active, &|_| HostState::default())
}

/// Entries whose names, extensions, times, and sizes all sort differently.
fn varied() -> Vec<DirEntry> {
    let at = |seconds: u64| Some(UNIX_EPOCH + Duration::from_secs(NOW - 1000 + seconds));
    let file = |name: &str, size: u64, seconds: u64| DirEntry {
        metadata: Metadata {
            modified: at(seconds),
            ..entry(name, FileKind::File, size).metadata
        },
        ..entry(name, FileKind::File, size)
    };
    let dir = |name: &str, seconds: u64| DirEntry {
        metadata: Metadata {
            modified: at(seconds),
            ..entry(name, FileKind::Dir, 4096).metadata
        },
        ..entry(name, FileKind::Dir, 4096)
    };
    vec![
        file("c.txt", 20, 2),
        dir("dir1", 0),
        file("b.md", 10, 1),
        file("Z", 5, 5),
        dir("Adir", 4),
        file("a.txt", 30, 3),
        file(".env", 1, 6),
    ]
}

fn marked(panel: &Panel) -> Vec<String> {
    let mut names: Vec<String> = panel
        .marked
        .iter()
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect();
    names.sort();
    names
}

/// The bottom line of `panel`'s frame drawn `width` cells wide and 6 high.
fn bottom(panel: &mut Panel, width: u16) -> String {
    let backend = draw(panel, width, 6, true);
    let buffer = backend.buffer();
    (0..width)
        .map(|x| buffer[(x, 5)].symbol().to_owned())
        .collect()
}
