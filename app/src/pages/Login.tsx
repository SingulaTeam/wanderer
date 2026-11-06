// Signing in: the brand, one line on what this is, and thirdweb's sign-in.
import { ConnectEmbed, darkTheme } from 'thirdweb/react';
import { Logo } from '../components/Logo';
import { WindowControls } from '../components/WindowControls';
import { useI18n } from '../i18n';
import { chain, client, wallets } from '../lib/thirdweb';

// thirdweb's sign-in in Wanderer's colours.
const theme = darkTheme({
	colors: {
		modalBg: '#141417',
		borderColor: 'rgba(255,255,255,0.08)',
		separatorLine: 'rgba(255,255,255,0.08)',
		tertiaryBg: '#1f1f24',
		accentText: '#fe5001',
		accentButtonBg: '#fe5001',
		accentButtonText: '#0b0b0d',
		primaryButtonBg: '#fe5001',
		primaryButtonText: '#0b0b0d',
		secondaryText: '#a1a1aa',
		primaryText: '#f4f4f5',
	},
});

export default function Login() {
	const { t, locale } = useI18n();
	return (
		<div className="relative mx-auto flex h-full max-w-md flex-col items-center justify-center gap-7 overflow-y-auto px-5 py-8">
			<div data-tauri-drag-region className="absolute inset-x-0 top-0 z-10 flex h-14 items-center justify-end px-4">
				<WindowControls />
			</div>
			<div aria-hidden="true" className="pointer-events-none absolute -right-36 -top-44 h-[420px] w-[420px] rounded-full bg-brand/15 blur-[100px]" />
			<div className="relative flex flex-col items-center gap-3 text-center">
				<Logo large />
				<p className="max-w-[300px] text-sm leading-snug text-muted">{t.tagline}</p>
			</div>
			<div className="relative w-full">
				<ConnectEmbed
					client={client}
					wallets={wallets}
					chain={chain}
					theme={theme}
					locale={locale === 'ru' ? 'ru_RU' : 'en_US'}
					showThirdwebBranding={false}
					style={{ width: '100%', maxWidth: 380, margin: '0 auto', borderRadius: 24 }}
				/>
			</div>
			<p className="relative max-w-[320px] text-center text-xs leading-snug text-muted">{t.signInHint}</p>
		</div>
	);
}
