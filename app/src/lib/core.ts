// The Rust core, as the UI sees it: commands to call, and events to follow.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Problem } from './problem';

export type Transport = 'vision' | 'xhttp';

/** The supervisor's state; see `wanderer_core::supervisor::State`. */
export type State =
	| { state: 'disconnected' }
	| { state: 'connecting' }
	| { state: 'connected'; nodes: number[]; transport: Transport; cover: string }
	| { state: 'holding'; problem: Problem }
	| { state: 'failed'; problem: Problem };

export type Settings = {
	transport: 'auto' | Transport;
	allow_lan: boolean;
	/** A country for each hop, entry first, as ISO 3166-1 alpha-2; null for any. */
	route: (string | null)[];
	/** Whether the hops may share an operator. */
	same_operator: boolean;
	/** A cover preset, such as `RU`; null for the node's choice. */
	cover: string | null;
	/** Two plans and an entry and exit, or every hop and setting. */
	mode: Mode;
};

export type Mode = 'simple' | 'advanced';

/** The most hops a route may have; the core holds the same limit. */
export const MAX_HOPS = 3;

/** Where the network has nodes. */
export type Locations = {
	countries: { code: string; nodes: number; load: number }[];
	/** Each node's country, by Star id. */
	nodes: Record<string, string>;
	/** Available nodes, in any country or none. */
	node_count: number;
	/** Operators running Control Planes that list nodes. */
	control_plane_operators: number;
};

/** The hop a route cannot fill, as the core reports it. */
export type Gap = { hop: number; country: string | null; only_same_operator: boolean };

export type Traffic = { up: number; down: number };

export type Subscription = {
	token: number;
	active: boolean;
	/** Later than now while it waits behind another one. */
	starts_at: number | null;
	expires_at: number | null;
	/** Concurrent sessions it pays for: devices, or hops of one route. */
	slots: number;
};

/** The most connections one subscription carries; each costs one price. */
export const MAX_SLOTS = 5;

/** The wallet's subscriptions and the ways to pay for another. */
export type Payment = 'usdc' | 'echo';
export type Offer = {
	payment: Payment;
	/** The price of one connection, in whole tokens. */
	price: string;
	balance: string;
	can_afford: boolean;
	/** How many connections the balance pays for, gas fee included. */
	affordable_slots: number;
	/** The gas fee of 1, 2, … MAX_SLOTS connections, in whole USDC. */
	gas_fees: string[];
	/** What 1, 2, … connections cost in all, gas fee included. */
	totals: string[];
};
/** What adding connections costs, in whole USDC. */
export type Quote = {
	/** The price and the gas fee together. */
	total: string;
	gas_fee: string;
};
export type Account = {
	subscriptions: Subscription[];
	offers: Offer[];
	/** Test USDC the faucet would add, in whole tokens; null off a test network or at the cap. */
	faucet: string | null;
};

/** Whether the core is ready to connect, and why not if it is not. */
export type Readiness =
	| { ready: 'starting' }
	| { ready: 'yes' }
	| { ready: 'no'; reason: string };

export const core = {
	readiness: () => invoke<Readiness>('readiness'),
	retry: () => invoke<void>('retry'),
	state: () => invoke<State>('state'),
	settings: () => invoke<Settings>('settings'),
	saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
	connect: () => invoke<void>('connect'),
	disconnect: () => invoke<void>('disconnect'),
	signedIn: (address: string) => invoke<void>('signed_in', { address }),
	account: () => invoke<Account>('account'),
	buy: (payment: Payment, slots: number) => invoke<number>('buy', { payment, slots }),
	addSlotsQuote: (token: number, extra: number) => invoke<Quote>('add_slots_quote', { token, extra }),
	addSlots: (token: number, extra: number) => invoke<void>('add_slots', { token, extra }),
	diagnostics: () => invoke<string>('diagnostics'),
	activate: (token: number) => invoke<void>('activate', { token }),
	faucet: () => invoke<void>('faucet'),
	locations: () => invoke<Locations>('locations'),
	signedOut: () => invoke<void>('signed_out'),
};

export function onState(f: (s: State) => void): Promise<UnlistenFn> {
	return listen<State>('core:state', (e) => f(e.payload));
}

export function onTraffic(f: (t: Traffic) => void): Promise<UnlistenFn> {
	return listen<Traffic>('core:traffic', (e) => f(e.payload));
}

export function onReadiness(f: (r: Readiness) => void): Promise<UnlistenFn> {
	return listen<Readiness>('core:readiness', (e) => f(e.payload));
}
