// A panel that rises from the bottom over a dimmed screen: on a phone it is
// within thumb's reach, on a desktop window it reads as a dialog.
import { useEffect, useRef, type ReactNode } from 'react';
import { useI18n } from '../i18n';

export function Sheet({
	title,
	onClose,
	children,
}: {
	title: string;
	onClose: () => void;
	children: ReactNode;
}) {
	const { t } = useI18n();
	const panel = useRef<HTMLElement>(null);

	useEffect(() => {
		panel.current?.focus();
		const onKey = (e: KeyboardEvent) => {
			if (e.key === 'Escape') onClose();
		};
		window.addEventListener('keydown', onKey);
		return () => window.removeEventListener('keydown', onKey);
	}, [onClose]);

	return (
		<div className="fixed inset-0 z-20 flex flex-col justify-end">
			<button
				type="button"
				aria-label={t.close}
				tabIndex={-1}
				onClick={onClose}
				className="absolute inset-0 animate-fade cursor-default bg-black/55"
			/>
			<section
				ref={panel}
				tabIndex={-1}
				role="dialog"
				aria-modal="true"
				aria-label={title}
				className="relative mx-auto flex max-h-[88%] w-full max-w-md animate-rise flex-col rounded-t-[28px] border-t border-line bg-card px-4 pt-2.5 outline-none"
			>
				<div aria-hidden="true" className="h-1 w-10 self-center rounded-full bg-white/[0.18]" />
				<div className="flex items-center justify-between px-1 pb-3 pt-3.5">
					<h2 className="text-xl font-semibold">{title}</h2>
					<button
						type="button"
						aria-label={t.close}
						onClick={onClose}
						className="flex h-11 w-11 items-center justify-center rounded-full bg-white/[0.06] hover:bg-white/10"
					>
						<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
							<path d="M6 6l12 12M18 6 6 18" />
						</svg>
					</button>
				</div>
				<div className="-mx-4 min-h-0 overflow-y-auto px-4 pb-6">{children}</div>
			</section>
		</div>
	);
}
