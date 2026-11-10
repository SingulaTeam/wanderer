//! Starting the core and driving it for the UI.
//!
//! At start-up the app resolves the RPC node and every Control Plane while
//! nothing blocks DNS, reads the chain, and loads the prover. Once a wallet
//! signs in, a supervisor runs for it; its state and traffic are forwarded to
//! the webview as `core:state` and `core:traffic`.

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use singula::Address;
use singula::chain::{Chain, GasOp, HttpRpc, Network, Payment, TokenId, front};
use singula::client::store::FileStore;
use singula::client::{Access, ControlPlaneClient, Singula};
use singula::zk::Prover;
use tauri::{AppHandle, Emitter as _, Manager as _};
use tokio::sync::{Mutex, watch};
#[cfg(desktop)]
use wanderer_core::daemon::Daemon;
use wanderer_core::engine::Engine as _;
use wanderer_core::problem::{Kind, Problem};
use wanderer_core::reach::{ControlPlaneReach, Reach, Resolved};
use wanderer_core::supervisor::{self, Handle, State};
use wanderer_core::transport::Choice;
use wanderer_tunnel::ipc;

use crate::relay::Relay;
use crate::wallet::{Pending, WebviewWallet};

/// The RPC node used unless `WANDERER_RPC_URL` names another.
const DEFAULT_RPC: &str = "https://sepolia-rollup.arbitrum.io/rpc";
const RPC_TIMEOUT: Duration = Duration::from_secs(15);
/// How often a running app reads the contract directory again: a contract
/// replaced on chain reaches an app that is never closed.
const DIRECTORY_REFRESH: Duration = Duration::from_secs(10 * 60);
/// How long the app waits, once the directory has moved, before switching:
/// every Control Plane has restarted on the new contracts by then (each
/// notices within 10 minutes and restarts within 10 more).
const DIRECTORY_SETTLE: Duration = Duration::from_secs(20 * 60);

/// The settings the person can change.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct Settings {
    pub(crate) transport: Choice,
    pub(crate) allow_lan: bool,
    /// A country for each hop, entry first, as ISO 3166-1 alpha-2 codes;
    /// none for any. Always one hop at least.
    #[serde(default = "one_hop")]
    pub(crate) route: Vec<Option<String>>,
    /// Whether the hops may share an operator.
    #[serde(default)]
    pub(crate) same_operator: bool,
    /// The single country earlier versions kept, read once into `route`.
    #[serde(default, skip_serializing)]
    country: Option<String>,
    /// The cover preset to ask for — what the connection looks like on the
    /// wire, for networks that filter by it; none for the node's choice.
    #[serde(default)]
    pub(crate) cover: Option<String>,
    /// How much of the route and transport the app shows: two plans, or
    /// every hop and setting.
    #[serde(default)]
    pub(crate) mode: Mode,
}

/// How much the app asks of the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    /// Two plans, one hop or three; only the entry and exit are chosen.
    #[default]
    Simple,
    /// Any number of hops, each chosen, and the transport's settings.
    Advanced,
}

fn one_hop() -> Vec<Option<String>> {
    vec![None]
}

impl Settings {
    /// Settings as the core can use them: a route of one to
    /// [`MAX_HOPS`](wanderer_core::route::MAX_HOPS) hops, the old single
    /// country carried into it.
    fn normalized(mut self) -> Self {
        if let Some(country) = self.country.take() {
            if self.route.iter().all(Option::is_none) {
                self.route = vec![Some(country)];
            }
        }
        self.route.truncate(wanderer_core::route::MAX_HOPS);
        if self.route.is_empty() {
            self.route = one_hop();
        }
        self
    }
}

/// Where the network has nodes, as the Control Planes see it.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Locations {
    /// Countries with available nodes, least loaded first.
    pub(crate) countries: Vec<CountryView>,
    /// Each node's country, by Star id, to name the one a session runs on.
    pub(crate) nodes: std::collections::BTreeMap<u64, String>,
    /// Available nodes, in any country or none: a chain needs two.
    pub(crate) node_count: usize,
    /// Operators running Control Planes that list nodes. With one, every
    /// hop of a chain is that operator's.
    pub(crate) control_plane_operators: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CountryView {
    pub(crate) code: String,
    pub(crate) nodes: u32,
    /// The nodes' average load there, from 0 to 1.
    ///
    /// Load, not latency: the only latency on offer is the Control Plane's
    /// round trip to a node, which says nothing about the person's own path
    /// to it — a node next to them and far from the Control Plane would look
    /// slow. Load is also what the network chooses by when no country is set.
    pub(crate) load: f32,
}

