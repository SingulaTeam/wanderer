// Minimise and close, for the window that draws its own title bar. Two, not
// three: the window is a fixed size, so there is nothing to maximise. Closing
// hides it into the tray; the protection keeps running.
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useI18n } from '../i18n';

/** Phones have no window to control. */
export const hasWindow = !/android|iphone|ipad/i.test(navigator.userAgent);

export function WindowControls() {
	const { t } = useI18n();
	if (!hasWindow) return null;
	const win = getCurrentWindow();
	return (
		<div className="flex items-center gap-1">
			<Control label={t.minimize} onClick={() => void win.minimize()} d="M5 12h14" />
			<Control label={t.close} onClick={() => void win.close()} d="M6 6l12 12M18 6 6 18" danger />
		</div>
	);
}

function Control({ label, onClick, d, danger }: { label: string; onClick: () => void; d: string; danger?: boolean }) {
	return (
		<button
			type="button"
			aria-label={label}
			title={label}
			onClick={onClick}
			className={`flex h-9 w-9 items-center justify-center rounded-full text-muted transition-colors hover:text-fg ${
				danger ? 'hover:bg-red-500/80' : 'hover:bg-white/10'
			}`}
		>
			<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
				<path d={d} />
			</svg>
		</button>
	);
}
