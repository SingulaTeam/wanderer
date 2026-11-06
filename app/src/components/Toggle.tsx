// An on/off switch over a real checkbox, so labels and keyboards work as usual.
export function Toggle({
	checked,
	disabled = false,
	onChange,
}: {
	checked: boolean;
	disabled?: boolean;
	onChange: (v: boolean) => void;
}) {
	return (
		<span className={`relative mt-0.5 inline-flex shrink-0 ${disabled ? 'opacity-50' : ''}`}>
			<input
				type="checkbox"
				checked={checked}
				disabled={disabled}
				onChange={(e) => onChange(e.target.checked)}
				className="peer absolute inset-0 h-full w-full cursor-pointer opacity-0 disabled:cursor-default"
			/>
			<span
				aria-hidden="true"
				className="h-7 w-12 rounded-full bg-white/15 transition-colors peer-checked:bg-brand peer-focus-visible:ring-2 peer-focus-visible:ring-brand/60"
			/>
			<span
				aria-hidden="true"
				className="absolute left-1 top-1 h-5 w-5 rounded-full bg-fg transition-transform peer-checked:translate-x-5"
			/>
		</span>
	);
}