/// Whether the core can connect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "ready", rename_all = "snake_case")]
pub(crate) enum Readiness {
    Starting,
    Yes,
    No { reason: String },
}

type Client = Singula<HttpRpc, FileStore>;

/// A wallet's standing, for the UI.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Account {
    pub(crate) subscriptions: Vec<SubscriptionView>,
    /// The ways to pay for another, those the contract accepts now.
    pub(crate) offers: Vec<PriceView>,
    /// How much test USDC the faucet would add, in whole tokens; `None` off
    /// a test network, or when the wallet already holds the cap.
    pub(crate) faucet: Option<String>,
}

/// One way to pay for a subscription.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct PriceView {
    pub(crate) payment: PaymentChoice,
    /// The price of a one-slot Orbit, in whole tokens: in USDC what the
    /// contract charges now, a promotion's discount included.
    pub(crate) price: String,
    /// How many slots the wallet can afford, gas fee included, up to the
    /// most an Orbit has.
    pub(crate) affordable_slots: u8,
    /// The gas fee of an Orbit with 1, 2, … `MAX_SLOTS` slots, in whole USDC
    /// rounded up: the gas of buying, activating and joining, which a
    /// gasless purchase pays on top of its price.
    pub(crate) gas_fees: Vec<String>,
    /// What an Orbit with 1, 2, … slots costs in all, fee included, rounded
    /// up; in USDC. With ECHO the fee is still USDC, so this is the price alone.
    pub(crate) totals: Vec<String>,
    /// What the wallet holds, in whole tokens.
    pub(crate) balance: String,
    pub(crate) can_afford: bool,
}

/// What something costs, in whole USDC rounded up.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Quote {
    /// The price and the gas fee together.
    pub(crate) total: String,
    /// The part that pays for gas.
    pub(crate) gas_fee: String,
}

/// The tokens a subscription can be bought with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PaymentChoice {
    Usdc,
    Echo,
}

impl From<PaymentChoice> for Payment {
    fn from(p: PaymentChoice) -> Self {
        match p {
            PaymentChoice::Usdc => Payment::Usdc,
            PaymentChoice::Echo => Payment::Echo,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SubscriptionView {
    pub(crate) token: u64,
    pub(crate) active: bool,
    /// Unix seconds; `None` until activated. In the future while it waits
    /// behind another of the wallet's subscriptions.
    pub(crate) starts_at: Option<i64>,
    /// Unix seconds; `None` until activated.
    pub(crate) expires_at: Option<i64>,
    /// Concurrent sessions it pays for: devices, or hops of one route.
    pub(crate) slots: u8,
}

/// How many slots `balance` pays for, an Orbit of k of them costing
/// `costs[k - 1]` plus `fees[k - 1]`, up to the most an Orbit carries.
fn affordable_slots(balance: u128, costs: &[u128], fees: &[u128]) -> u8 {
    let mut best = 0;
    for k in 1..=singula::client::MAX_SLOTS {
        let i = usize::from(k - 1);
        let Some(&cost) = costs.get(i) else { break };
        if cost == 0 {
            break;
        }
        let fee = fees.get(i).copied().unwrap_or(0);
        if cost.saturating_add(fee) <= balance {
            best = k;
        }
    }
    best
}

/// The payment token's decimals, as the contract and the token agree.
const TOKEN_DECIMALS: u32 = 18;

/// Arbitrum Sepolia, where USDC is a test token with a faucet.
const TEST_CHAIN_ID: u64 = 421_614;

/// What the faucet tops a wallet up to; the Control Plane holds the same cap.
const FAUCET_CAP: u128 = 100 * 10u128.pow(TOKEN_DECIMALS);

/// An amount in the token's smallest unit, as whole tokens to two places.
/// `amount` in whole tokens, rounded up to the cent: for what is charged.
fn units_up(amount: u128) -> String {
    let cent = 10u128.pow(TOKEN_DECIMALS) / 100;
    units(amount.div_ceil(cent) * cent)
}

fn units(amount: u128) -> String {
    let one = 10u128.pow(TOKEN_DECIMALS);
    let cents = amount % one * 100 / one;
    match cents {
        0 => format!("{}", amount / one),
        c => format!("{}.{c:02}", amount / one),
    }
}

/// Everything the commands share.
pub(crate) struct Core {
    app: AppHandle,
    data: PathBuf,
    readiness: watch::Sender<Readiness>,
    ready: Mutex<Option<(Client, Reach)>>,
    /// The relay to Control Planes' fronts, kept for the app's life.
    relay: Mutex<Option<Relay>>,
    session: Mutex<Option<(Address, Handle)>>,
    settings: Mutex<Settings>,
    /// Stars' owners, read once for the locations' survey.
    owners: wanderer_core::route::Owners,
    pub(crate) signatures: Arc<Pending>,
}

impl Core {
    pub(crate) fn new(app: AppHandle) -> Arc<Self> {
        let data = app
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("wanderer"));
        let settings = read_json::<Settings>(&data.join("settings.json"))
            .unwrap_or_default()
            .normalized();
        let (readiness, _) = watch::channel(Readiness::Starting);
        Arc::new(Self {
            app,
            data,
            readiness,
            ready: Mutex::new(None),
            relay: Mutex::new(None),
            session: Mutex::new(None),
            settings: Mutex::new(settings),
            owners: wanderer_core::route::Owners::default(),
            signatures: Arc::default(),
        })
    }

    /// Prepares the core in the background; `core:readiness` says how it went.
    pub(crate) fn start(self: &Arc<Self>) {
        let core = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let readiness = match core.prepare().await {
                Ok(ready) => {
                    *core.ready.lock().await = Some(ready);
                    tauri::async_runtime::spawn(Arc::clone(&core).watch_directory());
                    Readiness::Yes
                }
                Err(reason) => {
                    tracing::error!("not ready: {reason}");
                    Readiness::No { reason }
                }
            };
            core.app.emit("core:readiness", &readiness).ok();
            core.readiness.send_replace(readiness);
        });
    }

