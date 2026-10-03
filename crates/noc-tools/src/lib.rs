//! External programs that Noon Commander works with, other than ssh.
//!
//! This crate owns every command line of a helper program: zoxide, which ranks the directories
//! the user works in, and the editor of F4. ssh has rules of its own and lives in `noc-ssh`.
//! Programs run without a shell, with their arguments after `--` where they take paths, and
//! background ones end when their future is dropped.

pub mod editor;
mod error;
mod process;
pub mod zoxide;

pub use error::ToolError;
