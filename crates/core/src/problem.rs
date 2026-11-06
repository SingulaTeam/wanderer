//! What went wrong, in terms the person can act on.
//!
//! The UI words a [`Problem`] by its [`Kind`], in the person's language; the
//! error's own text rides along as `detail`, for the diagnostics log and for
//! whoever reads a problem report.

use serde::Serialize;
use singula::UnixTime;
use singula::chain::Clock;
use singula::client::ClientError;
use singula::client::wallet::WalletError;

use crate::daemon::DaemonError;
use crate::route::Gap;

/// What went wrong, and when it may stop being so.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// Which kind of trouble.
    pub kind: Kind,
    /// When trying again may work, if that is known (unix seconds).
    pub at: Option<i64>,
    /// The error as it came, in English.
    pub detail: String,
    /// For [`Kind::Route`], the hop that could not be filled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<Gap>,
}

/// The kinds of trouble the UI explains.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The person declined a signature.
    Declined,
    /// The wallet could not sign.
    WalletFailed,
    /// No Control Plane or RPC node answered.
    Offline,
    /// The wallet's identity is in no active subscription.
    NoSubscription,
    /// Every session the subscription allows is open.
    AllSessionsInUse,
    /// A session that just ended still holds its permission, until `at`.
    SessionReleasing,
    /// The wallet has used up its gasless transactions until `at`.
    GasBudget,
    /// The relaying Control Plane has used up its gas for now.
    RelayBusy,
    /// The wallet holds too little to pay.
    InsufficientFunds,
    /// Too many subscriptions bought and not activated.
    UnactivatedLimit,
    /// The price rose after it was read.
    PriceChanged,
    /// The protocol is paused.
    Paused,
    /// A node failed to carry the connection.
    Node,
    /// The connection was lost; reconnecting.
    Lost,
    /// No nodes fit the route the person asked for.
    Route,
    /// The Wanderer service is not running or failed.
    Service,
    /// The app cannot keep its data.
    Storage,
    /// Anything else.
    Other,
}

impl Problem {
    /// A problem of `kind`, with `detail`.
    pub fn new(kind: Kind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            at: None,
            detail: detail.into(),
            route: None,
        }
    }

    /// No nodes fit the route: `gap` says where.
    #[must_use]
    pub fn route(gap: Gap) -> Self {
        Self {
            detail: format!("no node fits hop {} of the route", gap.hop),
            route: Some(gap),
            ..Self::new(Kind::Route, "")
        }
    }

    /// Explains `e`; `clock` tells when a per-epoch limit resets.
    #[must_use]
    pub fn of_client(e: &ClientError, clock: &Clock) -> Self {
        let detail = e.to_string();
        let kind = match e {
            ClientError::Wallet(WalletError::Declined) => Kind::Declined,
            ClientError::Wallet(_) => Kind::WalletFailed,
            ClientError::Transport { .. }
            | ClientError::Timeout
            | ClientError::Closed
            | ClientError::NoControlPlane
            | ClientError::Chain(_) => Kind::Offline,
            ClientError::NotRegistered => Kind::NoSubscription,
            ClientError::NoPermissionFree => Kind::AllSessionsInUse,
            ClientError::PermissionFreeAt(at) => {
                return Self {
                    at: Some(at.0),
                    ..Self::new(Kind::SessionReleasing, detail)
                };
            }
            ClientError::NodeAnswer { .. } => Kind::Node,
            ClientError::Store(_) => Kind::Storage,
            ClientError::Refused { message, .. } => match reverted(message) {
                Some(Kind::GasBudget) => {
                    return Self {
                        at: next_epoch(clock).map(|t| t.0),
                        ..Self::new(Kind::GasBudget, detail)
                    };
                }
                Some(kind) => kind,
                None => Kind::Other,
            },
            _ => Kind::Other,
        };
        Self::new(kind, detail)
    }

    /// Explains a daemon error.
    #[must_use]
    pub fn of_daemon(e: &DaemonError) -> Self {
        Self::new(Kind::Service, e.to_string())
    }
}