    /// Reads the contract directory every `DIRECTORY_REFRESH` and, when a
    /// contract has been replaced, prepares again on the new ones.
    ///
    /// The switch waits until no tunnel carries traffic: one that is up stays
    /// up on the old contracts until it drops — the Control Planes' restart
    /// drops it — and the guard holds meanwhile. A session that was on is
    /// started again on the new client.
    async fn watch_directory(self: Arc<Self>) {
        loop {
            tokio::time::sleep(DIRECTORY_REFRESH).await;
            let Some((client, _)) = self.ready.lock().await.clone() else {
                continue;
            };
            match client.chain().directory_moved().await {
                Ok(false) => continue,
                Ok(true) => {}
                Err(e) => {
                    tracing::warn!("cannot read the contract directory: {e}");
                    continue;
                }
            }
            tracing::info!(
                minutes = DIRECTORY_SETTLE.as_secs() / 60,
                "the contract directory changed; switching once the Control Planes have"
            );
            tokio::time::sleep(DIRECTORY_SETTLE).await;

            // Not while a tunnel carries traffic.
            let state = self.session.lock().await.as_ref().map(|(_, h)| h.state());
            let mut was_on = false;
            if let Some(mut state) = state {
                let _ = state
                    .wait_for(|s| !matches!(s, State::Connected { .. }))
                    .await;
                was_on = !matches!(*state.borrow(), State::Disconnected | State::Failed { .. });
            }

            match self.prepare().await {
                Ok(ready) => {
                    *self.ready.lock().await = Some(ready);
                    let signed = self.session.lock().await.as_ref().map(|(a, _)| *a);
                    if let Some(address) = signed {
                        self.signed_out().await;
                        self.signed_in(address).await;
                        if was_on {
                            // The tunnel was on before the switch: bring it
                            // back, and say so if it does not come.
                            if let Err(reason) = self.connect().await {
                                tracing::warn!("could not reconnect after the switch: {reason}");
                            }
                        }
                    }
                    tracing::info!("switched to the new contracts");
                    self.app.emit("core:readiness", &Readiness::Yes).ok();
                }
                Err(reason) => {
                    tracing::warn!("cannot switch to the new contracts yet: {reason}");
                }
            }
        }
    }

