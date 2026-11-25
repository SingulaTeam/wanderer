// Transport, DNS, the local network, the language, and the account.
import { useState } from 'react';
import { LOCALES, useI18n, type Locale } from '../i18n';
import type { Settings } from '../lib/core';
import { RESOLVERS, checkResolver } from '../lib/dns';
import { Sheet } from './Sheet';
import { Toggle } from './Toggle';

export function SettingsSheet({
	settings,
	address,
	onChange,
	onSignOut,
	onOpen,
	onClose,
}: {
	settings: Settings;
	address: string;
	onChange: (s: Settings) => void;
	onSignOut: () => void;
	onOpen: (sheet: 'subscriptions' | 'diagnostics') => void;
	onClose: () => void;
}) {
	const { t, locale, setLocale } = useI18n();
	const [copied, setCopied] = useState(false);

	const copy = () => {
		navigator.clipboard
			.writeText(address)
			.then(() => {
				setCopied(true);
				setTimeout(() => setCopied(false), 1500);
			})
			.catch(console.error);
	};

	return (
		<Sheet title={t.settings} onClose={onClose}>
			<div className="flex flex-col gap-6 pt-1">
				<Field label={t.mode} hint={t.modeHint}>
					<Segmented
						label={t.mode}
						value={settings.mode}
						options={[
							['simple', t.modeSimple],
							['advanced', t.modeAdvanced],
						]}
						onChange={(mode) => onChange({ ...settings, mode })}
					/>
				</Field>

				{settings.mode === 'advanced' && (
					<>
						<Field label={t.transport} hint={t.transportHint}>
							<Segmented
								label={t.transport}
								value={settings.transport}
								options={[
									['auto', t.transportAuto],
									['vision', 'Vision'],
									['xhttp', 'xHTTP'],
								]}
								onChange={(transport) => onChange({ ...settings, transport })}
							/>
						</Field>

						<Field label={t.cover} hint={t.coverHint}>
							<Segmented
								label={t.cover}
								value={settings.cover ?? 'auto'}
								options={[
									['auto', t.coverAuto],
									['default', 'Global'],
									['RU', 'RU'],
									['CN', 'CN'],
									['IR', 'IR'],
								]}
								onChange={(v) => onChange({ ...settings, cover: v === 'auto' ? null : v })}
							/>
						</Field>

						<DnsField value={settings.dns} onChange={(dns) => onChange({ ...settings, dns })} />
					</>
				)}

				<label className="flex items-start justify-between gap-4">
					<span className="flex flex-col gap-1">
						<span className="text-[15px] font-medium">{t.localNetwork}</span>
						<span className="text-[13px] leading-snug text-muted">{t.localNetworkHint}</span>
					</span>
					<Toggle checked={settings.allow_lan} onChange={(allow_lan) => onChange({ ...settings, allow_lan })} />
				</label>

				<Field label={t.language}>
					<Segmented
						label={t.language}
						value={locale}
						options={(Object.keys(LOCALES) as Locale[]).map((l) => [l, LOCALES[l].name])}
						onChange={setLocale}
					/>
				</Field>

				<div className="flex flex-col gap-2">
					<NavRow label={t.subscriptions} onClick={() => onOpen('subscriptions')} />
					<NavRow label={t.diagnostics} onClick={() => onOpen('diagnostics')} />
				</div>

				<Field label={t.account}>
					<div className="flex items-center gap-2 rounded-2xl border border-line bg-bg p-1.5 pl-3.5">
						<span className="min-w-0 flex-1 truncate font-mono text-[13px] text-muted">{address}</span>
						<button
							type="button"
							onClick={copy}
							className="h-10 shrink-0 rounded-xl bg-white/[0.06] px-3 text-sm hover:bg-white/10"
						>
							{copied ? t.copied : t.copyAddress}
						</button>
					</div>
					<button
						type="button"
						onClick={onSignOut}
						className="mt-3 h-12 w-full rounded-full border border-white/[0.12] text-[15px] font-medium hover:bg-white/[0.04]"
					>
						{t.signOut}
					</button>
				</Field>
			</div>
		</Sheet>
	);
}

