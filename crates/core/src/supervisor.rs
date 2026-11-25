//! Keeping the tunnel up.
//!
//! The supervisor is one task that owns the [`Engine`] and the session. From
//! "connect" until "disconnect" the guard holds: nothing leaves outside the
//! tunnel, through every failure and reconnect in between.
//!
//! ```text
//! Disconnected ──connect──▶ Connecting ──▶ Connected
//!                               │  ▲            │ tunnel lost
//!                               ▼  │            ▼
//!                             Holding ◀─────────┘   (the guard blocks; retrying)
//!                               │
//!          a lasting refusal ───┴──▶ Failed       (the guard is removed)
//! ```
//!
//! The wallet signs before the guard goes up, never under it: a wallet may
//! need the network to sign.
//!
//! A lost tunnel is first retried on the same session — another transport,
//! another cover — since that needs nothing from the Control Plane. Only
//! when none carries traffic is the session given up and a new one opened,
//! on another node.
//!
//! Every hop's node is chosen with its Control Plane, from the lists of all
//! of them: a session on a right opens only at the Control Plane its node is
//! connected to.
//!
//! A route of several hops is opened one hop at a time, each session asked
//! for through the hops already up: the Control Plane of a hop sees the hop
//! before it, never the person. Its pings keep to that path — traffic to a
//! hop's Control Plane leaves the chain at that hop. If any hop is lost, the
//! whole chain is rebuilt. Each hop holds one of the subscription's slots,
//! so a route has at most as many hops as the subscription has slots.
//!
//! The network may close a session itself: idle too long, its pings lost,
//! its node no longer counting time, another device on its slot. Wanderer
//! reconnects as for a lost tunnel, avoiding the node when the node is to
//! blame, and says what happened once connected again — except when every
//! slot has been closed in turn for the person's other devices, when it
//! stops rather than take a slot from them.
//!
//! "Carries traffic" is checked, not assumed: once Xray is up, the Control
//! Plane is asked for its health through the tunnel. That answer has crossed
//! the node, so the tunnel works end to end; and while connected, it is asked
//! again every so often, since a node can stop forwarding without closing
//! anything.

use std::collections::HashSet;
use std::future::Future as _;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::Poll;
use std::time::Duration;

use serde::Serialize;
use singula::StarId;
use singula::chain::{Clock, HttpRpc};
use singula::client::store::Store;
use singula::client::wallet::Wallet;
use singula::client::{
    Access, ClientError, ConnectOptions, Connection, Country, EndReason, Event, Singula, Traffic,
};
use singula::journal::CloseReason;
use tokio::sync::{mpsc, watch};
use tokio::time::MissedTickBehavior;
use wanderer_tunnel::ipc;
use wanderer_tunnel::{EarlyExit, GuardPolicy, Hop, Resolver, TunnelPlan};

use crate::daemon::DaemonError;
use crate::engine::Engine;
use crate::problem::{self, Notice, Problem};
use crate::reach::Reach;
use crate::route::{self, Owners, Pick, Wanted};
use crate::transport::{Candidate, Choice, Kind, Offer};

/// How long the Control Plane has to answer through a new tunnel.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(8);
/// How often the daemon is asked whether Xray still runs, and for traffic.
const POLL_EVERY: Duration = Duration::from_secs(2);
/// How often a running tunnel is checked end to end.
const CHECK_EVERY: Duration = Duration::from_secs(20);
/// Failed end-to-end checks in a row that count as a lost tunnel.
const CHECKS_TO_LOSE: u32 = 2;
/// Failed pings in a row that call for an end-to-end check now.
const PINGS_TO_CHECK: u32 = 2;
/// How long a session must have run for another device's hold on its slot
/// to count as settled: overlaps after that start a fresh count.
const OVERLAP_SETTLED: Duration = Duration::from_secs(10 * 60);
/// Waits between attempts to open a session, growing to the last.
const BACKOFF: [Duration; 6] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Anonymous, or by wallet.
    pub access: Access,
    /// A country for each hop, entry first; `None` for any. Empty is one hop
    /// anywhere.
    pub route: Vec<Option<Country>>,
    /// Whether the hops of a route may share an operator.
    pub same_operator: bool,
    /// A cover preset to ask the node for, such as `RU`; none for its own
    /// choice.
    pub cover: Option<String>,
    /// Which transport.
    pub transport: Choice,
    /// Whether the local network stays reachable.
    pub allow_lan: bool,
    /// Where DNS is asked, through the tunnel.
    pub resolver: Resolver,
}

