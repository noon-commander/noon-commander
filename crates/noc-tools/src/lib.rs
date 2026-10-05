//! External programs that Noon Commander works with, other than ssh.
//!
//! This crate owns every command line of a helper program: zoxide, which ranks the directories
//! the user works in, the editor of F4, and the shell of the command line. ssh has rules of its
//! own and lives in `noc-ssh`. Programs run without a shell, with their arguments after `--`
//! where they take paths, and background ones end when their future is dropped; the shell of
//! the command line is the one exception to the first rule (ADR 0019).

pub mod editor;
mod error;
mod process;
pub mod shell;
pub mod zoxide;

pub use error::ToolError;
