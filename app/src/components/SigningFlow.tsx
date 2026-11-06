// The steps of a flow that needs the wallet, over everything else: which
// signature the wallet is showing now and what it is for, which are done,
// and what happens once they all are.
import { useI18n } from '../i18n';
import { signing, useSigning, type Step } from '../lib/signing';
import { ProblemText } from './ProblemText';

export function SigningFlow() {
	const { t } = useI18n();
	const s = useSigning();
	if (!s) return null;

	const waiting = s.steps.find((x) => x.state === 'waiting');
	const finished = s.phase === 'done' || s.phase === 'failed';
	const title =
		s.flow === 'buy'
			? t.flowBuy
			: s.flow === 'extend'
				? t.flowExtend
				: s.flow === 'activate'
					? t.flowActivate
					: s.flow === 'faucet'
						? t.flowFaucet
						: t.flowSignature;

	return (
		<div className="fixed inset-0 z-30 flex flex-col justify-end">
			<div aria-hidden="true" className="absolute inset-0 animate-fade bg-black/60" />
			<section
				role="dialog"
				aria-modal="true"
				aria-label={title}
				className="relative mx-auto flex max-h-[88%] w-full max-w-md animate-rise flex-col gap-4 overflow-y-auto rounded-t-[28px] border-t border-line bg-card px-4 pb-6 pt-5"
			>
				<h2 className="px-1 text-xl font-semibold">{title}</h2>

				<ol className="flex flex-col gap-2">
					{s.steps.map((step, i) => (
						<li key={`${step.purpose}-${i}`}>
							<Row step={step} n={i + 1} />
						</li>
					))}
				</ol>

				<div aria-live="polite" className="flex min-h-[44px] items-center gap-2.5 px-1 text-[14px] leading-snug">
					{s.phase === 'failed' ? (
						s.problem ? (
							<ProblemText problem={s.problem} />
						) : (
							<span className="text-red-400">{t.flowDeclined}</span>
						)
					) : s.phase === 'done' ? (
						<span className="text-ok">{s.flow === 'buy'
								? t.flowBought
								: s.flow === 'extend'
									? t.flowExtended
									: s.flow === 'faucet'
										? t.flowFunded
										: t.flowActivated}</span>
					) : s.phase === 'settling' ? (
						<>
							<Spinner />
							<span className="text-muted">{t.flowSettling}</span>
						</>
					) : waiting ? (
						<span className="text-muted">{t.flowConfirmInWallet}</span>
					) : (
						<>
							<Spinner />
							<span className="text-muted">{t.flowPreparing}</span>
						</>
					)}
				</div>

				<button
					type="button"
					onClick={signing.dismiss}
					className={`h-12 w-full shrink-0 rounded-full text-[15px] font-medium ${
						finished ? 'bg-brand font-semibold text-bg hover:bg-brand-hover' : 'border border-white/[0.12] hover:bg-white/[0.04]'
					}`}
				>
					{finished ? t.close : t.flowHide}
				</button>
				{!finished && <p className="-mt-2 text-center text-xs text-muted">{t.flowHideHint}</p>}
			</section>
		</div>
	);
}

function Row({ step, n }: { step: Step; n: number }) {
	const { t } = useI18n();
	const now = step.state === 'waiting';
	return (
		<div
			className={`flex gap-3 rounded-[18px] border px-3.5 py-3 ${
				now ? 'border-brand/50 bg-brand/[0.08]' : 'border-line'
			} ${step.state === 'upcoming' ? 'opacity-50' : ''}`}
		>
			<Mark state={step.state} n={n} />
			<span className="flex min-w-0 flex-col gap-1">
				<span className="text-[15px] font-medium">{t.purpose[step.purpose]}</span>
				{now && <span className="text-[13px] leading-snug text-muted">{t.purposeHint[step.purpose]}</span>}
			</span>
		</div>
	);
}

function Mark({ state, n }: { state: Step['state']; n: number }) {
	const base = 'mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-xs font-semibold';
	if (state === 'signed')
		return (
			<span className={`${base} bg-ok/15 text-ok`}>
				<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
					<path d="m5 12 5 5 9-10" />
				</svg>
			</span>
		);
	if (state === 'failed')
		return (
			<span className={`${base} bg-red-400/15 text-red-400`}>
				<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" aria-hidden="true">
					<path d="M6 6l12 12M18 6 6 18" />
				</svg>
			</span>
		);
	if (state === 'waiting')
		return (
			<span className={`${base} text-brand`}>
				<Spinner />
			</span>
		);
	return <span className={`${base} bg-white/[0.08] text-muted`}>{n}</span>;
}

function Spinner() {
	return (
		<span
			aria-hidden="true"
			className="inline-block h-4 w-4 shrink-0 animate-spin rounded-full border-2 border-current border-r-transparent text-brand"
		/>
	);
}