/// Where the connection is, for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum State {
    /// No tunnel and no guard: traffic flows as without Wanderer.
    Disconnected,
    /// Opening the first session; the guard already holds.
    Connecting,
    /// The tunnel carries traffic, checked end to end.
    Connected {
        /// The nodes' Star ids, entry first.
        nodes: Vec<u64>,
        /// How the entry is reached.
        transport: Kind,
        /// Under which cover.
        cover: String,
        /// What the network did that the app dealt with on the way here,
        /// for the person to see once.
        #[serde(skip_serializing_if = "Option::is_none")]
        notice: Option<Noticed>,
    },
    /// The tunnel is down and the guard holds: nothing leaves while the
    /// supervisor reconnects.
    Holding {
        /// What went wrong.
        problem: Problem,
    },
    /// Stopped trying for a reason retrying will not fix; the guard is
    /// removed, so the machine is not left offline.
    Failed {
        /// What went wrong.
        problem: Problem,
    },
}

/// A [`Notice`], and when it happened: the UI shows each one once, until
/// dismissed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Noticed {
    /// What happened.
    pub kind: Notice,
    /// When, in unix seconds.
    pub at: i64,
}

enum Command {
    Connect(Settings),
    Disconnect,
}

/// The app's handle on the supervisor.
#[derive(Clone)]
pub struct Handle {
    commands: mpsc::Sender<Command>,
    state: watch::Receiver<State>,
    traffic: watch::Receiver<Traffic>,
}

impl Handle {
    /// Connects with `settings`, or reconnects with them if connected.
    pub async fn connect(&self, settings: Settings) {
        let _ = self.commands.send(Command::Connect(settings)).await;
    }

    /// Disconnects, removing the guard.
    pub async fn disconnect(&self) {
        let _ = self.commands.send(Command::Disconnect).await;
    }

    /// The state, as it changes.
    #[must_use]
    pub fn state(&self) -> watch::Receiver<State> {
        self.state.clone()
    }

    /// Bytes through the tunnel since connecting, as they change.
    #[must_use]
    pub fn traffic(&self) -> watch::Receiver<Traffic> {
        self.traffic.clone()
    }
}

/// Starts the supervisor; it runs until every [`Handle`] is dropped, and
/// disconnects then.
pub fn spawn<S, W, E>(
    singula: Singula<HttpRpc, S>,
    wallet: Arc<W>,
    reach: Reach,
    daemon: E,
) -> Handle
where
    S: Store + 'static,
    W: Wallet + 'static,
    E: Engine,
{
    let (commands, rx) = mpsc::channel(8);
    let (state_tx, state) = watch::channel(State::Disconnected);
    let (traffic_tx, traffic) = watch::channel(Traffic { up: 0, down: 0 });
    let supervisor = Supervisor {
        singula,
        wallet,
        reach,
        daemon,
        state: state_tx,
        traffic: traffic_tx,
        total: Arc::new(Mutex::new(Traffic { up: 0, down: 0 })),
        worked: None,
        owners: Owners::default(),
        notice: None,
    };
    tokio::spawn(supervisor.run(rx));
    Handle {
        commands,
        state,
        traffic,
    }
}

struct Supervisor<S, W, E> {
    singula: Singula<HttpRpc, S>,
    wallet: Arc<W>,
    reach: Reach,
    daemon: E,
    state: watch::Sender<State>,
    traffic: watch::Sender<Traffic>,
    /// The daemon's count, shared with the sessions' meters.
    total: Arc<Mutex<Traffic>>,
    /// The transport that carried traffic last.
    worked: Option<Kind>,
    /// Stars' owners, for routes.
    owners: Owners,
    /// What to show once connected again.
    notice: Option<Noticed>,
}

