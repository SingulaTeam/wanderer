//! The signed-in wallet, which lives in the webview.
//!
//! thirdweb holds the key — an in-app wallet, or a phone through
//! `WalletConnect` — so the core asks the webview to sign: it emits
//! `wallet:sign` with an id, and the webview answers through the
//! `wallet_signed` command. The SDK checks each signature against the
//! address before it uses it, so a webview that answered wrongly fails the
//! request rather than signing for someone else.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use singula::Address;
use singula::client::wallet::{PersonalMessage, TypedData, Wallet, WalletError, WalletSignature};
use singula::zk::Identity;
use tauri::{AppHandle, Emitter as _};
use tokio::sync::oneshot;

/// How long the person has to approve a signature, on a phone if need be.
const SIGN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

type Answer = Result<String, String>;

/// Signature requests waiting for the webview, by id.
#[derive(Default)]
pub(crate) struct Pending {
    next: AtomicU64,
    waiting: Mutex<HashMap<u64, oneshot::Sender<Answer>>>,
}

impl Pending {
    fn open(&self) -> (u64, oneshot::Receiver<Answer>) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.lock().insert(id, tx);
        (id, rx)
    }

    /// Delivers the webview's answer to request `id`.
    pub(crate) fn answer(&self, id: u64, answer: Answer) {
        if let Some(tx) = self.lock().remove(&id) {
            let _ = tx.send(answer);
        }
    }

    fn forget(&self, id: u64) {
        self.lock().remove(&id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, oneshot::Sender<Answer>>> {
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the webview is asked to sign.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Request {
    Message {
        id: u64,
        purpose: Purpose,
        message: String,
    },
    TypedData {
        id: u64,
        purpose: Purpose,
        typed_data: Value,
    },
}

/// What a signature is for, so the webview can tell the person which step
/// of the flow their wallet is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Purpose {
    /// Deriving the anonymous identity, once per wallet.
    Identity,
    /// Signing in to a Control Plane.
    SignIn,
    /// The purchase itself.
    Purchase,
    /// Extending a subscription with more connections.
    Extend,
    /// Letting the subscription contract take the price.
    Permit,
    /// Starting a subscription.
    Activation,
    /// Joining the anonymity set later.
    Register,
    /// The gasless transaction a Control Plane sends on the wallet's behalf;
    /// wallets show it as a hex digest.
    Relay,
    Other,
}

impl Purpose {
    fn of_message(message: PersonalMessage<'_>) -> Self {
        match message {
            PersonalMessage::Text(Identity::SIGNING_MESSAGE) => Self::Identity,
            PersonalMessage::Text(_) => Self::SignIn,
            PersonalMessage::Raw(_) => Self::Relay,
        }
    }

    fn of_typed_data(data: &Value) -> Self {
        match data["primaryType"].as_str() {
            Some("MintRequest") => Self::Purchase,
            Some("AddSlots") => Self::Extend,
            Some("Permit") => Self::Permit,
            Some("Activation") => Self::Activation,
            Some("Register") => Self::Register,
            _ => Self::Other,
        }
    }
}

/// The wallet the person signed in with, signing through the webview.
pub(crate) struct WebviewWallet {
    address: Address,
    app: AppHandle,
    pending: Arc<Pending>,
}

impl WebviewWallet {
    pub(crate) fn new(address: Address, app: AppHandle, pending: Arc<Pending>) -> Self {
        Self {
            address,
            app,
            pending,
        }
    }

    async fn sign(
        &self,
        request: impl FnOnce(u64) -> Request,
    ) -> Result<WalletSignature, WalletError> {
        let (id, answer) = self.pending.open();
        tracing::info!(id, "asking the webview to sign");
        if let Err(e) = self.app.emit("wallet:sign", request(id)) {
            self.pending.forget(id);
            return Err(WalletError::Failed(e.into()));
        }
        let answer = tokio::time::timeout(SIGN_TIMEOUT, answer).await;
        self.pending.forget(id);
        match &answer {
            Ok(Ok(Ok(_))) => tracing::info!(id, "the webview signed"),
            Ok(Ok(Err(reason))) => tracing::warn!(id, reason, "the webview did not sign"),
            Ok(Err(_)) => tracing::warn!(id, "the request was dropped"),
            Err(_) => tracing::warn!(id, "no answer from the webview in time"),
        }
        match answer {
            Ok(Ok(Ok(signature))) => parse(&signature).ok_or(WalletError::WrongSignature),
            Ok(Ok(Err(reason))) if declined(&reason) => Err(WalletError::Declined),
            Ok(Ok(Err(reason))) => Err(WalletError::Failed(reason.into())),
            // No answer in time, or the webview went away.
            Ok(Err(_)) | Err(_) => Err(WalletError::Declined),
        }
    }
}

impl Wallet for WebviewWallet {
    fn address(&self) -> Address {
        self.address
    }

    async fn sign_message(
        &self,
        message: PersonalMessage<'_>,
    ) -> Result<WalletSignature, WalletError> {
        let purpose = Purpose::of_message(message);
        let message = format!("0x{}", hex(message.bytes()));
        self.sign(|id| Request::Message {
            id,
            purpose,
            message,
        })
        .await
    }

    async fn sign_typed_data(&self, data: &TypedData) -> Result<WalletSignature, WalletError> {
        let typed_data = data.to_json();
        let purpose = Purpose::of_typed_data(&typed_data);
        self.sign(|id| Request::TypedData {
            id,
            purpose,
            typed_data,
        })
        .await
    }
}

/// Wallets word a refusal in their own way; these are the usual ones.
fn declined(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    ["reject", "denied", "declined", "cancel"]
        .iter()
        .any(|w| reason.contains(w))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// A 65-byte signature in hex, with or without `0x`.
fn parse(signature: &str) -> Option<WalletSignature> {
    let digits = signature.strip_prefix("0x").unwrap_or(signature);
    if digits.len() != 130 || !digits.is_ascii() {
        return None;
    }
    let mut out = [0u8; 65];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&digits[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(WalletSignature(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_signatures_with_or_without_the_prefix() {
        let s = "ab".repeat(65);
        assert_eq!(parse(&s), Some(WalletSignature([0xab; 65])));
        assert_eq!(parse(&format!("0x{s}")), Some(WalletSignature([0xab; 65])));
        assert_eq!(parse(&s[2..]), None);
        assert_eq!(parse(&"zz".repeat(65)), None);
    }

    #[test]
    fn names_what_each_signature_is_for() {
        assert_eq!(
            Purpose::of_message(PersonalMessage::Text(Identity::SIGNING_MESSAGE)),
            Purpose::Identity
        );
        assert_eq!(
            Purpose::of_message(PersonalMessage::Text("Singula\n\nSign in")),
            Purpose::SignIn
        );
        assert_eq!(
            Purpose::of_message(PersonalMessage::Raw(&[0; 32])),
            Purpose::Relay
        );
        let typed = |t: &str| serde_json::json!({ "primaryType": t });
        assert_eq!(
            Purpose::of_typed_data(&typed("MintRequest")),
            Purpose::Purchase
        );
        assert_eq!(Purpose::of_typed_data(&typed("AddSlots")), Purpose::Extend);
        assert_eq!(Purpose::of_typed_data(&typed("Permit")), Purpose::Permit);
        assert_eq!(
            Purpose::of_typed_data(&typed("Activation")),
            Purpose::Activation
        );
        assert_eq!(Purpose::of_typed_data(&typed("Mail")), Purpose::Other);
    }

    #[test]
    fn tells_a_refusal_from_a_failure() {
        assert!(declined("User rejected the request."));
        assert!(!declined("network error"));
    }

    #[test]
    fn answers_reach_the_request_they_belong_to() {
        let pending = Pending::default();
        let (a, mut rx_a) = pending.open();
        let (b, mut rx_b) = pending.open();
        pending.answer(b, Ok("b".into()));
        pending.answer(a, Err("a".into()));
        assert_eq!(rx_b.try_recv().unwrap(), Ok("b".into()));
        assert_eq!(rx_a.try_recv().unwrap(), Err("a".into()));
        pending.answer(a, Ok("late".into()));
    }
}
