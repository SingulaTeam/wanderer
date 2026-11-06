// Every subscription the wallet holds, and buying another.
import { useEffect, useState } from 'react';
import { useI18n } from '../i18n';
import type { useAccount } from '../lib/account';
import { ProblemText } from './ProblemText';
import { core, MAX_SLOTS, type Mode, type Quote, type Subscription } from '../lib/core';
import { Stepper } from './PlanSheet';
import { Sheet } from './Sheet';

const SYMBOL = { usdc: 'USDC', echo: 'ECHO' } as const;

export function SubscriptionsSheet({
	sub,
	mode,
	busyNetwork,
	onChoosePlan,
	onClose,
}: {
	sub: ReturnType<typeof useAccount>;
	mode: Mode;
	busyNetwork: boolean;
	onChoosePlan: () => void;
	onClose: () => void;
}) {
	const { t, locale } = useI18n();
	const { account, working, error } = sub;
	// A subscription in reserve — queued, or bought and not activated — whose
	// connections are being added to.
	const [adding, setAdding] = useState<number | null>(null);
	const date = (unix: number) =>
		new Date(unix * 1000).toLocaleDateString(locale, { day: 'numeric', month: 'long', year: 'numeric' });
	const now = Date.now() / 1000;
	// Newest first: the one bought last is the one being looked for.
	const owned = [...(account?.subscriptions ?? [])].sort((a, b) => b.token - a.token);

	return (
		<Sheet title={t.subscriptions} onClose={onClose}>
			<div className="flex flex-col gap-6 pt-1">
				{account && account.offers.length > 0 && (
					<div className="grid grid-cols-2 gap-2.5">
						{account.offers.map((o) => (
							<div key={o.payment} className="flex flex-col gap-1 rounded-2xl bg-bg px-3.5 py-3">
								<span className="text-xs text-muted">
									{t.balance} · {SYMBOL[o.payment]}
								</span>
								<span className="truncate text-lg font-semibold tabular-nums">{o.balance}</span>
							</div>
						))}
					</div>
				)}
				{account?.faucet && (
					<Faucet
						amount={account.faucet}
						disabled={!!working || busyNetwork}
						working={working === 'faucet'}
						onClick={sub.faucet}
					/>
				)}

				<ul className="flex flex-col gap-2.5">
					{owned.length === 0 && <li className="text-sm text-muted">{t.noSubscriptionsYet}</li>}
					{owned.map((s) => (
						<li key={s.token}>
							<Row
								s={s}
								status={
									s.active && s.expires_at && s.starts_at && s.starts_at > now
										? t.queued(date(s.starts_at), date(s.expires_at))
										: s.active && s.expires_at
											? t.activeUntil(date(s.expires_at))
											: s.expires_at && s.expires_at < now
												? t.expiredOn(date(s.expires_at))
												: t.notActivated
								}
								onActivate={
									!s.active && s.expires_at === null
										? () => sub.activate(s.token)
										: undefined
								}
								disabled={!!working || busyNetwork}
								working={working === 'activate'}
								onAdd={
									reserve(s, now) && s.slots < (mode === 'simple' ? 3 : MAX_SLOTS)
										? () => setAdding(adding === s.token ? null : s.token)
										: undefined
								}
								adding={adding === s.token}
							/>
							{adding === s.token && (
								<div className="mt-2">
									<Upgrade
										s={s}
										mode={mode}
										whole
										disabled={!!working || busyNetwork}
										working={working === 'extend'}
										onAdd={(extra) => sub.addSlots(s.token, extra)}
									/>
								</div>
							)}
						</li>
					))}
				</ul>

				{sub.current && (
					<Upgrade
						s={sub.current}
						mode={mode}
						disabled={!!working || busyNetwork}
						working={working === 'extend'}
						onAdd={(extra) => sub.addSlots(sub.current!.token, extra)}
					/>
				)}

				{account && (
					<div className="flex flex-col gap-2.5">
						<button
							type="button"
							disabled={!!working || busyNetwork}
							onClick={onChoosePlan}
							className="h-12 w-full rounded-full border border-brand/50 text-[15px] font-medium text-fg hover:bg-brand/10 disabled:opacity-40"
						>
							{t.buyAnother}
						</button>
						{owned.length > 0 && <p className="text-[13px] leading-snug text-muted">{t.stackHint}</p>}
					</div>
				)}

				{busyNetwork && <p className="text-xs text-muted">{t.signingNeedsNetwork}</p>}
				{error && <ProblemText problem={error} />}
			</div>
		</Sheet>
	);
}

/** Queued behind the one running, or bought and not activated yet. */
function reserve(s: Subscription, now: number): boolean {
	const queued = s.active && s.starts_at !== null && s.starts_at > now;
	const unactivated = !s.active && s.expires_at === null;
	return queued || unactivated;
}

