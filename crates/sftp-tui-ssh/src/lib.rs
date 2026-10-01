//! System OpenSSH integration for sftp-tui.
//!
//! This crate owns every `ssh` command line: host discovery, `ssh -G` resolution,
//! validation of user-supplied arguments, the per-host `ControlMaster` connection,
//! SFTP channels, and the askpass bridge. Only [`policy`] exists so far.

pub mod policy;