    async fn prepare(&self) -> Result<(Client, Reach), String> {
        // The daemon first: without it nothing can connect, and it is the
        // one part the person must install.
        engine().status().await.map_err(|e| e.to_string())?;

        let url = std::env::var("WANDERER_RPC_URL").unwrap_or_else(|_| DEFAULT_RPC.to_owned());
        let parsed = tauri::Url::parse(&url).map_err(|e| format!("RPC URL: {e}"))?;
        let host = parsed.host_str().unwrap_or_default().to_owned();
        let port = parsed.port_or_known_default().unwrap_or(443);
        let cached: Option<Reach> = read_json(&self.data.join("reach.json"));

        // Names resolve only while no guard holds; when the app starts under
        // one — it was closed while connected — the last addresses stand in.
        let rpc_at = match Resolved::lookup(&host, port).await {
            Ok(r) => r,
            Err(e) => cached
                .as_ref()
                .map(|c| c.rpc.clone())
                .ok_or(format!("cannot resolve the RPC node: {e}"))?,
        };
        let rpc = HttpRpc::at(&url, &rpc_at.ips(), RPC_TIMEOUT).map_err(|e| e.to_string())?;
        let chain = Chain::connect(rpc, Network::ARBITRUM_SEPOLIA)
            .await
            .map_err(|e| format!("cannot read the chain: {e}"))?;
        let control_planes = chain
            .control_planes()
            .await
            .map_err(|e| format!("cannot list Control Planes: {e}"))?;
        let mut reached = Reach::control_planes(&control_planes).await;
        if reached.is_empty() {
            reached = cached.map(|c| c.control_planes).unwrap_or_default();
        }

        let prover = tokio::task::spawn_blocking(Prover::new)
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("cannot load the prover: {e}"))?;
        let store = FileStore::new(self.data.join("singula")).map_err(|e| e.to_string())?;
        let relay = self.start_relay(&reached).await;
        let mut client = Singula::new(chain, prover, store);
        // Every Control Plane at once: each choice waits on a timeout.
        let mut ways = tokio::task::JoinSet::new();
        for cp in &control_planes {
            let Some(at) = reached.iter().find(|r| r.star == cp.star) else {
                continue;
            };
            let Ok(direct) = ControlPlaneClient::at(cp, &at.at.ips()) else {
                continue;
            };
            let fronted = relay.as_ref().and_then(|r| r.entry(cp.star)).map(|port| {
                ControlPlaneClient::through(cp.star, move || {
                    tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
                })
            });
            ways.spawn(choose_way(direct, fronted));
        }
        while let Some(chosen) = ways.join_next().await {
            if let Ok(chosen) = chosen {
                client = client.with_control_plane(chosen);
            }
        }
        *self.relay.lock().await = relay;