/// Sessions with a tunnel that carries their traffic.
struct Live {
    /// One per hop, entry first.
    connections: Vec<Connection>,
    /// How the entry can be reached.
    offer: Offer,
    /// The entry's address.
    node: Ipv4Addr,
    /// The hops after the entry.
    onward: Vec<Hop>,
    /// The way to the entry that carried traffic last, without the hops
    /// after it, and its cover.
    entry: Option<(TunnelPlan, String)>,
}

impl Live {
    /// The Control Plane of the last hop: asked through the whole chain, it
    /// answers only if every hop carries traffic.
    fn exit_control_plane(&self) -> StarId {
        self.connections
            .last()
            .map_or(StarId(0), Connection::control_plane)
    }

    fn nodes(&self) -> Vec<StarId> {
        self.connections.iter().map(Connection::node).collect()
    }

    /// `entry` with the hops after it.
    fn plan(&self, mut entry: TunnelPlan, reach: &Reach) -> TunnelPlan {
        let cps: Vec<StarId> = self
            .connections
            .iter()
            .map(Connection::control_plane)
            .collect();
        entry.onward.clone_from(&self.onward);
        entry.early_exits = early_exits(&cps, reach);
        entry
    }

    fn end(self) {
        for connection in self.connections {
            end(connection);
        }
    }
}

/// Traffic to each hop's Control Plane leaves the chain at that hop. An
/// address that a later hop's Control Plane shares — one operator running
/// both, which only a route allowed to share operators has — leaves at the
/// later one.
fn early_exits(cps: &[StarId], reach: &Reach) -> Vec<EarlyExit> {
    let Some((_, before_last)) = cps.split_last() else {
        return Vec::new();
    };
    let mut exits = Vec::new();
    for (hop, &cp) in before_last.iter().enumerate() {
        for &address in reach.addresses(cp) {
            let later = cps[hop + 1..]
                .iter()
                .any(|&l| reach.addresses(l).contains(&address));
            if !later {
                exits.push(EarlyExit {
                    hop: u8::try_from(hop).unwrap_or(u8::MAX),
                    address,
                });
            }
        }
    }
    exits.truncate(wanderer_tunnel::plan::MAX_EARLY_EXITS);
    exits
}

/// Why keeping a session stopped.
enum Exit {
    Command(Option<Command>),
    Lost(String),
    /// The network closed a session, for this reason.
    Closed(CloseReason),
    Daemon(DaemonError),
}

/// Why opening a session did not work.
enum Refusal {
    /// Worth retrying, avoiding `node` if one is named.
    Transient {
        problem: Problem,
        node: Option<StarId>,
    },
    /// Not worth retrying.
    Lasting(Problem),
}

impl<S: Store + 'static, W: Wallet + 'static, E: Engine> Supervisor<S, W, E> {
    async fn run(mut self, mut commands: mpsc::Receiver<Command>) {
        let mut next = commands.recv().await;
        while let Some(command) = next.take() {
            next = match command {
                Command::Connect(settings) => self.session(settings, &mut commands).await,
                Command::Disconnect => {
                    self.off(State::Disconnected).await;
                    commands.recv().await
                }
            };
        }
        // Every handle is gone: the app is closing.
        self.off(State::Disconnected).await;
    }

