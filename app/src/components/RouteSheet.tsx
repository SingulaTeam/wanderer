// The route: a country, or any, for each hop between the person and the
// internet. Edited here and applied at once, so a half-built route never
// reconnects anything.
import { useMemo, useState } from 'react';
import { useI18n } from '../i18n';
import { MAX_HOPS, type Locations, type Mode, type Settings } from '../lib/core';
import { Flag } from './Flag';
import { Sheet } from './Sheet';
import { Toggle } from './Toggle';

type Route = Pick<Settings, 'route' | 'same_operator'>;

export function RouteSheet({
	locations,
	settings,
	mode,
	slots,
	live,
	onApply,
	onUpgrade,
	onClose,
}: {
	locations: Locations | null;
	settings: Settings;
	/** Simple: one hop or three, entry and exit chosen. Advanced: every hop. */
	mode: Mode;
	/** Connections of the subscription running now: one per hop. Null when unknown. */
	slots: number | null;
	/** Opens what lets the subscription carry three hops. */
	onUpgrade: () => void;
	/** Connected, or trying to: applying reconnects. */
	live: boolean;
	onApply: (route: Route) => void;
	onClose: () => void;
}) {
	const { t } = useI18n();
	const [route, setRoute] = useState<Route>(() => {
		if (mode === 'advanced') return { route: settings.route, same_operator: settings.same_operator };
		// Simple mode knows one hop or three: whatever advanced left is read
		// as its entry and, if the subscription carries three, its exit.
		const r = settings.route;
		const three = r.length >= 3 && (slots === null || slots >= 3);
		return {
			route: three ? [r[0] ?? null, null, r[r.length - 1] ?? null] : [r[0] ?? null],
			same_operator: settings.same_operator,
		};
	});
	// The hop whose country is being chosen.
	const [picking, setPicking] = useState<number | null>(null);

	// One operator runs every Control Plane: every hop is theirs whatever the
	// toggle says, so it is on and fixed. Fewer than two nodes: no chain.
	const oneOperator = locations?.control_plane_operators === 1;
	const canChain = !locations || locations.node_count >= 2;
	// Each hop holds one of the subscription's connections.
	const maxHops = Math.min(MAX_HOPS, slots ?? MAX_HOPS);
	const sameOperator = oneOperator || route.same_operator;

	const changed =
		(!oneOperator && route.same_operator !== settings.same_operator) ||
		route.route.length !== settings.route.length ||
		route.route.some((c, i) => c !== settings.route[i]);
	const hops = route.route;
	const setHops = (next: (string | null)[]) => setRoute({ ...route, route: next });

	if (picking !== null)
		return (
			<Sheet title={t.hop(picking + 1)} onClose={onClose}>
				<CountryPicker
					locations={locations}
					chosen={hops[picking] ?? null}
					onBack={() => setPicking(null)}
					onChoose={(code) => {
						setHops(hops.map((c, i) => (i === picking ? code : c)));
						setPicking(null);
					}}
				/>
			</Sheet>
		);

	if (mode === 'simple') {
		// One hop, or three with the middle left to the app. Hops never share
		// an operator, unless the network has only one.
		const three = hops.length >= 3 && (slots === null || slots >= 3);
		const entry = hops[0] ?? null;
		const exit = three ? (hops[hops.length - 1] ?? null) : null;
		const simpleRoute = (n: 1 | 3) => (n === 3 ? [entry, null, exit] : [entry]);
		const set = (next: (string | null)[]) => setRoute({ route: next, same_operator: oneOperator });
		const simpleChanged =
			hops.length !== settings.route.length || hops.some((c, i) => c !== settings.route[i]);
		return (
			<Sheet title={t.route} onClose={onClose}>
				<p className="px-1 text-[13px] leading-snug text-muted">{t.routeHint}</p>
				{(slots === null || slots >= 3) && canChain && (
					<div className="mt-4 flex gap-1 rounded-full bg-bg p-1">
						{([1, 3] as const).map((n) => (
							<button
								key={n}
								type="button"
								aria-pressed={(n === 3) === three}
								onClick={() => set(simpleRoute(n))}
								className={`h-10 flex-1 rounded-full text-[14px] font-medium transition-colors ${
									(n === 3) === three ? 'bg-fg text-bg' : 'text-muted hover:text-fg'
								}`}
							>
								{n === 3 ? t.hops3 : t.hops1}
							</button>
						))}
					</div>
				)}
				<ol className="relative mt-4">
					<span aria-hidden="true" className="absolute bottom-5 left-[28px] top-5 w-px bg-white/15" />
					<End label={t.you} icon={<Dot />} />
					<li>
						<HopRow
							index={0}
							code={entry}
							role={three ? t.roleEntry : t.roleOnly}
							locations={locations}
							onPick={() => setPicking(0)}
						/>
					</li>
					{three && (
						<>
							<li className="relative my-1.5 flex min-h-[52px] items-center gap-3 rounded-[18px] border border-dashed border-white/15 px-3">
								<span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-raised font-display text-xs font-bold text-muted">
									2
								</span>
								<span className="text-[13px] leading-snug text-muted">{t.middleAuto}</span>
							</li>
							<li>
								<HopRow
									index={2}
									code={exit}
									role={t.roleExit}
									locations={locations}
									onPick={() => setPicking(2)}
								/>
							</li>
						</>
					)}
					<End label={t.internet} icon={<Globe />} />
				</ol>
				{!canChain && <p className="mt-2 px-1 text-[13px] leading-snug text-hold-soft">{t.notEnoughNodes}</p>}
				{canChain && slots !== null && slots < 3 && (
					<div className="mt-3 flex flex-col gap-2 px-1">
						<p className="text-[13px] leading-snug text-muted">{t.upgradeForHops}</p>
						<button type="button" onClick={onUpgrade} className="self-start text-[14px] font-medium text-brand hover:text-brand-hover">
							{t.switchTo3} →
						</button>
					</div>
				)}
				{simpleChanged && (
					<button
						type="button"
						onClick={() => onApply({ route: hops, same_operator: oneOperator })}
						className="mt-5 h-[52px] w-full rounded-full bg-brand text-[15px] font-semibold text-black hover:bg-brand-hover"
					>
						{live ? t.applyAndReconnect : t.apply}
					</button>
				)}
			</Sheet>
		);
	}

	return (
		<Sheet title={t.route} onClose={onClose}>
			<p className="px-1 text-[13px] leading-snug text-muted">{t.routeHint}</p>

			<ol className="relative mt-4">
				{/* The rail the hops hang on, from the person to the internet. */}
				<span aria-hidden="true" className="absolute bottom-5 left-[28px] top-5 w-px bg-white/15" />
				<End label={t.you} icon={<Dot />} />
				{hops.map((code, i) => (
					<li key={i}>
						<HopRow
							index={i}
							code={code}
							role={role(t, i, hops.length)}
							locations={locations}
							onPick={() => setPicking(i)}
							onRemove={hops.length > 1 ? () => setHops(hops.filter((_, j) => j !== i)) : undefined}
						/>
					</li>
				))}
				<End label={t.internet} icon={<Globe />} />
			</ol>

			{!canChain && <p className="mt-2 px-1 text-[13px] leading-snug text-hold-soft">{t.notEnoughNodes}</p>}

			{/* Gone rather than greyed: at the limit nothing needs explaining —
			    unless the limit is the subscription's, which can be raised. */}
			{canChain && hops.length < maxHops && (
				<button
					type="button"
					onClick={() => setHops([...hops, null])}
					className="mt-2 flex h-12 w-full items-center justify-center gap-2 rounded-2xl border border-dashed border-white/20 text-[15px] text-fg hover:border-white/35 hover:bg-white/[0.03]"
				>
					<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" aria-hidden="true">
						<path d="M12 5v14M5 12h14" />
					</svg>
					{t.addHop}
					<span className="text-[13px] text-muted">· {t.hopsLimit(maxHops)}</span>
				</button>
			)}
			{canChain && slots !== null && slots < MAX_HOPS && hops.length === slots && (
				<p className="mt-2 px-1 text-[13px] leading-snug text-muted">{t.hopsBySlots(slots)}</p>
			)}
			{slots !== null && hops.length > slots && (
				<p className="mt-2 px-1 text-[13px] leading-snug text-hold-soft">{t.routeTooLong(slots)}</p>
			)}

			<label className="mt-6 flex items-start justify-between gap-4 px-1">
				<span className="flex flex-col gap-1">
					<span className="text-[15px] font-medium">{t.sameOperator}</span>
					<span className="text-[13px] leading-snug text-muted">{t.sameOperatorHint}</span>
				</span>
				<Toggle
					checked={sameOperator}
					disabled={oneOperator}
					onChange={(same_operator) => setRoute({ ...route, same_operator })}
				/>
			</label>
			{oneOperator && <p className="mt-2 px-1 text-[13px] leading-snug text-muted">{t.oneOperatorOnly}</p>}

			{hops.length > 1 && <p className="mt-5 px-1 text-[13px] leading-snug text-hold-soft">{t.permissionsUsed(hops.length)}</p>}

			{changed && (
				<button
					type="button"
					onClick={() => onApply({ ...route, same_operator: sameOperator })}
					className="mt-5 h-[52px] w-full rounded-full bg-brand text-[15px] font-semibold text-black hover:bg-brand-hover"
				>
					{live ? t.applyAndReconnect : t.apply}
				</button>
			)}
		</Sheet>
	);
}

