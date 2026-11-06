// Google and Apple sign-in.
//
// thirdweb opens the provider in a popup and waits for it to post the result
// back to `window.opener`. The webview cannot open popups, so the app opens a
// window of its own (see `oauth.rs`); its page posts the result as a Tauri
// event, and this hands it to thirdweb as the message it waits for.
//
// On a phone the page opens in the system browser instead and comes back as
// a `singula://oauth?authResult=…` link, which becomes the same message.
import { listen } from '@tauri-apps/api/event';
import { onOpenUrl } from '@tauri-apps/plugin-deep-link';

const THIRDWEB = 'https://embedded-wallet.thirdweb.com';

declare global {
	interface Window {
		__wandererOauthPopupClosed?: boolean;
	}
}

export function relayOauth() {
	listen<string>('oauth_postmessage', (e) => {
		try {
			const data = typeof e.payload === 'string' ? JSON.parse(e.payload) : e.payload;
			window.dispatchEvent(
				new MessageEvent('message', { data, origin: THIRDWEB }),
			);
		} catch (err) {
			console.error('cannot relay the sign-in result', err);
		}
	}).catch(console.error);
	listen('oauth_popup_closed', () => {
		window.__wandererOauthPopupClosed = true;
	}).catch(console.error);
	// Only registered on a phone; a desktop rejects, and that is expected.
	onOpenUrl((urls) => urls.forEach(relayLink)).catch(() => {});
}

/** Hands a `singula://oauth` link's result to thirdweb. */
function relayLink(url: string) {
	let data: unknown;
	try {
		const params = new URL(url).searchParams;
		const result = params.get('authResult');
		const error = params.get('error');
		if (result) {
			data = { eventType: 'oauthSuccessResult', authResult: JSON.parse(result) };
		} else if (error) {
			// thirdweb reads `errorString`, not `error`.
			data = { eventType: 'oauthFailureResult', errorString: error };
		} else {
			return;
		}
	} catch (err) {
		console.error('unreadable sign-in link', err);
		return;
	}
	window.dispatchEvent(new MessageEvent('message', { data, origin: THIRDWEB }));
	// Only after a result: set on any other link, thirdweb would give up on a
	// sign-in still running.
	window.__wandererOauthPopupClosed = true;
}
