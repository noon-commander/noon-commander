//! The desktop's opener, which opens a file or a directory as a double click does, with the
//! program the desktop has for it: `open` on macOS, `xdg-open` elsewhere.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{ToolError, process};

/// How long the opener may take to say whether it opened a file. It hands the file to another
/// program and exits at once, but `xdg-open` may wait for that program instead; past this, the
/// file counts as open.
const TIMEOUT: Duration = Duration::from_secs(5);

/// How to run the opener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opener {
    /// A name looked up in `PATH`, or a path.
    program: PathBuf,
}

impl Default for Opener {
    fn default() -> Self {
        if cfg!(target_os = "macos") {
            Self::new("open")
        } else {
            Self::new("xdg-open")
        }
    }
}

impl Opener {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Opens `path`, an absolute path: `open <path>`. `xdg-open` takes no `--`, so the path
    /// goes without one; being absolute, it is never taken for an option. The program that
    /// opens it runs on after Noon Commander quits.
    pub async fn open(&self, path: &Path) -> Result<(), ToolError> {
        debug_assert!(path.is_absolute());
        process::detached(&self.program, [path], TIMEOUT).await
    }
}