    /// Connects and keeps the connection until a command ends it; returns
    /// that command.
    async fn session(
        &mut self,
        settings: Settings,
        commands: &mut mpsc::Receiver<Command>,
    ) -> Option<Command> {
        let guard = self.reach.guard(settings.allow_lan);
        self.notice = None;
        self.set(State::Connecting);

        if let Err(problem) = self.prepare(&settings, &guard).await {
            self.off(State::Failed { problem }).await;
            return commands.recv().await;
        }

        let mut avoid = HashSet::new();
        let mut attempt = 0;
        let mut connected_before = false;
        // Sessions in a row another device's hold on their slot closed.
        let mut overlaps = 0;
        loop {
            let opened = self.open(&settings, &guard, &avoid).await;
            // Opening is not cancelled midway — a session half opened would
            // be left for its Control Plane to sweep — so a command waits.
            if let Ok(command) = commands.try_recv() {
                if let Ok(live) = opened {
                    self.leave(live, &command).await;
                }
                return Some(command);
            }
            match opened {
                Ok(mut live) => {
                    attempt = 0;
                    connected_before = true;
                    let started = tokio::time::Instant::now();
                    match self.keep(&mut live, &settings, &guard, commands).await {
                        Exit::Command(command) => {
                            if let Some(command) = &command {
                                self.leave(live, command).await;
                            } else {
                                live.end();
                            }
                            return command;
                        }
                        Exit::Lost(reason) => {
                            tracing::warn!("tunnel lost: {reason}");
                            avoid.extend(live.nodes());
                            live.end();
                            self.set(State::Holding {
                                problem: Problem::new(problem::Kind::Lost, reason),
                            });
                        }
                        Exit::Closed(reason) => {
                            let lived = started.elapsed();
                            let stop = self
                                .closed(reason, live, lived, &mut overlaps, &mut avoid, &settings)
                                .await;
                            if let Err(problem) = stop {
                                self.off(State::Failed { problem }).await;
                                return commands.recv().await;
                            }
                        }
                        Exit::Daemon(e) => {
                            live.end();
                            self.off(State::Failed {
                                problem: Problem::of_daemon(&e),
                            })
                            .await;
                            return commands.recv().await;
                        }
                    }
                }
                // Nodes come and go: a route that fitted a moment ago may
                // fit again, and the guard keeps holding meanwhile.
                Err(Refusal::Lasting(problem))
                    if problem.kind == problem::Kind::Route && connected_before =>
                {
                    tracing::warn!("no route now: {}", problem.detail);
                    self.set(State::Holding { problem });
                    let wait = BACKOFF[attempt.min(BACKOFF.len() - 1)];
                    attempt += 1;
                    tokio::select! {
                        command = commands.recv() => return command,
                        () = tokio::time::sleep(wait) => {}
                    }
                }
                Err(Refusal::Lasting(problem)) => {
                    self.off(State::Failed { problem }).await;
                    return commands.recv().await;
                }
                Err(Refusal::Transient { problem, node }) => {
                    tracing::warn!("no session: {}", problem.detail);
                    avoid.extend(node);
                    self.set(State::Holding { problem });
                    let wait = BACKOFF[attempt.min(BACKOFF.len() - 1)];
                    attempt += 1;
                    tokio::select! {
                        command = commands.recv() => return command,
                        () = tokio::time::sleep(wait) => {}
                    }
                }
            }
        }
    }

    /// Signs what the sessions need signed, then puts the guard up.
    ///
    /// Signing may need the network — an in-app wallet signs on its
    /// provider's servers — and the guard would block that, so it comes
    /// first. The anonymous identity is kept once derived, so this asks the
    /// wallet only the first time.
    async fn prepare(&mut self, settings: &Settings, guard: &GuardPolicy) -> Result<(), Problem> {
        if settings.access == Access::Anonymous {
            if let Err(e) = self.singula.identity(&*self.wallet).await {
                let (Refusal::Lasting(problem) | Refusal::Transient { problem, .. }) =
                    refusal(&e, &self.clock());
                return Err(problem);
            }
        }
        self.daemon
            .hold(guard)
            .await
            .map(drop)
            .map_err(|e| Problem::of_daemon(&e))
    }

