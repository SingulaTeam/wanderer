// The home screen: the orbit, where it connects, and what is going on.
import { useCallback, useEffect, useState } from 'react';
import { useActiveWallet, useDisconnect } from 'thirdweb/react';
import { Logo } from '../components/Logo';
import { OrbitButton, type Orbit } from '../components/OrbitButton';
import { Bolt, RouteSheet } from '../components/RouteSheet';
import { SettingsSheet } from '../components/SettingsSheet';
import { DiagnosticsSheet } from '../components/DiagnosticsSheet';
import { SubscriptionCard } from '../components/SubscriptionCard';
import { SubscriptionsSheet } from '../components/SubscriptionsSheet';
import { PlanSheet } from '../components/PlanSheet';
import { SigningFlow } from '../components/SigningFlow';
import { Flag } from '../components/Flag';
import { WindowControls } from '../components/WindowControls';
import { useI18n } from '../i18n';
import { useAccount } from '../lib/account';
import { useExplain } from '../lib/problem';
import {
	core,
	onReadiness,
	onState,
	onTraffic,
	type Locations,
	type Readiness,
	type Settings,
	type State,
	type Traffic,
} from '../lib/core';

export default function Home({ address }: { address: string }) {
	const { t, country, locale } = useI18n();
	const [readiness, setReadiness] = useState<Readiness>({ ready: 'starting' });
	const [state, setState] = useState<State>({ state: 'disconnected' });
	const [traffic, setTraffic] = useState<Traffic>({ up: 0, down: 0 });
	const [settings, setSettings] = useState<Settings | null>(null);
	const [locations, setLocations] = useState<Locations | null>(null);
	const [sheet, setSheet] = useState<'route' | 'settings' | 'subscriptions' | 'diagnostics' | 'plan' | null>(null);
	const [since, setSince] = useState<number | null>(null);
	const [now, setNow] = useState(Date.now());
	const wallet = useActiveWallet();
	const { disconnect } = useDisconnect();
	const sub = useAccount(readiness.ready === 'yes');
	const explain = useExplain();

	useEffect(() => {
		core.readiness().then(setReadiness).catch(console.error);
		core.state().then(setState).catch(console.error);
		core.settings().then(setSettings).catch(console.error);
		const stops = [onReadiness(setReadiness), onState(setState), onTraffic(setTraffic)];
		return () => {
			stops.forEach((s) => s.then((f) => f()));
		};
	}, []);

	const loadLocations = useCallback(() => {
		core.locations().then(setLocations).catch(() => setLocations(null));
	}, []);
	useEffect(() => {
		if (readiness.ready === 'yes') loadLocations();
	}, [readiness.ready, loadLocations]);

	// The session's clock starts when it first carries traffic, and keeps
	// running through a reconnect.
	useEffect(() => {
		if (state.state === 'connected') setSince((s) => s ?? Date.now());
		else if (state.state !== 'holding') setSince(null);
	}, [state.state]);
	useEffect(() => {
		if (since === null) return;
		const timer = setInterval(() => setNow(Date.now()), 1000);
		return () => clearInterval(timer);
	}, [since]);

	const save = (next: Settings) => {
		setSettings(next);
		core.saveSettings(next).catch(console.error);
	};

	const s = state.state;
	const live = s === 'connecting' || s === 'connected' || s === 'holding';
	const orbit: Orbit = s === 'connected' ? 'on' : s === 'holding' ? 'holding' : s === 'connecting' ? 'connecting' : 'idle';
	const press = () => (live ? core.disconnect() : core.connect()).catch(console.error);

	// A plan bought in simple mode is the route it was bought for.
	const bought = (slots: number) => {
		setSheet(null);
		if (!settings || settings.mode !== 'simple') return;
		const r = settings.route;
		save({ ...settings, route: slots >= 3 ? [r[0] ?? null, null, r[r.length - 1] ?? null] : [r[0] ?? null] });
	};

	const apply = (route: Pick<Settings, 'route' | 'same_operator'>) => {
		if (!settings) return;
		const next = { ...settings, ...route };
		setSettings(next);
		setSheet(null);
		// Connected elsewhere: move there now; the guard holds throughout.
		core
			.saveSettings(next)
			.then(() => (live ? core.connect() : undefined))
			.catch(console.error);
	};

	// Where the chain runs: the nodes' countries once connected, else the
	// countries asked for.
	const route: (string | null)[] =
		s === 'connected' ? state.nodes.map((n) => locations?.nodes[String(n)] ?? null) : (settings?.route ?? [null]);
	const exitCountry = s === 'connected' ? route[route.length - 1] : null;
	const until = sub.active?.expires_at
		? new Date(sub.active.expires_at * 1000).toLocaleDateString(locale, { day: 'numeric', month: 'short' })
		: null;

	let title = t.notProtected;
	let detail = t.notProtectedDetail;
	if (readiness.ready === 'starting') [title, detail] = [t.starting, ''];
	else if (readiness.ready === 'no') [title, detail] = [t.notReady, readiness.reason];
	else if (s === 'connecting') [title, detail] = [t.connecting, t.connectingDetail];
	else if (s === 'connected')
		[title, detail] = [
			t.protected,
			[exitCountry && country(exitCountry), state.transport === 'xhttp' ? 'xHTTP' : 'Vision', state.cover]
				.filter(Boolean)
				.join(' · '),
		];
	else if (s === 'holding')
		[title, detail] = [
			t.reconnecting,
			state.problem.kind === 'session_releasing' || state.problem.kind === 'route'
				? explain(state.problem)
				: t.reconnectingDetail,
		];
	else if (s === 'failed') [title, detail] = [t.stopped, explain(state.problem)];
	else if (readiness.ready === 'yes' && sub.account && !sub.active) detail = t.needSubscription;

	return (
		<div className="relative mx-auto flex h-full max-w-md flex-col overflow-hidden">
			<Glow orbit={orbit} />

			{/* The title bar: drag the window by it. Its children let clicks
			    through to it, apart from the buttons. */}
			<header data-tauri-drag-region className="relative flex items-center justify-between gap-2 px-5 pt-4">
				<div className="pointer-events-none">
					<Logo />
				</div>
				<div data-tauri-drag-region className="h-11 flex-1" />
				<button
					type="button"
					aria-label={t.settings}
					onClick={() => setSheet('settings')}
					className="flex h-11 w-11 items-center justify-center rounded-full border border-line bg-card hover:border-white/20"
				>
					<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
						<path d="M4 7h10M18 7h2M4 17h4M12 17h8" />
						<circle cx="16" cy="7" r="2" />
						<circle cx="10" cy="17" r="2" />
					</svg>
				</button>
				<WindowControls />
			</header>

			<div className="relative flex min-h-[44px] justify-center px-5 pt-3.5">
				<Chip
					s={s}
					since={since}
					now={now}
					until={until}
					hasSubscription={!!sub.active}
					onSubscriptions={() => setSheet('subscriptions')}
				/>
			</div>

			<main className="relative flex min-h-0 flex-1 flex-col items-center justify-center gap-8 px-5">
				<div className="flex flex-col gap-2 text-center">
					<h1 className="text-[26px] font-semibold tracking-tight">{title}</h1>
					{detail && <p className="mx-auto max-w-[300px] break-words text-sm leading-snug text-muted">{detail}</p>}
				</div>

				<OrbitButton
					orbit={orbit}
					disabled={!live && (readiness.ready !== 'yes' || !sub.active)}
					onPress={press}
				/>

				{s === 'connected' && (
					<div className="grid w-full grid-cols-2 gap-2.5">
						<Stat label={t.down} value={bytes(traffic.down)} />
						<Stat label={t.up} value={bytes(traffic.up)} />
					</div>
				)}
				{readiness.ready === 'no' && (
					<button type="button" onClick={() => core.retry().catch(console.error)} className="min-h-11 text-sm text-brand hover:text-brand-hover">
						{t.tryAgain}
					</button>
				)}
			</main>

			<footer className="relative flex flex-col gap-3 px-5 pb-7">
				{s === 'holding' ? (
					<>
						<button
							type="button"
							onClick={press}
							className="min-h-[52px] w-full rounded-full border border-white/[0.12] text-[15px] font-medium hover:bg-white/[0.04]"
						>
							{t.disconnectAndLift}
						</button>
						<p className="text-center text-xs leading-snug text-muted">{t.liftWarning}</p>
					</>
				) : readiness.ready === 'yes' && sub.account && !sub.active ? (
					<SubscriptionCard
						sub={sub}
						busyNetwork={live}
						onManage={() => setSheet('subscriptions')}
						onChoosePlan={() => setSheet('plan')}
					/>
				) : (
					<RouteCard
						route={route}
						star={s === 'connected' && state.nodes.length === 1 ? state.nodes[0] : null}
						count={locations?.countries.length ?? null}
						onOpen={() => {
							loadLocations();
							setSheet('route');
						}}
					/>
				)}
				{s !== 'holding' && (
					<div className="flex flex-wrap justify-center gap-2">
						<Pill>
							{t.transport} ·{' '}
							{settings?.transport === 'vision' ? 'Vision' : settings?.transport === 'xhttp' ? 'xHTTP' : t.transportAuto}
						</Pill>
						<Pill>{t.killSwitchOn}</Pill>
					</div>
				)}
			</footer>

			{sheet === 'route' && settings && (
				<RouteSheet
					locations={locations}
					settings={settings}
					mode={settings.mode}
					slots={sub.current?.slots ?? null}
					live={live}
					onApply={apply}
					onUpgrade={() => setSheet('subscriptions')}
					onClose={() => setSheet(null)}
				/>
			)}
			{sheet === 'settings' && settings && (
				<SettingsSheet
					settings={settings}
					address={address}
					onChange={save}
					onSignOut={() => wallet && disconnect(wallet)}
					onOpen={setSheet}
					onClose={() => setSheet(null)}
				/>
			)}
			{sheet === 'subscriptions' && (
				<SubscriptionsSheet
					sub={sub}
					mode={settings?.mode ?? 'simple'}
					busyNetwork={live}
					onChoosePlan={() => setSheet('plan')}
					onClose={() => setSheet(null)}
				/>
			)}
			{sheet === 'plan' && settings && (
				<PlanSheet
					sub={sub}
					mode={settings.mode}
					busyNetwork={live}
					onMode={(mode) => save({ ...settings, mode })}
					onBought={bought}
					onClose={() => setSheet(null)}
				/>
			)}
			{sheet === 'diagnostics' && <DiagnosticsSheet onClose={() => setSheet(null)} />}
			<SigningFlow />
		</div>
	);
}