/** What a hop at `i` of `count` sees. */
function role(t: ReturnType<typeof useI18n>['t'], i: number, count: number): string {
	if (count === 1) return t.roleOnly;
	if (i === 0) return t.roleEntry;
	if (i === count - 1) return t.roleExit;
	return t.roleMiddle;
}

function HopRow({
	index,
	code,
	role,
	locations,
	onPick,
	onRemove,
}: {
	index: number;
	code: string | null;
	role: string;
	locations: Locations | null;
	onPick: () => void;
	onRemove?: () => void;
}) {
	const { t, country } = useI18n();
	const known = code ? locations?.countries.find((c) => c.code === code) : undefined;
	return (
		<div className="relative my-1.5 flex items-center gap-1 rounded-[18px] border border-line bg-bg">
			<button type="button" onClick={onPick} className="flex min-h-[64px] min-w-0 flex-1 items-center gap-3 py-2 pl-3 text-left">
				<span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-raised font-display text-xs font-bold text-muted">
					{index + 1}
				</span>
				{code ? (
					<Flag code={code} size={36} />
				) : (
					<span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-brand/[0.12] text-brand">
						<Bolt />
					</span>
				)}
				<span className="flex min-w-0 flex-1 flex-col gap-0.5">
					<span className="truncate text-[15px] font-medium">
						{code ? country(code) : t.anyCountry}
						{code && !known && locations && <span className="text-[13px] font-normal text-hold-soft"> · {t.stars(0)}</span>}
					</span>
					<span className="text-[12.5px] leading-snug text-muted">{role}</span>
				</span>
			</button>
			{onRemove ? (
				<button
					type="button"
					aria-label={t.removeHop(index + 1)}
					onClick={onRemove}
					className="mr-1 flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-muted hover:bg-white/[0.06] hover:text-fg"
				>
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" aria-hidden="true">
						<path d="M6 6l12 12M18 6 6 18" />
					</svg>
				</button>
			) : (
				<svg className="mr-3.5 shrink-0" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#a1a1aa" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
					<path d="m9 6 6 6-6 6" />
				</svg>
			)}
		</div>
	);
}