        let reach = Reach {
            rpc: rpc_at,
            control_planes: reached,
        };
        write_json(&self.data.join("reach.json"), &reach);
        tracing::info!(control_planes = reach.control_planes.len(), "ready");
        Ok((client, reach))
    }

    /// Starts the relay to every Control Plane in `reached` that publishes
    /// a front; `None` when none does, or the relay cannot run.
    async fn start_relay(&self, reached: &[ControlPlaneReach]) -> Option<Relay> {
        let fronts: Vec<(singula::StarId, SocketAddrV4, String)> = reached
            .iter()
            .filter_map(|r| {
                let key = r.front_key.clone()?;
                let ip = *r.at.addresses.first()?;
                Some((r.star, SocketAddrV4::new(ip, front::PORT), key))
            })
            .collect();
        if fronts.is_empty() {
            return None;
        }
        match Relay::start(&fronts).await {
            Ok(relay) => Some(relay),
            Err(e) => {
                tracing::warn!("no relay to the fronts: {e}");
                None
            }
        }
    }

    /// Runs a supervisor for `address`, replacing one for another wallet.
    pub(crate) async fn signed_in(self: &Arc<Self>, address: Address) {
        if matches!(&*self.session.lock().await, Some((a, _)) if *a == address) {
            return;
        }
        self.signed_out().await;

        // Supervisors need the core; wait for it.
        let mut readiness = self.readiness.subscribe();
        let ready = match readiness.wait_for(|r| *r != Readiness::Starting).await {
            Ok(r) => *r == Readiness::Yes,
            Err(_) => false,
        };
        if !ready {
            return;
        }
        let Some((client, reach)) = self.ready.lock().await.clone() else {
            return;
        };

        let wallet = WebviewWallet::new(address, self.app.clone(), Arc::clone(&self.signatures));
        let handle = supervisor::spawn(client, Arc::new(wallet), reach, engine());
        self.forward(&handle);

        // Closed while connected, the app left the guard holding; carry on.
        let held = matches!(
            engine().status().await,
            Ok(s) if s.state != ipc::State::Off
        );
        if held {
            handle.connect(self.supervisor_settings().await).await;
        }
        *self.session.lock().await = Some((address, handle));
    }

    /// Stops the signed-in wallet's supervisor, disconnecting.
    pub(crate) async fn signed_out(&self) {
        if let Some((_, handle)) = self.session.lock().await.take() {
            disconnect_and_wait(&handle).await;
        }
    }

    pub(crate) async fn connect(&self) -> Result<(), String> {
        let settings = self.supervisor_settings().await;
        let session = self.session.lock().await;
        let (_, handle) = session.as_ref().ok_or("sign in first")?;
        handle.connect(settings).await;
        Ok(())
    }

    pub(crate) async fn disconnect(&self) {
        if let Some((_, handle)) = &*self.session.lock().await {
            handle.disconnect().await;
        }
    }

    pub(crate) async fn state(&self) -> State {
        match &*self.session.lock().await {
            Some((_, handle)) => handle.state().borrow().clone(),
            None => State::Disconnected,
        }
    }

    /// The signed-in wallet's subscriptions, the price of another and what
    /// the wallet holds to pay for it.
    pub(crate) async fn account(&self) -> Result<Account, String> {
        let (client, address) = self.signed().await?;
        let chain = client.chain();
        let subscriptions = chain
            .subscriptions(address)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|s| SubscriptionView {
                token: s.token.0,
                active: s.active,
                starts_at: s.starts_at.map(|t| t.0),
                expires_at: s.expires_at.map(|t| t.0),
                slots: s.slots,
            })
            .collect();
        let mut offers = Vec::new();
        let mut faucet = None;
        for payment in [PaymentChoice::Usdc, PaymentChoice::Echo] {
            // None: the contract does not take this token now.
            let Some(price) = chain
                .subscription_price(payment.into())
                .await
                .map_err(|e| e.to_string())?
            else {
                continue;
            };
            let balance = chain
                .balance(address, payment.into())
                .await
                .map_err(|e| e.to_string())?;
            if payment == PaymentChoice::Usdc {
                faucet = faucet_amount(chain.deployment().chain_id, balance).map(units);
            }
            let mut fees = Vec::new();
            for k in 1..=singula::client::MAX_SLOTS {
                fees.push(
                    chain
                        .gas_fee(GasOp::Purchase, k)
                        .await
                        .map_err(|e| e.to_string())?,
                );
            }
            // Only USDC pays the fee alongside the price. In USDC each size
            // is the contract's own price for it: a promotion may cover a
            // one-slot discount and not a larger one, so it is not k prices.
            let (costs, charged): (Vec<u128>, Vec<u128>) = if payment == PaymentChoice::Usdc {
                let mut costs = Vec::new();
                for k in 1..=singula::client::MAX_SLOTS {
                    costs.push(chain.mint_cost(k).await.map_err(|e| e.to_string())?);
                }
                (costs, fees.clone())
            } else {
                let costs = (1..=singula::client::MAX_SLOTS)
                    .map(|k| price * u128::from(k))
                    .collect();
                (costs, Vec::new())
            };
            let affordable = affordable_slots(balance, &costs, &charged);
            offers.push(PriceView {
                payment,
                price: units(costs.first().copied().unwrap_or(price)),
                affordable_slots: affordable,
                balance: units(balance),
                can_afford: affordable >= 1,
                totals: costs
                    .iter()
                    .zip(0..)
                    .map(|(&cost, i)| units_up(cost + charged.get(i).copied().unwrap_or(0)))
                    .collect(),
                gas_fees: fees.into_iter().map(units_up).collect(),
            });
        }
        Ok(Account {
            subscriptions,
            offers,
            faucet,
        })
    }

    /// Tops the wallet's test USDC up to the faucet's cap.
    pub(crate) async fn faucet(&self) -> Result<(), Problem> {
        let (client, address) = self.signed().await.map_err(other)?;
        let chain = client.chain();
        let held = chain
            .balance(address, Payment::Usdc)
            .await
            .map_err(|e| explain(&singula::client::ClientError::from(e), &client))?;
        let amount = faucet_amount(chain.deployment().chain_id, held)
            .ok_or_else(|| other("no faucet here, or the wallet holds enough".into()))?;
        let wallet = self.wallet(address);
        let tx = client
            .faucet(&wallet, amount)
            .await
            .map_err(|e| explain(&e, &client))?;
        tracing::info!(%tx, "faucet");
        Ok(())
    }

    /// Buys a subscription with `slots` concurrent sessions, paid with
    /// `payment`; returns the new token.
    ///
    /// Relayed by a Control Plane, so the wallet needs no ETH. Its signature
    /// may need the network — an in-app wallet signs on its provider's
    /// servers — so this is for when no guard holds or the tunnel is up.
    pub(crate) async fn buy(&self, payment: PaymentChoice, slots: u8) -> Result<u64, Problem> {
        let (client, address) = self.signed().await.map_err(other)?;
        let wallet = self.wallet(address);
        let minted = client
            .mint(&wallet, payment.into(), slots)
            .await
            .map_err(|e| explain(&e, &client))?;
        tracing::info!(token = minted.token.0, slots, tx = %minted.tx, "bought a subscription");
        Ok(minted.token.0)
    }

    /// What adding `extra` slots to `token` costs now, in whole USDC, its
    /// gas fee included and named apart.
    pub(crate) async fn add_slots_quote(&self, token: u64, extra: u8) -> Result<Quote, Problem> {
        let (client, _) = self.signed().await.map_err(other)?;
        let chain = client.chain();
        let fail = |e| explain(&singula::client::ClientError::from(e), &client);
        let cost = chain
            .add_slots_cost(TokenId(token), extra)
            .await
            .map_err(fail)?;
        let fee = chain.gas_fee(GasOp::AddSlots, extra).await.map_err(fail)?;
        Ok(Quote {
            total: units_up(cost + fee),
            gas_fee: units_up(fee),
        })
    }

    /// Adds `extra` slots to `token` for the rest of its term.
    pub(crate) async fn add_slots(&self, token: u64, extra: u8) -> Result<(), Problem> {
        let (client, address) = self.signed().await.map_err(other)?;
        let wallet = self.wallet(address);
        let tx = client
            .add_slots(&wallet, TokenId(token), extra)
            .await
            .map_err(|e| explain(&e, &client))?;
        tracing::info!(token, extra, %tx, "added slots");
        Ok(())
    }

    /// Activates `token`, joining the anonymity set in the same transaction.
    pub(crate) async fn activate(&self, token: u64) -> Result<(), Problem> {
        let (client, address) = self.signed().await.map_err(other)?;
        let wallet = self.wallet(address);
        let activated = client
            .activate(&wallet, TokenId(token))
            .await
            .map_err(|e| explain(&e, &client))?;
        tracing::info!(token, tx = %activated.tx, "activated");
        Ok(())
    }

    async fn signed(&self) -> Result<(Client, Address), String> {
        let address = self
            .session
            .lock()
            .await
            .as_ref()
            .map(|(a, _)| *a)
            .ok_or("sign in first")?;
        let client = self
            .ready
            .lock()
            .await
            .as_ref()
            .map(|(c, _)| c.clone())
            .ok_or("not ready yet")?;
        Ok((client, address))
    }

    fn wallet(&self, address: Address) -> WebviewWallet {
        WebviewWallet::new(address, self.app.clone(), Arc::clone(&self.signatures))
    }

    pub(crate) fn readiness(&self) -> Readiness {
        self.readiness.borrow().clone()
    }

    pub(crate) async fn settings(&self) -> Settings {
        self.settings.lock().await.clone()
    }

    pub(crate) async fn save_settings(&self, settings: Settings) {
        let settings = settings.normalized();
        write_json(&self.data.join("settings.json"), &settings);
        *self.settings.lock().await = settings;
    }

    /// The countries the network can serve now, from every Control Plane's
    /// list of its nodes: a route places each hop at the Control Plane that
    /// holds its node.
    pub(crate) async fn locations(&self) -> Result<Locations, String> {
        let (client, reach) = self.ready.lock().await.clone().ok_or("not ready yet")?;
        let offered = wanderer_core::route::survey(&client, &self.owners, &reach.stars()).await;
        if offered.is_empty() {
            return Err("no Control Plane listed its nodes".into());
        }

        let mut by_country: std::collections::BTreeMap<String, (u32, f32)> =
            std::collections::BTreeMap::new();
        let mut nodes = std::collections::BTreeMap::new();
        for n in &offered {
            let Some(country) = n.country else { continue };
            let code = country.as_str().to_owned();
            if nodes.insert(n.node.0, code.clone()).is_some() {
                continue;
            }
            let entry = by_country.entry(code).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += n.load.clamp(0.0, 1.0);
        }
        let mut countries: Vec<CountryView> = by_country
            .into_iter()
            .map(|(code, (count, total))| CountryView {
                code,
                nodes: count,
                #[allow(clippy::cast_precision_loss, reason = "a handful of nodes")]
                load: total / count as f32,
            })
            .collect();
        countries.sort_by(|a, b| a.load.total_cmp(&b.load).then_with(|| a.code.cmp(&b.code)));
        let node_count = offered.iter().map(|o| o.node).collect::<HashSet<_>>().len();
        let control_plane_operators = offered
            .iter()
            .map(|o| o.cp_owner)
            .collect::<HashSet<_>>()
            .len();
        Ok(Locations {
            countries,
            nodes,
            node_count,
            control_plane_operators,
        })
    }

    async fn supervisor_settings(&self) -> supervisor::Settings {
        let s = self.settings().await;
        supervisor::Settings {
            access: Access::Anonymous,
            route: s
                .route
                .iter()
                .map(|c| c.as_deref().and_then(|c| c.parse().ok()))
                .collect(),
            same_operator: s.same_operator,
            cover: s.cover.clone(),
            transport: s.transport,
            allow_lan: s.allow_lan,
        }
    }

    /// Passes a supervisor's state and traffic on to the webview.
    fn forward(&self, handle: &Handle) {
        let mut state = handle.state();
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let now = state.borrow_and_update().clone();
                app.emit("core:state", &now).ok();
                if state.changed().await.is_err() {
                    break;
                }
            }
        });
        let mut traffic = handle.traffic();
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            while traffic.changed().await.is_ok() {
                let t = *traffic.borrow_and_update();
                app.emit(
                    "core:traffic",
                    serde_json::json!({ "up": t.up, "down": t.down }),
                )
                .ok();
            }
        });
    }

    /// Disconnects before the app exits, so the guard is not left holding.
    pub(crate) async fn shut_down(&self) {
        self.signed_out().await;
    }
}

