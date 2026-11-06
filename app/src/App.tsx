import { useEffect } from 'react';
import { useActiveAccount } from 'thirdweb/react';
import { core } from './lib/core';
import { serveSignatures } from './lib/signer';
import Home from './pages/Home';
import Login from './pages/Login';

export default function App() {
	const account = useActiveAccount();

	// The core follows the signed-in account, and signs through it.
	useEffect(() => {
		if (!account) {
			core.signedOut().catch(console.error);
			return;
		}
		core.signedIn(account.address).catch(console.error);
		const stop = serveSignatures(account);
		return () => {
			stop.then((f) => f());
		};
	}, [account]);

	return account ? <Home address={account.address} /> : <Login />;
}