    /// Deals with a session the network closed for `reason` after `lived`:
    /// avoids its node when the node is to blame, keeps what to tell the
    /// person once connected again, and stops — `Err` — when every slot has
    /// been closed in turn for another device, since trying on would only
    /// take a slot from it.
    async fn closed(
        &mut self,
        reason: CloseReason,
        live: Live,
        lived: Duration,
        overlaps: &mut usize,
        avoid: &mut HashSet<StarId>,
        settings: &Settings,
    ) -> Result<(), Problem> {
        tracing::warn!("the network closed a session: {reason}");
        if lived >= OVERLAP_SETTLED {
            *overlaps = 0;
        }
        if reason == CloseReason::Overlap {
            *overlaps += 1;
        }
        let notice = Notice::of_close(&reason);
        if notice.is_some_and(Notice::blames_the_node) {
            avoid.extend(live.nodes());
        }
        live.end();
        if *overlaps > self.slots(settings).await {
            return Err(Problem::new(
                problem::Kind::AllSessionsInUse,
                "every slot is held by another device",
            ));
        }
        self.notice = notice.map(|kind| Noticed {
            kind,
            at: singula::UnixTime::now().0,
        });
        self.set(State::Holding {
            problem: Problem::new(problem::Kind::Lost, reason.to_string()),
        });
        Ok(())
    }

    /// Leaves a live session because `command` arrived. A disconnect takes
    /// the tunnel and the guard down first — the person gets their network
    /// back at once — and the session is ended in the background, which may
    /// take a Control Plane a while to answer.
    async fn leave(&mut self, live: Live, command: &Command) {
        if matches!(command, Command::Disconnect) {
            self.off(State::Disconnected).await;
        }
        live.end();
    }

    /// Opens the route's sessions and brings up a tunnel that carries
    /// their traffic.
    async fn open(
        &mut self,
        settings: &Settings,
        guard: &GuardPolicy,
        avoid: &HashSet<StarId>,
    ) -> Result<Live, Refusal> {
        // Each hop holds one of the subscription's slots.
        if settings.access == Access::Anonymous && settings.route.len() > self.slots(settings).await
        {
            return Err(Refusal::Lasting(Problem::new(
                problem::Kind::NoFreeSlot,
                "the route has more hops than the subscription has slots",
            )));
        }
        // Every hop is placed here, a single one too: a session on a right
        // opens only at the Control Plane its node is connected to, so the
        // node and its Control Plane are chosen together, from every Control
        // Plane's list. Asking one Control Plane for a country fails whenever
        // the country's nodes are connected to another.
        let route = self.route(settings, avoid).await?;
        let mut options = ConnectOptions::new().access(settings.access);
        if let Some(cover) = &settings.cover {
            options = options.cover(cover.clone());
        }
        if let Some(entry) = route.first() {
            options = options.control_plane(entry.cp).node(entry.node);
        }

        let meter = self.meter();
        let connection = self
            .singula
            .connect(&*self.wallet, options, meter)
            .await
            .map_err(|e| refusal(&e, &self.clock()))?;
        let node = connection.node();
        let tunnel = connection.tunnel();
        let address = match tunnel.address.parse::<Ipv4Addr>() {
            Ok(ip) => Some(ip),
            // The guard blocks DNS; the daemon's own queries pass it.
            Err(_) => match self.daemon.resolve(&tunnel.address).await {
                Ok(addresses) => addresses.first().copied(),
                Err(e) => {
                    tracing::warn!(host = tunnel.address, "{e}");
                    None
                }
            },
        };
        let Some(address) = address else {
            end(connection);
            return Err(unresolved(node));
        };
        let mut live = Live {
            offer: Offer::from(tunnel),
            connections: vec![connection],
            node: address,
            onward: Vec::new(),
            entry: None,
        };
        match self.bring_up(&mut live, settings, guard).await {
            Ok(true) => {}
            Ok(false) => {
                live.end();
                return Err(silent(node));
            }
            Err(e) => {
                live.end();
                return Err(Refusal::Lasting(Problem::of_daemon(&e)));
            }
        }
        for &pick in route.iter().skip(1) {
            if let Err(refused) = self.extend(&mut live, settings, guard, pick).await {
                live.end();
                return Err(refused);
            }
        }
        self.announce(&live);
        Ok(live)
    }

    /// The nodes for the route the person asked for.
    async fn route(
        &self,
        settings: &Settings,
        avoid: &HashSet<StarId>,
    ) -> Result<Vec<Pick>, Refusal> {
        let offered = route::survey(&self.singula, &self.owners, &self.reach.stars()).await;
        if offered.is_empty() {
            return Err(Refusal::Transient {
                problem: Problem::new(problem::Kind::Offline, "no Control Plane listed its nodes"),
                node: None,
            });
        }
        let wanted = Wanted {
            hops: settings.route.clone(),
            same_operator: settings.same_operator,
        };
        route::choose(&wanted, &offered, avoid).map_err(|gap| Refusal::Lasting(Problem::route(gap)))
    }

