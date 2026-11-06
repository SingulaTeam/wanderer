import React from 'react';
import ReactDOM from 'react-dom/client';
import { ThirdwebProvider } from 'thirdweb/react';
import App from './App';
import { I18nProvider } from './i18n';
import { relayOauth } from './lib/oauth';
import './index.css';

relayOauth();

ReactDOM.createRoot(document.getElementById('root')!).render(
	<React.StrictMode>
		<ThirdwebProvider>
			<I18nProvider>
				<App />
			</I18nProvider>
		</ThirdwebProvider>
	</React.StrictMode>,
);
