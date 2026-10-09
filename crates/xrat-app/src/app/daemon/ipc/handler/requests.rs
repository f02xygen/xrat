use super::*;

#[cfg(unix)]
pub async fn serve_ping(
    socket_path: &Path,
    supervisor_tx: mpsc::Sender<SupervisorEvent>,
) -> crate::app::Result<()> {
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if socket_path.exists() {
        match crate::app::daemon::ipc::ping_daemon(socket_path).await {
            Ok(_) => {
                return Err(crate::app::AppError::InvalidArgument(format!(
                    "daemon is already running at {}; stop it first",
                    socket_path.display()
                )));
            }
            Err(err) if daemon_unreachable(&err) => {
                if let Err(error) = std::fs::remove_file(socket_path) {
                    tracing::warn!(path = %socket_path.display(), %error, "failed to remove stale daemon socket");
                }
            }
            Err(err) => return Err(err),
        }
    }

    let listener = UnixListener::bind(socket_path)?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let shutdown_signal = async {
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    };
    tokio::pin!(shutdown_signal);
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
    loop {
        tokio::select! {
            biased;
            _ = shutdown_rx.recv() => break,
            result = &mut shutdown_signal => {
                result?;
                crate::app::daemon::ipc::transport::daemon_shutdown_response_via_supervisor(supervisor_tx.clone()).await?;
                break;
            }
            accept_result = listener.accept() => {
                let (mut stream, _) = accept_result?;
                let supervisor_tx = supervisor_tx.clone();
                let shutdown_tx = shutdown_tx.clone();
                tokio::spawn(async move {
                    if let Err(error) = io::handle_connection(&mut stream, supervisor_tx, shutdown_tx).await {
                        tracing::debug!(%error, "daemon IPC request failed");
                    }
                });
            }
        }
    }
    if let Err(error) = std::fs::remove_file(socket_path) {
        tracing::warn!(path = %socket_path.display(), %error, "failed to remove daemon socket");
    }
    Ok(())
}

#[cfg(not(unix))]
pub async fn serve_ping(
    _socket_path: &Path,
    _supervisor_tx: mpsc::Sender<SupervisorEvent>,
) -> crate::app::Result<()> {
    Err(crate::app::AppError::InvalidArgument(
        "daemon IPC server is not supported on this platform yet".to_string(),
    ))
}