/// How long a Control Plane has to answer, each way, when choosing one.
const WAY_TIMEOUT: Duration = Duration::from_secs(12);

/// The way to reach a Control Plane: through its front if it answers there
/// — where the endpoint is blocked, it is the only way, and it hides that
/// the app talks to Singula — else directly, else the front all the same,
/// for when the network comes back.
async fn choose_way(
    direct: ControlPlaneClient,
    fronted: Option<ControlPlaneClient>,
) -> ControlPlaneClient {
    let Some(fronted) = fronted else {
        return direct;
    };
    let answers = |cp: &ControlPlaneClient| {
        let cp = cp.clone();
        async move {
            matches!(
                tokio::time::timeout(WAY_TIMEOUT, cp.health()).await,
                Ok(Ok(h)) if h.healthy
            )
        }
    };
    let (via_front, via_endpoint) = tokio::join!(answers(&fronted), answers(&direct));
    tracing::info!(star = %direct.star(), via_front, via_endpoint, "Control Plane reached");
    if via_front || !via_endpoint {
        fronted
    } else {
        direct
    }
}

/// What the faucet would add to `held`, on a test network.
fn faucet_amount(chain_id: u64, held: u128) -> Option<u128> {
    (chain_id == TEST_CHAIN_ID && held < FAUCET_CAP).then(|| FAUCET_CAP - held)
}

