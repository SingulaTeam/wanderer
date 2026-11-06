//! Talking to the daemon.
//!
//! On Linux it listens on a Unix socket, on Windows on a named pipe; the
//! lines on either are the same.

use std::net::Ipv4Addr;
use std::path::PathBuf;

use tokio::io::{
    AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader, ReadHalf, WriteHalf,
};
use wanderer_tunnel::ipc::{MAX_LINE, PROTOCOL_VERSION, Reply, Request, Status};
use wanderer_tunnel::{GuardPolicy, TunnelPlan};

use crate::engine::Engine;

#[cfg(unix)]
type Stream = tokio::net::UnixStream;
#[cfg(windows)]
type Stream = tokio::net::windows::named_pipe::NamedPipeClient;

/// Why the daemon did not do what it was asked.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    /// The daemon is not running, or its socket cannot be reached.
    #[error("the Wanderer service is not running ({0})")]
    Unreachable(#[source] std::io::Error),
    /// The daemon speaks another version of the protocol.
    #[error("the Wanderer service is version {0}, this app needs {PROTOCOL_VERSION}")]
    Version(u32),
    /// The daemon answered with something unreadable.
    #[error("the Wanderer service answered with something unreadable")]
    Garbled,
    /// The daemon refused or failed; `status` is where it is now.
    #[error("{message}")]
    Refused {
        /// Its reason.
        message: String,
        /// Its state after the failure.
        status: Status,
    },
}

/// A connection to the daemon, opened again whenever it breaks.
pub struct Daemon {
    socket: PathBuf,
    stream: Option<(BufReader<ReadHalf<Stream>>, WriteHalf<Stream>)>,
}

impl Daemon {
    /// The daemon at `socket`; nothing is opened until the first request.
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
            stream: None,
        }
    }

    async fn request(&mut self, request: &Request) -> Result<Status, DaemonError> {
        match self.send(request).await? {
            Reply::Ok(status) => {
                check_version(&status)?;
                Ok(status)
            }
            _ => Err(DaemonError::Garbled),
        }
    }

    /// Sends `request`; a refusal becomes [`DaemonError::Refused`].
    async fn send(&mut self, request: &Request) -> Result<Reply, DaemonError> {
        let line = serde_json::to_vec(request).map_err(|_| DaemonError::Garbled)?;
        // A connection the daemon closed — it restarted, say — fails on use;
        // one fresh attempt tells that apart from a daemon that is gone.
        let reply = if let Ok(reply) = self.exchange(&line).await {
            reply
        } else {
            self.stream = None;
            self.exchange(&line)
                .await
                .map_err(DaemonError::Unreachable)?
        };
        match serde_json::from_slice(&reply).map_err(|_| DaemonError::Garbled)? {
            Reply::Error { message, status } => {
                check_version(&status)?;
                Err(DaemonError::Refused { message, status })
            }
            reply => Ok(reply),
        }
    }

    async fn exchange(&mut self, line: &[u8]) -> std::io::Result<Vec<u8>> {
        if self.stream.is_none() {
            let (read, write) = tokio::io::split(open(&self.socket).await?);
            self.stream = Some((BufReader::new(read), write));
        }
        let Some((read, write)) = self.stream.as_mut() else {
            return Err(std::io::ErrorKind::NotConnected.into());
        };
        write.write_all(line).await?;
        write.write_all(b"\n").await?;

        let mut reply = Vec::new();
        let limit = u64::try_from(MAX_LINE).unwrap_or(u64::MAX);
        read.take(limit).read_until(b'\n', &mut reply).await?;
        if reply.last() != Some(&b'\n') {
            self.stream = None;
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        Ok(reply)
    }
}

impl Engine for Daemon {
    async fn status(&mut self) -> Result<Status, DaemonError> {
        self.request(&Request::Status).await
    }

    async fn connect(
        &mut self,
        plan: &TunnelPlan,
        guard: &GuardPolicy,
    ) -> Result<Status, DaemonError> {
        let request = Request::Connect {
            plan: plan.clone(),
            guard: guard.clone(),
        };
        self.request(&request).await
    }

