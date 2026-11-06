// A country's flag, round. Bundled, not emoji: Windows has no emoji flags,
// and on Linux they depend on the fonts installed.
const FLAGS = import.meta.glob<string>('/node_modules/flag-icons/flags/1x1/*.svg', {
	query: '?url',
	import: 'default',
	eager: true,
});

const url = (code: string) => FLAGS[`/node_modules/flag-icons/flags/1x1/${code.toLowerCase()}.svg`];

/** The flag of `code` (ISO 3166-1 alpha-2), or the code itself if there is none. */
export function Flag({ code, size }: { code: string; size: number }) {
	const src = url(code);
	const box = { width: size, height: size };
	return src ? (
		<img src={src} alt="" style={box} className="shrink-0 rounded-full object-cover ring-1 ring-white/10" />
	) : (
		<span
			style={box}
			className="flex shrink-0 items-center justify-center rounded-full bg-raised font-display text-xs font-bold tracking-[0.08em]"
		>
			{code}
		</span>
	);
}
