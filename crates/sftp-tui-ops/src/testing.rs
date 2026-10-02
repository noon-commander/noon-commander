//! What the tests of jobs share.

use std::path::Path;
use std::process::Stdio;

use sftp_tui_vfs::SftpFs;

/// A local `sftp-server` that starts in `dir`, if there is one, and a session with it. The
/// server goes when the child is dropped.
pub(crate) async fn sftp_server(dir: &Path) -> Option<(tokio::process::Child, SftpFs)> {
    let program = ["/usr/libexec/sftp-server", "/usr/lib/openssh/sftp-server"]
        .into_iter()
        .map(Path::new)
        .find(|path| path.exists())?;
    let mut child = tokio::process::Command::new(program)
        .arg("-e")
        .arg("-d")
        .arg(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let fs = SftpFs::from_pipes(stdin, stdout).await.unwrap();
    Some((child, fs))
}
