// Choosing what to buy, before buying it: two plans in simple mode, any
// number of connections in advanced. A plan is a number of connections — one
// per hop of the route — and costs one price per connection.
import { useState } from 'react';
import { useI18n } from '../i18n';
import type { useAccount } from '../lib/account';
import { MAX_SLOTS, type Mode, type Offer } from '../lib/core';
import { ProblemText } from './ProblemText';
import { Sheet } from './Sheet';
import { Faucet } from './SubscriptionsSheet';

const SYMBOL = { usdc: 'USDC', echo: 'ECHO' } as const;

/** The two simple plans, by the connections they carry. */
const PLANS = [1, 3] as const;

export function PlanSheet({
	sub,
	mode,
	busyNetwork,
	onMode,
	onBought,
	onClose,
}: {
	sub: ReturnType<typeof useAccount>;
	mode: Mode;
	busyNetwork: boolean;
	onMode: (mode: Mode) => void;
	/** After the purchase lands: how many connections were bought. */
	onBought: (slots: number) => void;
	onClose: () => void;
}) {
	const { t } = useI18n();
	const { account, working, error } = sub;
	const [slots, setSlots] = useState(1);
	// The stablecoin first; ECHO is offered when the contract takes it.
	const offer = account?.offers[0];
	const chosen = mode === 'simple' && !PLANS.includes(slots as (typeof PLANS)[number]) ? 1 : slots;

	const buy = async () => {
		if (!offer) return;
		if (await sub.buy(offer.payment, chosen)) onBought(chosen);
	};

	return (
		<Sheet title={t.choosePlan} onClose={onClose}>
			<div className="flex flex-col gap-5 pt-1">
				<ModeSwitch mode={mode} onMode={onMode} />

				{mode === 'simple' ? (
					<div className="flex flex-col gap-2.5">
						<PlanCard
							selected={chosen === 1}
							title={t.planStandard}
							hops={t.planStandardHops}
							detail={t.planStandardDetail}
							price={offer && t.perTerm(offer.totals[0], SYMBOL[offer.payment])}
							onSelect={() => setSlots(1)}
						/>
						<PlanCard
							selected={chosen === 3}
							title={t.planMax}
							hops={t.planMaxHops}
							detail={t.planMaxDetail}
							price={offer && t.perTerm(offer.totals[2], SYMBOL[offer.payment])}
							onSelect={() => setSlots(3)}
						/>
						<p className="px-1 text-[13px] leading-snug text-muted">{t.planPerDevice}</p>
					</div>
				) : (
					<div className="flex flex-col gap-1.5 rounded-2xl bg-bg px-3.5 py-3">
						<Stepper label={t.connections} value={chosen} max={MAX_SLOTS} onChange={setSlots} />
						<p className="text-[13px] leading-snug text-muted">
							{t.connectionsHint} {t.advancedHint}
						</p>
					</div>
				)}

				{offer && (
					<Pay
						offer={offer}
						slots={chosen}
						disabled={!!working || busyNetwork}
						working={working === 'buy'}
						onBuy={buy}
					/>
				)}
				{account?.faucet && offer && offer.affordable_slots < chosen && (
					<Faucet
						amount={account.faucet}
						disabled={!!working || busyNetwork}
						working={working === 'faucet'}
						onClick={sub.faucet}
					/>
				)}
				{busyNetwork && <p className="text-xs text-muted">{t.signingNeedsNetwork}</p>}
				{error && <ProblemText problem={error} />}
			</div>
		</Sheet>
	);
}

export function ModeSwitch({ mode, onMode }: { mode: Mode; onMode: (mode: Mode) => void }) {
	const { t } = useI18n();
	const option = (value: Mode, label: string) => (
		<button
			type="button"
			aria-pressed={mode === value}
			onClick={() => onMode(value)}
			className={`h-10 flex-1 rounded-full text-[14px] font-medium transition-colors ${
				mode === value ? 'bg-fg text-bg' : 'text-muted hover:text-fg'
			}`}
		>
			{label}
		</button>
	);
	return (
		<div className="flex gap-1 rounded-full bg-bg p-1">
			{option('simple', t.modeSimple)}
			{option('advanced', t.modeAdvanced)}
		</div>
	);
}

function PlanCard({
	selected,
	title,
	hops,
	detail,
	price,
	onSelect,
}: {
	selected: boolean;
	title: string;
	hops: string;
	detail: string;
	price?: string;
	onSelect: () => void;
}) {
	return (
		<button
			type="button"
			aria-pressed={selected}
			onClick={onSelect}
			className={`flex w-full flex-col gap-1 rounded-[18px] border px-4 py-3.5 text-left transition-colors ${
				selected ? 'border-brand/60 bg-brand/[0.08]' : 'border-line hover:bg-white/[0.03]'
			}`}
		>
			<span className="flex items-baseline justify-between gap-3">
				<span className="text-[16px] font-semibold">{title}</span>
				<span className="shrink-0 text-[13px] text-muted">{hops}</span>
			</span>
			<span className="text-[13px] leading-snug text-muted">{detail}</span>
			{price && <span className="mt-1 text-[14px] font-medium tabular-nums">{price}</span>}
		</button>
	);
}

export function Stepper({
	label,
	value,
	min = 1,
	max,
	onChange,
}: {
	label: string;
	value: number;
	min?: number;
	max: number;
	onChange: (n: number) => void;
}) {
	const { t } = useI18n();
	const step =
		'flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-white/[0.06] text-lg hover:bg-white/10 disabled:opacity-30';
	return (
		<div className="flex items-center justify-between gap-3">
			<span className="text-[15px]">{label}</span>
			<div className="flex items-center gap-3">
				<button type="button" aria-label={t.fewerConnections} disabled={value <= min} onClick={() => onChange(value - 1)} className={step}>
					−
				</button>
				<span className="w-5 text-center text-lg font-semibold tabular-nums">{value}</span>
				<button type="button" aria-label={t.moreConnections} disabled={value >= max} onClick={() => onChange(value + 1)} className={step}>
					+
				</button>
			</div>
		</div>
	);
}

function Pay({
	offer,
	slots,
	disabled,
	working,
	onBuy,
}: {
	offer: Offer;
	slots: number;
	disabled: boolean;
	working: boolean;
	onBuy: () => void;
}) {
	const { t } = useI18n();
	const short = offer.affordable_slots < slots;
	return (
		<div className="flex flex-col gap-1.5">
			<button
				type="button"
				disabled={disabled || short}
				onClick={onBuy}
				className="h-[52px] w-full rounded-full bg-brand text-[15px] font-semibold text-black hover:bg-brand-hover disabled:opacity-40"
			>
				{working ? t.flowInProgress : t.buyWith(offer.totals[slots - 1], SYMBOL[offer.payment])}
			</button>
			<p className="text-center text-xs text-muted">
				{short ? t.cannotAfford : `${t.balance}: ${offer.balance} ${SYMBOL[offer.payment]}`}
			</p>
			{offer.payment === 'usdc' && (
				<p className="text-center text-xs text-muted">{t.gasIncluded(offer.gas_fees[slots - 1])}</p>
			)}
		</div>
	);
}