    /// Adds the hop `pick` to the end of the chain, asking for its session
    /// through the hops already up.
    async fn extend(
        &mut self,
        live: &mut Live,
        settings: &Settings,
        guard: &GuardPolicy,
        pick: Pick,
    ) -> Result<(), Refusal> {
        let options = ConnectOptions::new()
            .access(settings.access)
            .control_plane(pick.cp)
            .node(pick.node)
            .exit_leg();
        let meter = self.meter();
        let connection = self
            .singula
            .connect(&*self.wallet, options, meter)
            .await
            .map_err(|e| match refusal(&e, &self.clock()) {
                Refusal::Transient {
                    problem,
                    node: None,
                } => Refusal::Transient {
                    problem,
                    node: Some(pick.node),
                },
                other => other,
            })?;
        let tunnel = connection.tunnel();
        let address = match tunnel.address.parse::<Ipv4Addr>() {
            Ok(ip) => Some(ip),
            // The tunnel is up, so the system resolver asks through it.
            // The daemon's resolver would ask beside the tunnel, in the
            // clear, and name the hop to the local network.
            Err(_) => resolve_through_tunnel(&tunnel.address).await,
        };
        let Some(hop) = address.and_then(|ip| Offer::from(tunnel).hop(ip)) else {
            end(connection);
            return Err(unresolved(pick.node));
        };
        live.onward.push(hop);
        live.connections.push(connection);

        let Some((entry, _)) = live.entry.clone() else {
            return Err(silent(pick.node));
        };
        let plan = live.plan(entry, &self.reach);
        if let Err(e) = self.daemon.connect(&plan, guard).await {
            return Err(Refusal::Lasting(Problem::of_daemon(&e)));
        }
        if self.verify(live.exit_control_plane()).await {
            tracing::info!(node = %pick.node, hops = live.connections.len(), "hop added");
            Ok(())
        } else {
            Err(silent(pick.node))
        }
    }

    /// Tries every way to reach the entry, with the hops after it, until one
    /// carries traffic; `false` if none does, with the guard holding.
    async fn bring_up(
        &mut self,
        live: &mut Live,
        settings: &Settings,
        guard: &GuardPolicy,
    ) -> Result<bool, DaemonError> {
        let candidates = live.offer.plans(live.node, settings.transport, self.worked);
        for Candidate { cover, plan } in candidates {
            let mut whole = live.plan(plan.clone(), &self.reach);
            whole.resolver.clone_from(&settings.resolver);
            match self.daemon.connect(&whole, guard).await {
                Ok(_) => {}
                Err(DaemonError::Refused { message, .. }) => {
                    tracing::warn!("the daemon refused a plan: {message}");
                    continue;
                }
                Err(e) => return Err(e),
            }
            let transport = Kind::from(&plan.transport);
            if self.verify(live.exit_control_plane()).await {
                tracing::info!(nodes = ?live.nodes(), ?transport, cover, "carries traffic");
                self.worked = Some(transport);
                live.entry = Some((plan, cover));
                return Ok(true);
            }
            tracing::info!(?transport, cover, "no traffic through this way; next");
        }
        self.daemon.hold(guard).await?;
        Ok(false)
    }

    /// Tells the UI the chain is up.
    fn announce(&self, live: &Live) {
        let Some((plan, cover)) = &live.entry else {
            return;
        };
        self.set(State::Connected {
            nodes: live.nodes().iter().map(|n| n.0).collect(),
            transport: Kind::from(&plan.transport),
            cover: cover.clone(),
            notice: self.notice,
        });
    }

