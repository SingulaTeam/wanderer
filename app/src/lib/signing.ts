// Where a flow that needs the wallet stands: which signatures it has had,
// which one the wallet is showing now, and what happens after.
//
// The signer reports each request as it goes to the wallet and comes back;
// the account reports the flow around them. A signature asked for outside a
// flow — the identity and sign-in on the first connect — shows on its own.
import { useSyncExternalStore } from 'react';
import type { Problem } from './problem';

/** What a signature is for, as the core names it. */
export type Purpose =
	| 'identity'
	| 'sign_in'
	| 'purchase'
	| 'extend'
	| 'permit'
	| 'activation'
	| 'register'
	| 'relay'
	| 'other';

export type Flow = 'buy' | 'extend' | 'activate' | 'faucet';

export type Step = { purpose: Purpose; state: 'upcoming' | 'waiting' | 'signed' | 'failed' };

export type Signing = {
	flow: Flow | null;
	phase: 'working' | 'settling' | 'done' | 'failed';
	steps: Step[];
	problem: Problem | null;
};

// The signatures each flow asks for, in the core's order. Signing in and the
// identity come first when they are not cached, and are added as they come.
const PLANNED: Record<Flow, Purpose[]> = {
	buy: ['purchase', 'permit', 'relay'],
	extend: ['extend', 'permit', 'relay'],
	activate: ['activation', 'relay'],
	faucet: ['relay'],
};

let state: Signing | null = null;
const listeners = new Set<() => void>();

function set(next: Signing | null) {
	state = next;
	listeners.forEach((l) => l());
}

export const signing = {
	start(flow: Flow) {
		set({ flow, phase: 'working', steps: PLANNED[flow].map((purpose) => ({ purpose, state: 'upcoming' })), problem: null });
	},

	/** The wallet has been asked for a `purpose` signature. */
	asked(purpose: Purpose) {
		const s: Signing = state ?? { flow: null, phase: 'working', steps: [], problem: null };
		const at = s.steps.findIndex((x) => x.purpose === purpose && x.state === 'upcoming');
		const steps = [...s.steps];
		if (at >= 0) steps[at] = { purpose, state: 'waiting' };
		else {
			// Not planned: goes before the first step still to come.
			const next = steps.findIndex((x) => x.state === 'upcoming');
			steps.splice(next < 0 ? steps.length : next, 0, { purpose, state: 'waiting' });
		}
		set({ ...s, steps });
	},

	answered(purpose: Purpose, signed: boolean) {
		if (!state) return;
		const steps = state.steps.map((x) =>
			x.purpose === purpose && x.state === 'waiting' ? { purpose, state: signed ? 'signed' : 'failed' } as Step : x,
		);
		// Outside a flow there is nothing after the signature to show, and a
		// refusal ends it: the core reports what it could not do on its own.
		if (state.flow === null) set(signed ? null : { ...state, steps, phase: 'failed' });
		else set({ ...state, steps });
	},

	settling() {
		if (state) set({ ...state, phase: 'settling' });
	},

	done() {
		if (state) set({ ...state, phase: 'done' });
	},

	failed(problem: Problem) {
		set({ ...(state ?? { flow: null, steps: [] }), phase: 'failed', problem });
	},

	dismiss() {
		set(null);
	},
};

export function useSigning(): Signing | null {
	return useSyncExternalStore(
		(l) => {
			listeners.add(l);
			return () => listeners.delete(l);
		},
		() => state,
	);
}
