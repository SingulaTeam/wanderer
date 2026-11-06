// The interface's language: English or Russian, chosen by the person or, the
// first time, by the system. A new language is a file mirroring `en.ts` and a
// line in LOCALES.
import { invoke } from '@tauri-apps/api/core';
import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { en, type Strings } from './en';
import { ru } from './ru';

export const LOCALES = { en: { name: 'English', strings: en }, ru: { name: 'Русский', strings: ru } };
export type Locale = keyof typeof LOCALES;

const KEY = 'wanderer.locale';

function initial(): Locale {
	try {
		const saved = localStorage.getItem(KEY);
		if (saved && saved in LOCALES) return saved as Locale;
	} catch {
		// Storage may be unavailable; the system language then decides.
	}
	return navigator.language.toLowerCase().startsWith('ru') ? 'ru' : 'en';
}

type I18n = {
	t: Strings;
	locale: Locale;
	setLocale: (l: Locale) => void;
	/** A country's name in the current language, from its ISO code. */
	country: (code: string) => string;
};

const Context = createContext<I18n | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
	const [locale, setLocaleState] = useState<Locale>(initial);
	const value = useMemo<I18n>(() => {
		// The system knows every country's name in every language; nothing to ship.
		const names = new Intl.DisplayNames([locale], { type: 'region' });
		return {
			t: LOCALES[locale].strings,
			locale,
			setLocale: (l) => {
				setLocaleState(l);
				try {
					localStorage.setItem(KEY, l);
				} catch {
					// Kept for this run only.
				}
			},
			country: (code) => names.of(code) ?? code,
		};
	}, [locale]);
	// The tray menu lives outside the webview; tell it the language too.
	useEffect(() => {
		const { trayShow, trayQuit } = LOCALES[locale].strings;
		invoke('tray_labels', { show: trayShow, quit: trayQuit }).catch(() => {});
	}, [locale]);
	return <Context.Provider value={value}>{children}</Context.Provider>;
}

export function useI18n(): I18n {
	const value = useContext(Context);
	if (!value) throw new Error('useI18n outside I18nProvider');
	return value;
}