    /// Watches a live session until it ends, the tunnel is lost for good,
    /// or a command arrives.
    async fn keep(
        &mut self,
        live: &mut Live,
        settings: &Settings,
        guard: &GuardPolicy,
        commands: &mut mpsc::Receiver<Command>,
    ) -> Exit {
        let mut poll = tokio::time::interval(POLL_EVERY);
        let mut check = tokio::time::interval(CHECK_EVERY);
        poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
        check.set_missed_tick_behavior(MissedTickBehavior::Delay);
        check.reset(); // just verified
        let mut failed_checks = 0;

        loop {
            let lost = tokio::select! {
                command = commands.recv() => return Exit::Command(command),
                event = next_event(&mut live.connections) => match event {
                    Some(Event::Ended(EndReason::ClosedByNetwork(reason))) => return Exit::Closed(reason),
                    None | Some(Event::Ended(_)) => return Exit::Lost("a session ended".into()),
                    Some(Event::PingFailed { in_a_row, .. }) if in_a_row >= PINGS_TO_CHECK => {
                        (!self.verify(live.exit_control_plane()).await)
                            .then(|| "pings stopped getting through".to_owned())
                    }
                    Some(event) => {
                        tracing::debug!(?event);
                        None
                    }
                },
                _ = poll.tick() => match self.daemon.status().await {
                    Ok(status) if status.state == ipc::State::Up => {
                        self.count(status.traffic);
                        None
                    }
                    Ok(status) => Some(status.xray_exit.unwrap_or_else(|| "the tunnel stopped".into())),
                    Err(e) => return Exit::Daemon(e),
                },
                _ = check.tick() => {
                    if self.verify(live.exit_control_plane()).await {
                        failed_checks = 0;
                        None
                    } else {
                        failed_checks += 1;
                        (failed_checks >= CHECKS_TO_LOSE)
                            .then(|| "no traffic gets through the tunnel".to_owned())
                    }
                }
            };
            let Some(reason) = lost else { continue };

            // The session may be fine and only this way blocked: try the
            // others before giving the session up.
            tracing::warn!("{reason}; trying the session's other ways");
            self.set(State::Holding {
                problem: Problem::new(problem::Kind::Lost, reason.clone()),
            });
            match self.bring_up(live, settings, guard).await {
                Ok(true) => {
                    self.announce(live);
                    failed_checks = 0;
                    check.reset();
                }
                Ok(false) => return Exit::Lost(reason),
                Err(e) => return Exit::Daemon(e),
            }
        }
    }

    /// How many slots the subscription has: how many sessions it can hold at
    /// once. Without an answer, as many as a route can have, and the Control
    /// Planes decide.
    async fn slots(&self, settings: &Settings) -> usize {
        if settings.access != Access::Anonymous {
            return usize::MAX;
        }
        self.singula
            .identities(&*self.wallet)
            .await
            .map_or(usize::MAX, |slots| slots.len())
    }

    /// Whether the Control Plane answers through the tunnel.
    fn clock(&self) -> Clock {
        self.singula.chain().deployment().clock
    }

    async fn verify(&self, cp: StarId) -> bool {
        let Ok(client) = self.singula.control_plane(cp).await else {
            return false;
        };
        // A connection to it opened before the tunnel came up dies when the
        // routes move under it; the second attempt opens a fresh one.
        for _ in 0..2 {
            let answer = tokio::time::timeout(VERIFY_TIMEOUT, client.health()).await;
            if matches!(answer, Ok(Ok(h)) if h.healthy) {
                return true;
            }
        }
        false
    }

    /// The meter a new session counts from: the daemon's count, from now.
    fn meter(&self) -> impl Fn() -> Traffic + Send + Sync + 'static {
        let total = Arc::clone(&self.total);
        let start = *total.lock().unwrap_or_else(PoisonError::into_inner);
        move || {
            let now = *total.lock().unwrap_or_else(PoisonError::into_inner);
            Traffic {
                up: now.up.saturating_sub(start.up),
                down: now.down.saturating_sub(start.down),
            }
        }
    }

    fn count(&self, traffic: ipc::Traffic) {
        let traffic = Traffic {
            up: traffic.up,
            down: traffic.down,
        };
        *self.total.lock().unwrap_or_else(PoisonError::into_inner) = traffic;
        self.traffic.send_replace(traffic);
    }

    /// Removes the guard and the tunnel, and says why.
    async fn off(&mut self, state: State) {
        if let Err(e) = self.daemon.disconnect().await {
            tracing::warn!("disconnecting: {e}");
        }
        self.set(state);
    }

    fn set(&self, state: State) {
        self.state.send_if_modified(|current| {
            let changed = *current != state;
            *current = state;
            changed
        });
    }
}