    async fn hold(&mut self, guard: &GuardPolicy) -> Result<Status, DaemonError> {
        let request = Request::Hold {
            guard: guard.clone(),
        };
        self.request(&request).await
    }

    async fn disconnect(&mut self) -> Result<Status, DaemonError> {
        self.request(&Request::Disconnect).await
    }

    async fn resolve(&mut self, host: &str) -> Result<Vec<Ipv4Addr>, DaemonError> {
        let request = Request::Resolve {
            host: host.to_owned(),
        };
        match self.send(&request).await? {
            Reply::Resolved { addresses } => Ok(addresses),
            _ => Err(DaemonError::Garbled),
        }
    }
}

#[cfg(unix)]
async fn open(socket: &std::path::Path) -> std::io::Result<Stream> {
    tokio::net::UnixStream::connect(socket).await
}

#[cfg(windows)]
async fn open(pipe: &std::path::Path) -> std::io::Result<Stream> {
    use tokio::net::windows::named_pipe::ClientOptions;
    // ERROR_PIPE_BUSY: every instance is taken for the moment; the daemon
    // opens the next one at once.
    const PIPE_BUSY: i32 = 231;
    loop {
        match ClientOptions::new().open(pipe) {
            Err(e) if e.raw_os_error() == Some(PIPE_BUSY) => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            opened => return opened,
        }
    }
}

fn check_version(status: &Status) -> Result<(), DaemonError> {
    if status.version == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(DaemonError::Version(status.version))
    }
}

// Two attributes, not `all(test, unix)`: clippy recognises a test module by a
// plain `cfg(test)`, and only then lets its tests unwrap.
#[cfg(test)]
#[cfg(unix)]
mod tests {
    use tokio::net::UnixListener;

    use wanderer_tunnel::ipc::{State, Traffic};

    use super::*;

    fn status(version: u32) -> Status {
        Status {
            version,
            state: State::Holding,
            traffic: Traffic::default(),
            xray_exit: None,
        }
    }

    /// A daemon that answers each connection's first request with `reply`,
    /// then hangs up, as a restarting daemon would.
    fn daemon(reply: Reply) -> (tempdir::Dir, Daemon) {
        let dir = tempdir::Dir::new();
        let path = dir.0.join("d.sock");
        let listener = UnixListener::bind(&path).unwrap();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (read, mut write) = stream.into_split();
                let mut line = String::new();
                BufReader::new(read).read_line(&mut line).await.unwrap();
                serde_json::from_str::<Request>(&line).unwrap();
                let mut out = serde_json::to_vec(&reply).unwrap();
                out.push(b'\n');
                write.write_all(&out).await.unwrap();
            }
        });
        (dir, Daemon::new(path))
    }

    #[tokio::test]
    async fn reconnects_when_the_daemon_hung_up() {
        let (_dir, mut d) = daemon(Reply::Ok(status(PROTOCOL_VERSION)));
        assert_eq!(d.status().await.unwrap().state, State::Holding);
        assert_eq!(d.status().await.unwrap().state, State::Holding);
    }

    #[tokio::test]
    async fn refuses_another_protocol_version() {
        let (_dir, mut d) = daemon(Reply::Ok(status(PROTOCOL_VERSION + 1)));
        assert!(matches!(d.status().await, Err(DaemonError::Version(_))));
    }

    #[tokio::test]
    async fn passes_a_refusal_on_with_the_state() {
        let (_dir, mut d) = daemon(Reply::Error {
            message: "invalid node".into(),
            status: status(PROTOCOL_VERSION),
        });
        match d.disconnect().await {
            Err(DaemonError::Refused { message, status }) => {
                assert_eq!(
                    (message.as_str(), status.state),
                    ("invalid node", State::Holding)
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn says_when_the_daemon_is_not_running() {
        let mut d = Daemon::new("/nonexistent/wanderer.sock");
        assert!(matches!(d.status().await, Err(DaemonError::Unreachable(_))));
    }

    mod tempdir {
        /// A directory removed when dropped.
        pub(super) struct Dir(pub(super) std::path::PathBuf);

        impl Dir {
            pub(super) fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "wanderer-test-{}-{}",
                    std::process::id(),
                    rand::random::<u64>()
                ));
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
