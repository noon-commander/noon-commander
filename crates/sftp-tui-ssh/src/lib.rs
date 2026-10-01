//! System OpenSSH integration for sftp-tui.
//!
//! This crate owns every `ssh` command line: host discovery, `ssh -G` resolution,
//! validation of user-supplied arguments, the per-host `ControlMaster` connection,
//! SFTP channels, and the askpass bridge.

pub mod args;
pub mod askpass;
mod command;
pub mod discovery;
mod error;
pub mod pattern;
pub mod policy;
pub mod resolve;
mod resolve_cache;
mod runtime;
mod session;
pub mod version;

pub use command::{SshSettings, Target};
pub use error::SshError;
pub use resolve_cache::{CachedHost, ConfigStamp, ResolveCache};
pub use runtime::cleanup_stale;
pub use session::{ChannelProcess, Session, SftpChannel, StderrTail};
