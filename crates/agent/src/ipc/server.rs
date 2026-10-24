//! The desktop-role Unix socket server.

use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use reopen_protocol::ErrorCode;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Semaphore, watch};
use tokio::time::timeout;
use uuid::Uuid;

use super::frame::{FrameError, read_frame, write_frame};
use super::handler::{DesktopHandler, Failure, error_frame};

/// Concurrent client connections; further connections are closed immediately.
pub const MAX_CONNECTIONS: usize = 8;
/// A connection that sends nothing for this long is closed.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Binds the socket with owner-only permissions. A stale socket is removed
/// first; the caller must hold the agent lock so no live agent owns it.
///
/// # Errors
///
/// I/O errors, or `AlreadyExists` if the path is something other than a socket.
pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => std::fs::remove_file(path)?,
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "socket path is occupied by another file",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(path)?;
    // The runtime directory is already 0700; this keeps the socket private too.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Accepts connections until shutdown. Only processes running as this user
/// (checked with `SO_PEERCRED`) are served.
pub async fn serve(
    listener: UnixListener,
    handler: Arc<DesktopHandler>,
    mut shutdown: watch::Receiver<bool>,
) {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let uid = rustix::process::getuid().as_raw();
    loop {
        let stream = tokio::select! {
            biased;
            _ = shutdown.changed() => return,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(_) => continue,
            },
        };
        if !matches!(stream.peer_cred(), Ok(cred) if cred.uid() == uid) {
            tracing::warn!(event = "ipc.peer_rejected");
            continue;
        }
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            tracing::warn!(event = "ipc.connections_exhausted");
            continue;
        };
        let handler = Arc::clone(&handler);
        tokio::spawn(async move {
            connection(stream, &handler).await;
            drop(permit);
        });
    }
}

/// Serves one connection: one request at a time, so at most one is outstanding.
async fn connection(mut stream: UnixStream, handler: &DesktopHandler) {
    loop {
        let frame = match timeout(IDLE_TIMEOUT, read_frame(&mut stream)).await {
            Ok(Ok(Some(frame))) => frame,
            Ok(Err(FrameError::TooLarge(_))) => {
                let failure = Failure {
                    code: ErrorCode::LimitExceeded,
                    retryable: false,
                    message: "message too large",
                };
                let _ = write_frame(&mut stream, &error_frame(Uuid::nil(), &failure)).await;
                return;
            }
            // Clean close, I/O error or idle timeout.
            Ok(Ok(None) | Err(FrameError::Io(_))) | Err(_) => return,
        };
        let response = handler.handle(&frame).await;
        if write_frame(&mut stream, &response).await.is_err() {
            return;
        }
    }
}
