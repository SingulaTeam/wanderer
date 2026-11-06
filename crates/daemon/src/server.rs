//! The control socket.
//!
//! Any local user may connect — as with any VPN client on a desktop, whoever
//! sits at the machine may turn the tunnel on and off — so what a request can
//! do is limited by its type, not by who sends it: there is no request that
//! carries a configuration, a path or a command.
//!
//! The connection that last asked for a tunnel or a hold is the app. When it
//! closes — the app quit or crashed — and no other takes its place within a
//! grace period, the tunnel and the guard come down: a session nobody reports
//! on is swept by its Control Plane within minutes, and a tunnel left to it
//! would hold the machine offline with nothing on screen to turn it off.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::io::{
    AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, BufReader,
};
use tokio::sync::Mutex;
use wanderer_tunnel::ipc::{MAX_LINE, Reply, Request};

use crate::engine::Engine;

/// Listens on the Unix socket at `socket` and serves connections until the
/// listener fails. `grace` is how long the tunnel outlives the app that
/// asked for it; `None` keeps it up regardless.
#[cfg(unix)]
pub(crate) async fn serve(
    socket: &Path,
    engine: Arc<Mutex<Engine>>,
    grace: Option<Duration>,
) -> Result<(), String> {
    let listener = unix::listen(socket)?;
    let next = AtomicU64::new(1);
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let id = next.fetch_add(1, Ordering::Relaxed);
                let peer = stream.peer_cred().ok().map(|c| c.uid());
                tokio::spawn(client(stream, peer, Arc::clone(&engine), id, grace));
            }
            Err(e) => tracing::warn!("accept: {e}"),
        }
    }
}

/// Listens on the named pipe at `pipe` and serves connections until creating
/// the next instance fails. `grace` is as for Unix.
#[cfg(windows)]
pub(crate) async fn serve(
    pipe: &Path,
    engine: Arc<Mutex<Engine>>,
    grace: Option<Duration>,
) -> Result<(), String> {
    use crate::windows::pipe;
    let fail = |e: std::io::Error| format!("cannot listen on {}: {e}", pipe.display());
    let mut waiting = pipe::create(pipe, true).map_err(fail)?;
    let next = AtomicU64::new(1);
    loop {
        if let Err(e) = waiting.connect().await {
            tracing::warn!("accept: {e}");
            continue;
        }
        // A new instance before the next client knocks, or it finds none.
        let connected = std::mem::replace(&mut waiting, pipe::create(pipe, false).map_err(fail)?);
        let id = next.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(client(connected, None, Arc::clone(&engine), id, grace));
    }
}

/// Removes what serving left behind.
pub(crate) fn cleanup(socket: &Path) {
    #[cfg(unix)]
    let _ = std::fs::remove_file(socket);
    #[cfg(windows)]
    let _ = socket; // a pipe goes with its last handle
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    use tokio::net::UnixListener;

    pub(super) fn listen(socket: &Path) -> Result<UnixListener, String> {
        let fail = |what: &str, e: std::io::Error| format!("{what} {}: {e}", socket.display());
        if let Some(dir) = socket.parent() {
            std::fs::create_dir_all(dir).map_err(|e| fail("cannot create the directory of", e))?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| fail("cannot open the directory of", e))?;
        }
        match std::fs::remove_file(socket) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(fail("cannot replace", e));
            }
            _ => {}
        }
        let listener = UnixListener::bind(socket).map_err(|e| fail("cannot listen on", e))?;
        // Every local user may reach it; see above for why that is safe.
        std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o666))
            .map_err(|e| fail("cannot open", e))?;
        Ok(listener)
    }
}

async fn client(
    stream: impl AsyncRead + AsyncWrite + Send + 'static,
    peer: Option<u32>,
    engine: Arc<Mutex<Engine>>,
    id: u64,
    grace: Option<Duration>,
) {
    tracing::debug!(?peer, "client connected");
    let (read, mut write) = tokio::io::split(stream);
    let mut read = BufReader::new(read);
    let mut line = Vec::new();
    loop {
        line.clear();
        // One byte over the limit tells an overlong line from one that fits.
        let limit = u64::try_from(MAX_LINE + 1).unwrap_or(u64::MAX);
        match (&mut read).take(limit).read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) if line.len() > MAX_LINE => {
                tracing::warn!(?peer, "request too long; closing");
                break;
            }
            Ok(_) => {}
        }
        let reply = match serde_json::from_slice::<Request>(&line) {
            Ok(request) => handle(&engine, request, id).await,
            Err(e) => {
                let status = engine.lock().await.status();
                Reply::Error {
                    message: format!("unreadable request: {e}"),
                    status,
                }
            }
        };
        let Ok(mut out) = serde_json::to_vec(&reply) else {
            break;
        };
        out.push(b'\n');
        if write.write_all(&out).await.is_err() {
            break;
        }
    }
    tracing::debug!(?peer, "client gone");

    if let Some(grace) = grace {
        if engine.lock().await.controlled_by(id) {
            tokio::spawn(abandon(engine, id, grace));
        }
    }
}

/// Takes the tunnel and the guard down if the app that asked for them has
/// not come back within `grace`.
async fn abandon(engine: Arc<Mutex<Engine>>, id: u64, grace: Duration) {
    tracing::info!(
        "the app went away; waiting {}s for it to come back",
        grace.as_secs()
    );
    tokio::time::sleep(grace).await;
    let mut engine = engine.lock().await;
    if engine.controlled_by(id) {
        tracing::warn!("the app did not come back; taking the tunnel and the guard down");
        let _ = engine.disconnect().await;
    }
}

async fn handle(engine: &Mutex<Engine>, request: Request, id: u64) -> Reply {
    match request {
        // Needs nothing of the engine's, so it does not wait on a connect.
        Request::Resolve { host } => resolve(engine, &host).await,
        Request::Status => Reply::Ok(engine.lock().await.status()),
        Request::Connect { plan, guard } => {
            let mut engine = engine.lock().await;
            engine.claim(id);
            engine.connect(&plan, &guard).await
        }
        Request::Hold { guard } => {
            let mut engine = engine.lock().await;
            engine.claim(id);
            engine.hold(&guard).await
        }
        Request::Disconnect => engine.lock().await.disconnect().await,
    }
}

async fn resolve(engine: &Mutex<Engine>, host: &str) -> Reply {
    let failed = |message: String| async move {
        Reply::Error {
            message,
            status: engine.lock().await.status(),
        }
    };
    if !wanderer_tunnel::plan::is_host_name(host) {
        return failed("invalid host name".into()).await;
    }
    match crate::resolve::resolve(host).await {
        Ok(addresses) => Reply::Resolved { addresses },
        Err(e) => failed(format!("cannot resolve {host}: {e}")).await,
    }
}