function Glow({ orbit }: { orbit: Orbit }) {
	const cls =
		orbit === 'on'
			? 'left-1/2 top-[150px] h-[300px] w-[300px] -translate-x-1/2 bg-brand/20'
			: orbit === 'holding'
				? 'left-1/2 top-[170px] h-[240px] w-[240px] -translate-x-1/2 bg-hold/10'
				: '-right-36 -top-44 h-[420px] w-[420px] bg-brand/10';
	return <div aria-hidden="true" className={`pointer-events-none absolute rounded-full blur-[90px] transition-all duration-700 ${cls}`} />;
}

function Chip({
	s,
	since,
	now,
	until,
	hasSubscription,
	onSubscriptions,
}: {
	s: State['state'];
	since: number | null;
	now: number;
	until: string | null;
	hasSubscription: boolean;
	onSubscriptions: () => void;
}) {
	const { t } = useI18n();
	if (s === 'holding')
		return (
			<span className="inline-flex h-[30px] items-center gap-2 rounded-full border border-hold/35 bg-hold/[0.08] px-3 text-xs text-hold-soft">
				<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
					<rect x="5" y="11" width="14" height="10" rx="2" />
					<path d="M8 11V8a4 4 0 0 1 8 0v3" />
				</svg>
				{t.killSwitchHolding}
			</span>
		);
	if (s === 'connected' && since !== null)
		return (
			<span className="inline-flex h-[30px] items-center gap-2 rounded-full border border-brand/35 bg-brand/[0.08] px-3 text-xs tabular-nums text-brand-soft">
				<span className="h-1.5 w-1.5 rounded-full bg-brand" />
				{t.session(clock(now - since))}
			</span>
		);
	if (!hasSubscription || !until) return null;
	return (
		<button
			type="button"
			onClick={onSubscriptions}
			className="inline-flex h-[30px] items-center gap-2 rounded-full border border-line bg-card px-3 text-xs text-muted hover:border-white/20 hover:text-fg"
		>
			<span className="h-1.5 w-1.5 rounded-full bg-ok" />
			{t.subscriptionActive(until)}
		</button>
	);
}