/** The resolver: three by name, or one typed, saved once the core would take it. */
function DnsField({ value, onChange }: { value: string | null; onChange: (dns: string | null) => void }) {
	const { t } = useI18n();
	const named = RESOLVERS.find((r) => r.value === value);
	const [custom, setCustom] = useState(!named);
	const [typed, setTyped] = useState(named ? '' : (value ?? ''));
	const check = checkResolver(typed);

	const type = (s: string) => {
		setTyped(s);
		if (checkResolver(s) !== 'invalid') onChange(s.trim());
	};

	const option = (selected: boolean, name: string, onClick: () => void) => (
		<button
			key={name}
			type="button"
			role="radio"
			aria-checked={selected}
			onClick={onClick}
			className={`h-10 min-w-0 truncate rounded-xl px-2 text-sm transition-colors ${
				selected ? 'bg-raised font-medium text-fg' : 'text-muted hover:text-fg'
			}`}
		>
			{name}
		</button>
	);

	return (
		<Field label={t.dns} hint={t.dnsHint}>
			{/* Two columns: four names in one row do not fit a narrow phone. */}
			<div role="radiogroup" aria-label={t.dns} className="grid grid-cols-2 gap-1 rounded-2xl bg-bg p-1">
				{RESOLVERS.map((r) =>
					option(!custom && named?.id === r.id, r.name, () => {
						setCustom(false);
						onChange(r.value);
					}),
				)}
				{option(custom, t.dnsCustom, () => setCustom(true))}
			</div>
			{custom && (
				<>
					<input
						type="text"
						inputMode="url"
						autoCapitalize="off"
						autoCorrect="off"
						spellCheck={false}
						value={typed}
						onChange={(e) => type(e.target.value)}
						placeholder="https://9.9.9.9/dns-query"
						aria-label={t.dnsCustom}
						aria-invalid={typed.trim() !== '' && check === 'invalid'}
						className="h-12 w-full min-w-0 rounded-2xl border border-line bg-bg px-3.5 font-mono text-[15px] text-fg outline-none placeholder:text-muted focus:border-white/30"
					/>
					{typed.trim() !== '' && check === 'invalid' && (
						<span className="text-[13px] leading-snug text-red-400">{t.dnsInvalid}</span>
					)}
					{check === 'plain' && <span className="text-[13px] leading-snug text-hold">{t.dnsPlain}</span>}
				</>
			)}
		</Field>
	);
}

function NavRow({ label, onClick }: { label: string; onClick: () => void }) {
	return (
		<button
			type="button"
			onClick={onClick}
			className="flex h-12 w-full items-center justify-between rounded-2xl bg-bg px-4 text-left text-[15px] hover:bg-white/[0.04]"
		>
			{label}
			<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#a1a1aa" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
				<path d="m9 6 6 6-6 6" />
			</svg>
		</button>
	);
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
	return (
		<div className="flex flex-col gap-2.5">
			<span className="text-[15px] font-medium">{label}</span>
			{children}
			{hint && <span className="text-[13px] leading-snug text-muted">{hint}</span>}
		</div>
	);
}

function Segmented<T extends string>({
	label,
	value,
	options,
	onChange,
}: {
	label: string;
	value: T;
	options: [T, string][];
	onChange: (v: T) => void;
}) {
	return (
		<div role="radiogroup" aria-label={label} className="flex gap-1 rounded-2xl bg-bg p-1">
			{options.map(([v, name]) => (
				<button
					key={v}
					type="button"
					role="radio"
					aria-checked={value === v}
					onClick={() => onChange(v)}
					className={`h-10 flex-1 rounded-xl text-sm transition-colors ${
						value === v ? 'bg-raised font-medium text-fg' : 'text-muted hover:text-fg'
					}`}
				>
					{name}
				</button>
			))}
		</div>
	);
}