/** The person's end of the route, or the internet's. */
function End({ label, icon }: { label: string; icon: React.ReactNode }) {
	return (
		<li className="relative flex h-10 items-center gap-3 pl-3">
			<span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-card text-muted">{icon}</span>
			<span className="text-[13px] text-muted">{label}</span>
		</li>
	);
}

function CountryPicker({
	locations,
	chosen,
	onBack,
	onChoose,
}: {
	locations: Locations | null;
	chosen: string | null;
	onBack: () => void;
	onChoose: (code: string | null) => void;
}) {
	const { t, country } = useI18n();
	const [query, setQuery] = useState('');

	const shown = useMemo(() => {
		const q = query.trim().toLowerCase();
		const all = locations?.countries ?? [];
		return q ? all.filter((c) => country(c.code).toLowerCase().includes(q) || c.code.toLowerCase() === q) : all;
	}, [locations, query, country]);

	return (
		<>
			<button type="button" onClick={onBack} className="-ml-1 mb-3 flex h-11 items-center gap-1.5 px-1 text-[15px] text-muted hover:text-fg">
				<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
					<path d="m15 6-6 6 6 6" />
				</svg>
				{t.back}
			</button>

			<label className="flex h-12 items-center gap-2.5 rounded-2xl border border-line bg-bg px-3.5">
				<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#a1a1aa" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
					<circle cx="11" cy="11" r="6.5" />
					<path d="m16 16 4 4" />
				</svg>
				<span className="sr-only">{t.searchCountries}</span>
				<input
					type="search"
					value={query}
					onChange={(e) => setQuery(e.target.value)}
					placeholder={t.searchCountries}
					className="min-w-0 flex-1 bg-transparent text-[15px] text-fg outline-none placeholder:text-muted"
				/>
			</label>

			<Row
				selected={chosen === null}
				onClick={() => onChoose(null)}
				badge={<Bolt />}
				badgeClass="bg-brand/15 text-brand"
				title={t.anyCountry}
				detail={t.autoDetail}
			/>

			<p className="mx-1 mb-1.5 mt-5 font-display text-xs tracking-[0.08em] text-muted">{t.countries.toUpperCase()}</p>

			{!locations && <p className="px-1 py-4 text-sm text-muted">{t.locationsUnavailable}</p>}
			{locations && shown.length === 0 && <p className="px-1 py-4 text-sm text-muted">{t.noCountries}</p>}
			<ul className="flex flex-col">
				{shown.map((c) => (
					<li key={c.code}>
						<Row
							selected={chosen === c.code}
							onClick={() => onChoose(c.code)}
							badge={<Flag code={c.code} size={40} />}
							badgeClass=""
							title={country(c.code)}
							detail={`${t.stars(c.nodes)} · ${t.load(Math.round(c.load * 100))}`}
							load={c.load}
							plain
						/>
					</li>
				))}
			</ul>
		</>
	);
}

