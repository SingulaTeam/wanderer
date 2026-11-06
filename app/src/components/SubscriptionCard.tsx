// Buying or activating a subscription, shown where the location card would
// be until there is an active one.
import { useI18n } from '../i18n';
import type { useAccount } from '../lib/account';
import { ProblemText } from './ProblemText';
import { Faucet } from './SubscriptionsSheet';

export function SubscriptionCard({
	sub,
	busyNetwork,
	onManage,
	onChoosePlan,
}: {
	sub: ReturnType<typeof useAccount>;
	busyNetwork: boolean;
	onManage: () => void;
	/** Buying starts with choosing what to buy. */
	onChoosePlan: () => void;
}) {
	const { t } = useI18n();
	const { account, waiting, working, error } = sub;
	// The stablecoin first; other ways to pay are a tap away.
	const offer = account?.offers[0];

	return (
		<section className="flex flex-col gap-2 rounded-[20px] border border-line bg-card p-4">
			{waiting ? (
				<div className="flex items-center justify-between gap-4">
					<span className="text-[15px]">{t.bought(waiting.token)}</span>
					<Action
						label={t.activate}
						working={working === 'activate'}
						disabled={!!working || busyNetwork}
						onClick={() => sub.activate(waiting.token)}
					/>
				</div>
			) : (
				<div className="flex items-center justify-between gap-4">
					<span className="flex flex-col gap-0.5">
						<span className="text-[15px] font-medium">{t.noSubscription}</span>
						{offer && (
							<span className="text-[13px] text-muted">
								{t.price(offer.price, offer.balance)}
							</span>
						)}
					</span>
					<Action
						label={t.choosePlan}
						working={working === 'buy'}
						disabled={!!working || busyNetwork || !offer}
						onClick={onChoosePlan}
					/>
				</div>
			)}
			{!waiting && offer && !offer.can_afford && account?.faucet && (
				<Faucet
					amount={account.faucet}
					disabled={!!working || busyNetwork}
					working={working === 'faucet'}
					onClick={sub.faucet}
				/>
			)}
			<button type="button" onClick={onManage} className="self-start text-[13px] text-brand hover:text-brand-hover">
				{t.subscriptions} →
			</button>
			{busyNetwork && <p className="text-xs text-muted">{t.signingNeedsNetwork}</p>}
			{error && <ProblemText problem={error} />}
		</section>
	);
}

function Action(props: { label: string; working: boolean; disabled: boolean; onClick: () => void }) {
	const { t } = useI18n();
	return (
		<button
			type="button"
			disabled={props.disabled}
			onClick={props.onClick}
			className="h-11 shrink-0 rounded-full bg-brand px-5 text-[15px] font-semibold text-bg hover:bg-brand-hover disabled:opacity-40"
		>
			{props.working ? t.flowInProgress : props.label}
		</button>
	);
}
