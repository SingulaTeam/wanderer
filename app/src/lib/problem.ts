// What went wrong, as the core explains it; see `wanderer_core::problem`.
import { useI18n } from '../i18n';
import type { Gap } from './core';

export type ProblemKind =
	| 'declined'
	| 'wallet_failed'
	| 'offline'
	| 'no_subscription'
	| 'all_sessions_in_use'
	| 'session_releasing'
	| 'gas_budget'
	| 'relay_busy'
	| 'insufficient_funds'
	| 'unactivated_limit'
	| 'price_changed'
	| 'paused'
	| 'node'
	| 'lost'
	| 'route'
	| 'service'
	| 'storage'
	| 'other';

export type Problem = {
	kind: ProblemKind;
	/** When trying again may work, in unix seconds. */
	at: number | null;
	/** The error as it came, in English. */
	detail: string;
	/** For `route`: the hop that could not be filled. */
	route?: Gap;
};

/** A rejected command's error: a `Problem`, or a bare message from older commands. */
export function problemOf(e: unknown): Problem {
	if (e && typeof e === 'object' && 'kind' in e && 'detail' in e) return e as Problem;
	return { kind: 'other', at: null, detail: String(e) };
}

/** Words `problem` in the person's language. */
export function useExplain() {
	const { t, locale, country } = useI18n();
	return (p: Problem) => {
		if (p.kind === 'route' && p.route)
			return t.routeGap(p.route.hop, p.route.country && country(p.route.country), p.route.only_same_operator);
		const when = p.at === null ? null : moment(p.at, locale);
		return t.problem[p.kind](when);
	};
}

/** A time today, or a date and time further off. */
function moment(unix: number, locale: string): string {
	const at = new Date(unix * 1000);
	const today = at.toDateString() === new Date().toDateString();
	return at.toLocaleString(
		locale,
		today ? { hour: '2-digit', minute: '2-digit' } : { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' },
	);
}