function RouteCard({
	route,
	star,
	count,
	onOpen,
}: {
	/** A country for each hop; null for any. */
	route: (string | null)[];
	/** The node of a single-hop connection. */
	star: number | null;
	count: number | null;
	onOpen: () => void;
}) {
	const { t, country } = useI18n();
	const single = route.length === 1;
	const code = route[0] ?? null;
	const title = single ? (code ? country(code) : t.auto) : t.hopsCount(route.length);
	const detail = !single
		? route.map((c) => (c ? country(c) : t.anyCountry)).join(' → ')
		: star !== null
			? t.star(star)
			: code
				? t.location
				: count !== null
					? `${t.autoDetail} · ${t.countriesCount(count)}`
					: t.autoDetail;
	return (
		<button
			type="button"
			onClick={onOpen}
			className="flex min-h-[68px] w-full items-center gap-3.5 rounded-[20px] border border-line bg-card px-4 py-3 text-left hover:border-white/20"
		>
			<span className="flex shrink-0">
				{route.map((c, i) => (
					<span key={i} className={`rounded-full ${i > 0 ? '-ml-3 ring-2 ring-card' : ''}`}>
						{c ? (
							<Flag code={c} size={single ? 42 : 34} />
						) : (
							<span
								style={{ width: single ? 42 : 34, height: single ? 42 : 34 }}
								className="flex items-center justify-center rounded-full bg-[#2a1a12] text-brand"
							>
								<Bolt />
							</span>
						)}
					</span>
				))}
			</span>
			<span className="flex min-w-0 flex-1 flex-col gap-0.5">
				<span className="truncate text-[15px] font-medium">{title}</span>
				<span className="truncate text-[13px] text-muted">{detail}</span>
			</span>
			<svg className="shrink-0" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#a1a1aa" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
				<path d="m9 6 6 6-6 6" />
			</svg>
		</button>
	);
}

function Stat({ label, value }: { label: string; value: string }) {
	return (
		<div className="flex flex-col gap-1 rounded-2xl bg-white/[0.04] px-3.5 py-3">
			<span className="text-xs text-muted">{label}</span>
			<span className="text-lg font-semibold tabular-nums">{value}</span>
		</div>
	);
}

function Pill({ children }: { children: React.ReactNode }) {
	return <span className="inline-flex h-7 items-center rounded-full bg-white/[0.04] px-3 text-xs text-muted">{children}</span>;
}

function clock(ms: number): string {
	const total = Math.max(0, Math.floor(ms / 1000));
	const pad = (n: number) => String(n).padStart(2, '0');
	return `${pad(Math.floor(total / 3600))}:${pad(Math.floor((total % 3600) / 60))}:${pad(total % 60)}`;
}

function bytes(n: number): string {
	const units = ['B', 'KB', 'MB', 'GB', 'TB'];
	let i = 0;
	while (n >= 1024 && i < units.length - 1) {
		n /= 1024;
		i++;
	}
	return `${n.toFixed(i ? 1 : 0)} ${units[i]}`;
}
