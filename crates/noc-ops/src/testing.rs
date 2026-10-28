//! What the tests of jobs share.

use std::path::Path;
use std::process::Stdio;

use noc_vfs::SftpFs;

/// A local `sftp-server` that starts in `dir`, if there is one, and a session with it. The
/// server goes when the child is dropped.
pub(crate) async fn sftp_server(dir: &Path) -> Option<(tokio::process::Child, SftpFs)> {
    let mut command = tokio::process::Command::new(program()?);
    command.arg("-e").arg("-d").arg(dir);
    Some(connect(command).await)
}

/// The same, but a server that refuses to write files beyond `blocks` of what the shell's
/// `ulimit -f` counts in: 1 KiB, or 512 bytes in some shells.
pub(crate) async fn sftp_server_with_file_limit(
    dir: &Path,
    blocks: u32,
) -> Option<(tokio::process::Child, SftpFs)> {
    let mut command = tokio::process::Command::new("/bin/sh");
    command
        .arg("-c")
        // Without the trap, the signal for a file too big ends the server.
        .arg(r#"trap '' XFSZ; ulimit -f "$1" && exec "$2" -e -d "$3""#)
        .arg("sh")
        .arg(blocks.to_string())
        .arg(program()?)
        .arg(dir);
    Some(connect(command).await)
}

fn program() -> Option<&'static Path> {
    ["/usr/libexec/sftp-server", "/usr/lib/openssh/sftp-server"]
        .into_iter()
        .map(Path::new)
        .find(|path| path.exists())
}

async fn connect(mut command: tokio::process::Command) -> (tokio::process::Child, SftpFs) {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let fs = SftpFs::from_pipes(stdin, stdout).await.unwrap();
    (child, fs)
}
