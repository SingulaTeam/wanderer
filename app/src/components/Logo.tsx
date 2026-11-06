/** The mark and the wordmark, as on the landing. `large` for the sign-in. */
export function Logo({ large }: { large?: boolean }) {
	return large ? (
		<div className="flex flex-col items-center gap-4">
			<img src="/wanderer-logo.png" alt="" width={900} height={500} className="h-16 w-auto" />
			<span className="font-display text-lg font-bold tracking-[0.22em]">WANDERER</span>
		</div>
	) : (
		<div className="flex items-center gap-2.5">
			<img src="/wanderer-logo.png" alt="" width={900} height={500} className="h-6 w-auto" />
			<span className="font-display text-sm font-bold tracking-[0.2em]">WANDERER</span>
		</div>
	);
}
