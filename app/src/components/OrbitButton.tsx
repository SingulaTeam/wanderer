// The one control that matters: a round button inside two orbits. Its look
// says where the connection is; pressing it connects, or disconnects.
import { useI18n } from '../i18n';

export type Orbit = 'idle' | 'connecting' | 'on' | 'holding';

export function OrbitButton({
	orbit,
	disabled,
	onPress,
}: {
	orbit: Orbit;
	disabled?: boolean;
	onPress: () => void;
}) {
	const { t } = useI18n();
	const on = orbit === 'on';
	const holding = orbit === 'holding';
	const moving = orbit === 'connecting' || holding;

	const outer = on
		? 'border border-brand/35'
		: holding
			? 'border-[1.5px] border-dashed border-hold/45'
			: 'border border-dashed border-white/[0.12]';
	const inner = on ? 'border border-brand/55 shadow-[0_0_40px_rgba(254,80,1,0.25)]' : 'border border-line';
	const satellite = on
		? 'h-2.5 w-2.5 bg-brand shadow-[0_0_14px_rgba(254,80,1,0.9)]'
		: holding
			? 'h-2.5 w-2.5 bg-hold shadow-[0_0_14px_rgba(251,191,36,0.8)]'
			: orbit === 'connecting'
				? 'h-2 w-2 bg-brand-soft'
				: 'h-2 w-2 bg-white/30';
	const button = on
		? 'border-0 bg-brand text-bg shadow-[0_20px_60px_-12px_rgba(254,80,1,0.75)] hover:bg-brand-hover'
		: holding
			? 'border border-hold/45 bg-card text-hold-soft'
			: 'border border-white/[0.12] bg-card text-fg shadow-[0_24px_60px_-20px_rgba(0,0,0,0.8)] hover:border-white/25';

	const label = on ? t.on : holding ? t.holding : orbit === 'connecting' ? '' : t.connect;
	const name = on || holding || orbit === 'connecting' ? t.disconnectAndLift : t.connect;

	return (
		<div className="relative flex h-[248px] w-[248px] items-center justify-center">
			{on && (
				<div aria-hidden="true" className="absolute inset-[-30px] rounded-full bg-brand/20 blur-[60px]" />
			)}
			<div aria-hidden="true" className={`absolute inset-0 rounded-full ${outer}`} />
			<div aria-hidden="true" className={`absolute inset-6 rounded-full ${inner}`} />
			{/* The satellite: slow when on, quick while connecting or holding. */}
			<div
				aria-hidden="true"
				className={`absolute inset-0 ${moving ? 'animate-orbit-fast' : on ? 'animate-orbit' : ''}`}
			>
				<span className={`absolute left-1/2 top-[-4px] -translate-x-1/2 rounded-full ${satellite}`} />
			</div>
			<button
				type="button"
				aria-label={name}
				disabled={disabled}
				onClick={onPress}
				className={`relative flex h-[156px] w-[156px] flex-col items-center justify-center gap-3 rounded-full transition-colors disabled:opacity-40 ${button}`}
			>
				<Icon orbit={orbit} />
				{label && <span className="font-display text-xs font-bold tracking-[0.22em]">{label.toUpperCase()}</span>}
			</button>
		</div>
	);
}

function Icon({ orbit }: { orbit: Orbit }) {
	const common = {
		width: 40,
		height: 40,
		viewBox: '0 0 24 24',
		fill: 'none',
		stroke: 'currentColor',
		strokeLinecap: 'round' as const,
		strokeLinejoin: 'round' as const,
		'aria-hidden': true,
	};
	switch (orbit) {
		case 'on':
			return (
				<svg {...common} strokeWidth="1.8">
					<path d="M12 3 4.5 6v5.5c0 4.6 3.2 8.2 7.5 9.5 4.3-1.3 7.5-4.9 7.5-9.5V6L12 3z" />
					<path d="m8.8 12.2 2.3 2.3 4.3-4.6" />
				</svg>
			);
		case 'holding':
			return (
				<svg {...common} strokeWidth="1.6">
					<path d="M20 12a8 8 0 1 1-2.3-5.7" />
					<path d="M20 4v5h-5" />
				</svg>
			);
		case 'connecting':
			return (
				<svg {...common} strokeWidth="1.6" className="animate-breathe">
					<path d="M12 3v8" />
					<path d="M6.3 6.8a8 8 0 1 0 11.4 0" />
				</svg>
			);
		default:
			return (
				<svg {...common} strokeWidth="1.6">
					<path d="M12 3v8" />
					<path d="M6.3 6.8a8 8 0 1 0 11.4 0" />
				</svg>
			);
	}
}
