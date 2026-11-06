// The signed-in wallet's subscriptions, kept fresh.
import { useCallback, useEffect, useState } from 'react';
import { core, type Account, type Payment } from './core';
import { problemOf, type Problem } from './problem';
import { signing, type Flow } from './signing';

const REFRESH_EVERY_MS = 30_000;
const SETTLE_EVERY_MS = 2_000;
const SETTLE_ATTEMPTS = 15;

export function useAccount(ready: boolean) {
	const [account, setAccount] = useState<Account | null>(null);
	const [working, setWorking] = useState<Flow | null>(null);
	const [error, setError] = useState<Problem | null>(null);

	const refresh = useCallback(() => {
		core.account().then(setAccount).catch((e) => setError(problemOf(e)));
	}, []);

	useEffect(() => {
		if (!ready) return;
		refresh();
		const timer = setInterval(refresh, REFRESH_EVERY_MS);
		return () => clearInterval(timer);
	}, [ready, refresh]);

	// The relay answers once the transaction is sent; the RPC node may show
	// it a few blocks later. Read until the change is there. Says whether it
	// went through.
	const run = useCallback(
		async (what: Flow, f: () => Promise<unknown>, done: (a: Account) => boolean): Promise<boolean> => {
			setWorking(what);
			setError(null);
			signing.start(what);
			try {
				await f();
				signing.settling();
				for (let i = 0; i < SETTLE_ATTEMPTS; i++) {
					const next = await core.account();
					setAccount(next);
					if (done(next)) break;
					await new Promise((r) => setTimeout(r, SETTLE_EVERY_MS));
				}
				signing.done();
				return true;
			} catch (e) {
				const problem = problemOf(e);
				setError(problem);
				signing.failed(problem);
				return false;
			} finally {
				setWorking(null);
			}
		},
		[],
	);

	// The one that runs out last: subscriptions stack, so that is when the
	// wallet stops being covered.
	const active =
		account?.subscriptions
			.filter((s) => s.active)
			.sort((a, b) => (b.expires_at ?? 0) - (a.expires_at ?? 0))[0] ?? null;
	const waiting = account?.subscriptions.find((s) => !s.active && s.expires_at === null) ?? null;
	// The one running now: its connections are what a route can spend.
	const now = Date.now() / 1000;
	const current =
		account?.subscriptions.find(
			(s) => s.active && s.starts_at !== null && s.starts_at <= now && (s.expires_at ?? 0) > now,
		) ?? null;

	const buy = (payment: Payment, slots: number) => {
		const before = account?.subscriptions.length ?? 0;
		return run('buy', () => core.buy(payment, slots), (a) => a.subscriptions.length > before);
	};
	const addSlots = (token: number, extra: number) => {
		const before = account?.subscriptions.find((s) => s.token === token)?.slots ?? 0;
		return run(
			'extend',
			() => core.addSlots(token, extra),
			(a) => (a.subscriptions.find((s) => s.token === token)?.slots ?? 0) > before,
		);
	};
	const activate = (token: number) =>
		run('activate', () => core.activate(token), (a) => a.subscriptions.some((s) => s.token === token && s.active));

	const faucet = () => {
		const before = account?.faucet;
		return run('faucet', () => core.faucet(), (a) => a.faucet !== before);
	};

	return { account, active, current, waiting, working, error, buy, addSlots, activate, faucet };
}
