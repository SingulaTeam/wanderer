// Signs for the Rust core with the wallet the person signed in with.
//
// The core never holds a key. When the protocol needs a signature it emits
// `wallet:sign`; this answers with `wallet_signed`. The core checks every
// signature it gets back against the address before using it.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Account } from 'thirdweb/wallets';
import { signing, type Purpose } from './signing';

type Request =
	| { id: number; purpose: Purpose; kind: 'message'; message: `0x${string}` }
	| { id: number; purpose: Purpose; kind: 'typed_data'; typed_data: TypedData };

type TypedData = {
	domain: Record<string, unknown>;
	types: Record<string, { name: string; type: string }[]>;
	primaryType: string;
	message: Record<string, unknown>;
};

export function serveSignatures(account: Account): Promise<UnlistenFn> {
	return listen<Request>('wallet:sign', async ({ payload }) => {
		signing.asked(payload.purpose);
		try {
			const signature =
				payload.kind === 'message'
					? await account.signMessage({ message: { raw: payload.message } })
					: await account.signTypedData(forViem(payload.typed_data) as never);
			signing.answered(payload.purpose, true);
			await invoke('wallet_signed', { id: payload.id, signature, error: null });
		} catch (e) {
			signing.answered(payload.purpose, false);
			await invoke('wallet_signed', { id: payload.id, signature: null, error: String(e) });
		}
	});
}

/**
 * The core's typed data, in the form thirdweb's signers (viem) hash correctly.
 *
 * viem derives the domain's type itself and leaves out `chainId` unless it
 * is a number — the core sends integers as decimal strings, as
 * `eth_signTypedData_v4` allows — so a wallet would sign over a domain
 * without the chain and the core would refuse the signature. Integers become
 * `bigint`s, by their declared types, and the domain's type is left to viem.
 */
function forViem(data: TypedData): TypedData {
	const { EIP712Domain: _, ...types } = data.types;
	const isInteger = (type: string) => /^u?int\d*$/.test(type);
	const message = Object.fromEntries(
		Object.entries(data.message).map(([name, value]) => {
			const field = types[data.primaryType]?.find((f) => f.name === name);
			return [name, field && isInteger(field.type) ? BigInt(value as string) : value];
		}),
	);
	const { chainId } = data.domain;
	const domain = chainId === undefined ? data.domain : { ...data.domain, chainId: BigInt(chainId as string) };
	return { ...data, types, domain, message };
}