/// Explains a failed purchase or activation, and logs it as it came.
fn explain(e: &singula::client::ClientError, client: &Client) -> Problem {
    tracing::warn!("{e}");
    Problem::of_client(e, &client.chain().deployment().clock)
}

fn other(detail: String) -> Problem {
    Problem::new(Kind::Other, detail)
}

/// Asks to disconnect and waits until the supervisor has, for a while.
async fn disconnect_and_wait(handle: &Handle) {
    let mut state = handle.state();
    handle.disconnect().await;
    let done = state.wait_for(|s| matches!(s, State::Disconnected | State::Failed { .. }));
    let _ = tokio::time::timeout(Duration::from_secs(10), done).await;
}

/// What runs the tunnel: the daemon at its socket, which
/// `WANDERER_DAEMON_SOCKET` overrides for development.
#[cfg(desktop)]
fn engine() -> Daemon {
    let socket = std::env::var_os("WANDERER_DAEMON_SOCKET")
        .map_or_else(|| ipc::SOCKET_PATH.into(), PathBuf::from);
    Daemon::new(socket)
}

/// What runs the tunnel: the app's VPN service.
#[cfg(target_os = "android")]
fn engine() -> crate::android::VpnService {
    crate::android::VpnService
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn write_json(path: &Path, value: &impl Serialize) {
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, serde_json::to_vec_pretty(value).unwrap_or_default()));
    if let Err(e) = written {
        tracing::warn!("cannot write {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_the_gas_fee_in_what_a_balance_affords() {
        let one = 10u128.pow(TOKEN_DECIMALS);
        let costs: Vec<u128> = (1..=5).map(|k| 10 * one * k).collect();
        let fees = [one / 20; 5]; // 0.05 each
        assert_eq!(affordable_slots(30 * one, &costs, &[]), 3);
        assert_eq!(affordable_slots(30 * one, &costs, &fees), 2);
        assert_eq!(affordable_slots(30 * one + one / 20, &costs, &fees), 3);
        assert_eq!(affordable_slots(one, &costs, &fees), 0);
    }

    #[test]
    fn affords_by_each_sizes_own_price() {
        // A promotion's allowance covers a discount on one slot only.
        let one = 10u128.pow(TOKEN_DECIMALS);
        let costs = [8 * one, 20 * one, 30 * one, 40 * one, 50 * one];
        assert_eq!(affordable_slots(19 * one, &costs, &[]), 1);
        assert_eq!(affordable_slots(20 * one, &costs, &[]), 2);
    }

    #[test]
    fn rounds_charges_up_to_the_cent() {
        let one = 10u128.pow(TOKEN_DECIMALS);
        assert_eq!(units_up(one / 1000), "0.01");
        assert_eq!(units_up(10 * one + one / 100), "10.01");
        assert_eq!(units_up(10 * one), "10");
    }

    #[test]
    fn the_faucet_tops_up_to_the_cap_on_a_test_network_only() {
        let one = 10u128.pow(TOKEN_DECIMALS);
        assert_eq!(faucet_amount(TEST_CHAIN_ID, 0), Some(FAUCET_CAP));
        assert_eq!(faucet_amount(TEST_CHAIN_ID, 40 * one), Some(60 * one));
        assert_eq!(faucet_amount(TEST_CHAIN_ID, FAUCET_CAP), None);
        assert_eq!(faucet_amount(42_161, 0), None);
    }

    #[test]
    fn shows_amounts_in_whole_tokens() {
        let one = 10u128.pow(TOKEN_DECIMALS);
        assert_eq!(units(10 * one), "10");
        assert_eq!(units(one * 25 / 10), "2.50");
        assert_eq!(units(0), "0");
    }

    #[test]
    fn settings_keep_a_route_of_one_to_three_hops() {
        let read = |json: &str| serde_json::from_str::<Settings>(json).unwrap().normalized();
        // What earlier versions kept: one country, carried into the route.
        let old = read(r#"{"transport":"auto","allow_lan":false,"country":"DE","cover":null}"#);
        assert_eq!(old.route, [Some("DE".to_owned())]);
        assert!(!serde_json::to_string(&old).unwrap().contains("country"));

        let fresh = read(r#"{"transport":"auto","allow_lan":false}"#);
        assert_eq!(fresh.route, [None]);
        assert!(!fresh.same_operator);

        let long = read(r#"{"transport":"auto","allow_lan":false,"route":[null,"DE","NL","JP"]}"#);
        assert_eq!(long.route.len(), wanderer_core::route::MAX_HOPS);
        assert_eq!(
            read(r#"{"transport":"auto","allow_lan":false,"route":[]}"#).route,
            [None]
        );
    }
}