/// Whether an SDK error is worth retrying.
fn refusal(e: &ClientError, clock: &Clock) -> Refusal {
    let problem = Problem::of_client(e, clock);
    match e {
        ClientError::NotRegistered
        | ClientError::NoFreeSlot
        | ClientError::Wallet(_)
        | ClientError::Store(_) => Refusal::Lasting(problem),
        // The node, or a Control Plane relaying for it, does not answer as
        // the protocol requires; another node may.
        ClientError::NodeAnswer { node, .. } => Refusal::Transient {
            problem,
            node: Some(*node),
        },
        _ => Refusal::Transient {
            problem,
            node: None,
        },
    }
}

/// The node's address did not resolve.
fn unresolved(node: StarId) -> Refusal {
    Refusal::Transient {
        problem: Problem::new(problem::Kind::Node, "the node's address does not resolve"),
        node: Some(node),
    }
}

/// Nothing got through the node.
fn silent(node: StarId) -> Refusal {
    Refusal::Transient {
        problem: Problem::new(problem::Kind::Node, "the node does not carry traffic"),
        node: Some(node),
    }
}

/// `host`'s first IPv4 address, by the system's resolver.
async fn resolve_through_tunnel(host: &str) -> Option<Ipv4Addr> {
    let addresses = tokio::time::timeout(VERIFY_TIMEOUT, tokio::net::lookup_host((host, 443)))
        .await
        .ok()?
        .ok()?;
    addresses.into_iter().find_map(|a| match a.ip() {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    })
}

/// The next event of any of `connections`.
async fn next_event(connections: &mut [Connection]) -> Option<Event> {
    std::future::poll_fn(|cx| {
        for connection in connections.iter_mut() {
            // Receiving is cancel-safe: an event not yet taken stays queued.
            let next = std::pin::pin!(connection.next_event());
            if let Poll::Ready(event) = next.poll(cx) {
                return Poll::Ready(event);
            }
        }
        Poll::Pending
    })
    .await
}

/// Ends a session in the background: its end is a courtesy to the Control
/// Plane, and nothing here waits on it.
fn end(connection: Connection) {
    tokio::spawn(connection.close());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reach::{ControlPlaneReach, Resolved};

    fn reach(cps: &[(u64, &[u8])]) -> Reach {
        Reach {
            rpc: Resolved::default(),
            control_planes: cps
                .iter()
                .map(|(star, last)| ControlPlaneReach {
                    star: StarId(*star),
                    at: Resolved {
                        addresses: last
                            .iter()
                            .map(|&l| Ipv4Addr::new(203, 0, 113, l))
                            .collect(),
                        port: 443,
                    },
                    front_key: None,
                })
                .collect(),
        }
    }

    fn exits(cps: &[u64], reach: &Reach) -> Vec<(u8, u8)> {
        let cps: Vec<StarId> = cps.iter().copied().map(StarId).collect();
        early_exits(&cps, reach)
            .iter()
            .map(|e| (e.hop, e.address.octets()[3]))
            .collect()
    }

    #[test]
    fn each_control_plane_is_reached_from_its_own_hop() {
        let reach = reach(&[(1, &[1, 2]), (2, &[3]), (3, &[4])]);
        assert_eq!(exits(&[1, 2, 3], &reach), [(0, 1), (0, 2), (1, 3)]);
        assert!(
            exits(&[3], &reach).is_empty(),
            "one hop leaves at its only hop"
        );
    }

    #[test]
    fn a_control_plane_serving_a_later_hop_is_reached_from_that_one() {
        let reach = reach(&[(1, &[1]), (2, &[2])]);
        assert_eq!(exits(&[1, 2, 1], &reach), [(1, 2)]);
    }
}