/// Contract errors a Control Plane passes on, by selector, and what they
/// mean here.
const REVERTS: [([u8; 4], Kind); 7] = [
    // GasStation.ClientGasBudgetExceeded(address,uint256,uint256)
    ([0x4b, 0x60, 0x6c, 0x87], Kind::GasBudget),
    // GasStation.GasBudgetExceeded(uint256,uint256,uint256)
    ([0x39, 0x48, 0xff, 0xb4], Kind::RelayBusy),
    // ERC20InsufficientBalance(address,uint256,uint256)
    ([0xe4, 0x50, 0xd3, 0x8c], Kind::InsufficientFunds),
    // ERC20InsufficientAllowance(address,uint256,uint256)
    ([0xfb, 0x8f, 0x41, 0xb2], Kind::InsufficientFunds),
    // SubscriptionNFT.UnactivatedLimitExceeded()
    ([0x54, 0x02, 0x17, 0x41], Kind::UnactivatedLimit),
    // SubscriptionNFT.CostExceeded(uint256,uint256)
    ([0xae, 0x2f, 0x2f, 0x6a], Kind::PriceChanged),
    // Pausable.EnforcedPause()
    ([0xd9, 0x3c, 0x06, 0x65], Kind::Paused),
];

/// The known contract error in a Control Plane's refusal, if any.
///
/// The Control Plane quotes the revert data as `revert data: 0x…`. A relayed
/// call that failed is wrapped in `GasStation.RelayFailed(bytes)`, so the
/// inner error sits further in; every 4-byte-aligned word is looked at.
fn reverted(message: &str) -> Option<Kind> {
    let (_, rest) = message.split_once("revert data: 0x")?;
    let digits: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
    let bytes: Vec<u8> = digits
        .as_bytes()
        .chunks_exact(2)
        .filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect();
    bytes.chunks_exact(4).find_map(|word| {
        REVERTS
            .iter()
            .find(|(selector, _)| selector == word)
            .map(|(_, kind)| *kind)
    })
}

fn next_epoch(clock: &Clock) -> Option<UnixTime> {
    let now = clock.current_epoch()?;
    clock.epoch_start(singula::Epoch(now.0 + 1))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const CLOCK: Clock = Clock {
        epoch_zero: UnixTime(0),
        epoch_length: Duration::from_secs(86_400),
    };

    fn refused(message: &str) -> ClientError {
        ClientError::Refused {
            code: 0,
            message: message.into(),
        }
    }

    #[test]
    fn reads_the_gas_budget_and_when_it_resets() {
        let e = refused(
            "failed to submit activation transaction: activateWithSignature relayClient: \
             GasStation.relayClient: gas estimate failed, revert data: 0x4b606c87\
             000000000000000000000000804ae4c760b39a084b2e9840c598697afc3cad15\
             00000000000000000000000000000000000000000000000000000000001074bd\
             00000000000000000000000000000000000000000000000000000000000f4240 \
             (err: execution reverted)",
        );
        let p = Problem::of_client(&e, &CLOCK);
        assert_eq!(p.kind, Kind::GasBudget);
        let at = p.at.unwrap();
        assert!(at > UnixTime::now().0 && at % 86_400 == 0);
    }

    #[test]
    fn finds_an_error_wrapped_in_relay_failed() {
        // RelayFailed(bytes) carrying ERC20InsufficientBalance.
        let inner = "e450d38c".to_owned() + &"00".repeat(96);
        let data = format!("74c412f5{:064x}{:064x}{inner}", 32, inner.len() / 2);
        let e = refused(&format!("mint failed, revert data: 0x{data}"));
        assert_eq!(Problem::of_client(&e, &CLOCK).kind, Kind::InsufficientFunds);
    }

    #[test]
    fn leaves_the_rest_as_it_is() {
        let p = Problem::of_client(&refused("something new"), &CLOCK);
        assert_eq!((p.kind, p.at), (Kind::Other, None));
        assert_eq!(p.detail, "the Control Plane refused: something new");
        assert_eq!(
            Problem::of_client(&ClientError::Wallet(WalletError::Declined), &CLOCK).kind,
            Kind::Declined
        );
    }
}
