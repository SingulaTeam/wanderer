import { createThirdwebClient } from 'thirdweb';
import { arbitrumSepolia } from 'thirdweb/chains';
import { inAppWallet, createWallet } from 'thirdweb/wallets';

const clientId = import.meta.env.VITE_THIRDWEB_CLIENT_ID as string | undefined;
if (!clientId) {
	// Not thrown: a throw here blanks the whole window before React mounts.
	console.error('VITE_THIRDWEB_CLIENT_ID is not set; sign-in will not work.');
}

export const client = createThirdwebClient({ clientId: clientId ?? 'missing' });

/** The chain the protocol runs on. */
export const chain = arbitrumSepolia;

export const wallets = [
	inAppWallet({ auth: { options: ['email', 'google', 'apple'] } }),
	createWallet('walletConnect'),
];
