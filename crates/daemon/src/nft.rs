//! Applying the guard's scripts with `nft`.

use std::process::Stdio;

use tokio::io::AsyncWriteExt as _;
use tokio::process::Command;

/// Why a script did not apply.
#[derive(Debug, thiserror::Error)]
pub(crate) enum NftError {
    /// `nft` could not be run at all.
    #[error("cannot run nft: {0}")]
    Spawn(#[from] std::io::Error),
    /// `nft` refused the script; nothing of it applied.
    #[error("nft refused the rules: {0}")]
    Refused(String),
}

/// Runs `script` as one nftables transaction: all of it applies, or none.
pub(crate) async fn apply(script: &str) -> Result<(), NftError> {
    let mut child = Command::new("nft")
        .args(["-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes()).await?;
    }
    let out = child.wait_with_output().await?;
    if out.status.success() {
        Ok(())
    } else {
        Err(NftError::Refused(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ))
    }
}