function Row({
	selected,
	onClick,
	badge,
	badgeClass,
	title,
	detail,
	load,
	plain,
}: {
	selected: boolean;
	onClick: () => void;
	badge: React.ReactNode;
	badgeClass: string;
	title: string;
	detail: string;
	load?: number;
	plain?: boolean;
}) {
	const frame = selected
		? 'border border-brand/45 bg-brand/[0.08] rounded-[18px]'
		: plain
			? 'border-b border-white/5'
			: 'border border-line rounded-[18px]';
	return (
		<button
			type="button"
			aria-pressed={selected}
			onClick={onClick}
			className={`mt-2.5 flex min-h-[60px] w-full items-center gap-3.5 px-3 py-2 text-left transition-colors hover:bg-white/[0.03] ${frame}`}
		>
			<span className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-full ${badgeClass}`}>{badge}</span>
			<span className="flex min-w-0 flex-1 flex-col gap-0.5">
				<span className="truncate text-[15px] font-medium">{title}</span>
				<span className="text-[13px] text-muted">{detail}</span>
			</span>
			{load !== undefined && <LoadMeter load={load} />}
			{selected && (
				<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#fe5001" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
					<path d="m5 12 4.5 4.5L19 7" />
				</svg>
			)}
		</button>
	);
}

/** How full the nodes are: a short bar, filled by the load. */
function LoadMeter({ load }: { load: number }) {
	const percent = Math.round(Math.min(Math.max(load, 0), 1) * 100);
	const color = percent < 60 ? 'bg-ok' : percent < 85 ? 'bg-hold' : 'bg-red-400';
	return (
		<span aria-hidden="true" className="h-1.5 w-10 shrink-0 overflow-hidden rounded-full bg-white/10">
			<span style={{ width: `${Math.max(percent, 4)}%` }} className={`block h-full rounded-full ${color}`} />
		</span>
	);
}

export function Bolt() {
	return (
		<svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
			<path d="M13 2 4 14h7l-1 8 9-12h-7l1-8z" />
		</svg>
	);
}

function Dot() {
	return <span className="h-2 w-2 rounded-full bg-fg" />;
}

function Globe() {
	return (
		<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" aria-hidden="true">
			<circle cx="12" cy="12" r="9" />
			<path d="M3 12h18M12 3c2.5 2.6 3.8 5.6 3.8 9s-1.3 6.4-3.8 9c-2.5-2.6-3.8-5.6-3.8-9S9.5 5.6 12 3z" />
		</svg>
	);
}