function Row({
	s,
	status,
	onActivate,
	onAdd,
	adding,
	disabled,
	working,
}: {
	s: Subscription;
	status: string;
	onActivate?: () => void;
	/** Opens adding connections to it. */
	onAdd?: () => void;
	adding?: boolean;
	disabled: boolean;
	working: boolean;
}) {
	const { t } = useI18n();
	return (
		<div
			className={`flex min-h-[60px] items-center gap-3 rounded-[18px] border px-3.5 py-2.5 ${
				s.active ? 'border-brand/45 bg-brand/[0.08]' : 'border-line'
			}`}
		>
			<span className={`h-2 w-2 shrink-0 rounded-full ${s.active ? 'bg-ok' : 'bg-white/25'}`} />
			<span className="flex min-w-0 flex-1 flex-col gap-0.5">
				<span className="text-[15px] font-medium">{t.subscription(s.token)}</span>
				<span className="text-[13px] text-muted">
					{t.connectionsCount(s.slots)} · {status}
				</span>
			</span>
			{onAdd && (
				<button
					type="button"
					aria-label={t.addConnections}
					aria-expanded={adding}
					disabled={disabled}
					onClick={onAdd}
					className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-lg disabled:opacity-40 ${
						adding ? 'bg-brand/20 text-brand' : 'bg-white/[0.06] hover:bg-white/10'
					}`}
				>
					+
				</button>
			)}
			{onActivate && (
				<button
					type="button"
					disabled={disabled}
					onClick={onActivate}
					className="h-10 shrink-0 rounded-full bg-brand px-4 text-sm font-semibold text-bg hover:bg-brand-hover disabled:opacity-40"
				>
					{working ? t.flowInProgress : t.activate}
				</button>
			)}
		</div>
	);
}

// More connections for the subscription running now, for the rest of its
// term: in simple mode the step to three hops, in advanced any number up to
// the most a subscription carries.
function Upgrade({
	s,
	mode,
	whole,
	disabled,
	working,
	onAdd,
}: {
	s: Subscription;
	mode: Mode;
	/** Its whole term is ahead: priced in full, not for the days left. */
	whole?: boolean;
	disabled: boolean;
	working: boolean;
	onAdd: (extra: number) => void;
}) {
	const { t } = useI18n();
	const room = MAX_SLOTS - s.slots;
	const [extra, setExtra] = useState(1);
	const wanted = mode === 'simple' ? 3 - s.slots : Math.min(extra, room);
	const [quote, setQuote] = useState<Quote | null>(null);

	useEffect(() => {
		if (wanted < 1) return;
		let live = true;
		setQuote(null);
		core
			.addSlotsQuote(s.token, wanted)
			.then((q) => live && setQuote(q))
			.catch(() => live && setQuote(null));
		return () => {
			live = false;
		};
	}, [s.token, wanted]);

	if (mode === 'simple' ? s.slots >= 3 : room < 1) return null;
	return (
		<div className="flex flex-col gap-2.5 rounded-[18px] border border-line px-3.5 py-3">
			<span className="text-[15px] font-medium">{mode === 'simple' ? t.upgradeTitle : t.addConnections}</span>
			{mode === 'advanced' && <Stepper label={t.connections} value={wanted} max={room} onChange={setExtra} />}
			<p className="text-[13px] leading-snug text-muted">
				{quote === null
					? '…'
					: whole
						? t.reserveDetail(quote.total)
						: mode === 'simple'
							? t.upgradeDetail(quote.total, 'USDC')
							: t.addConnectionsDetail(quote.total, 'USDC')}
				{quote !== null && ` ${t.gasIncluded(quote.gas_fee)}.`}
			</p>
			<button
				type="button"
				disabled={disabled || quote === null}
				onClick={() => onAdd(wanted)}
				className="h-11 w-full rounded-full bg-brand text-[15px] font-semibold text-black hover:bg-brand-hover disabled:opacity-40"
			>
				{working ? t.flowInProgress : mode === 'simple' ? t.upgrade : t.add}
			</button>
		</div>
	);
}

export function Faucet({
	amount,
	disabled,
	working,
	onClick,
}: {
	amount: string;
	disabled: boolean;
	working: boolean;
	onClick: () => void;
}) {
	const { t } = useI18n();
	return (
		<div className="flex flex-col gap-1.5">
			<button
				type="button"
				disabled={disabled}
				onClick={onClick}
				className="h-11 w-full rounded-full bg-white/[0.06] text-[15px] font-medium hover:bg-white/10 disabled:opacity-40"
			>
				{working ? t.flowInProgress : t.faucet(amount)}
			</button>
			<p className="text-center text-xs text-muted">{t.faucetHint}</p>
		</div>
	);
}
