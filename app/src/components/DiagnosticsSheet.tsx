// The app's recent log, to copy into a problem report.
import { useEffect, useState } from 'react';
import { useI18n } from '../i18n';
import { core } from '../lib/core';
import { Sheet } from './Sheet';

export function DiagnosticsSheet({ onClose }: { onClose: () => void }) {
	const { t } = useI18n();
	const [report, setReport] = useState('');
	const [copied, setCopied] = useState(false);

	useEffect(() => {
		core.diagnostics().then(setReport).catch((e) => setReport(String(e)));
	}, []);

	const copy = () => {
		navigator.clipboard
			.writeText(report)
			.then(() => {
				setCopied(true);
				setTimeout(() => setCopied(false), 1500);
			})
			.catch(console.error);
	};

	return (
		<Sheet title={t.diagnostics} onClose={onClose}>
			<div className="flex flex-col gap-3 pt-1">
				<p className="text-[13px] leading-snug text-muted">{t.diagnosticsHint}</p>
				<pre className="max-h-[320px] overflow-auto whitespace-pre-wrap break-all rounded-2xl bg-bg p-3 font-mono text-[11px] leading-relaxed text-muted">
					{report}
				</pre>
				<button
					type="button"
					onClick={copy}
					className="h-12 w-full rounded-full border border-white/[0.12] text-[15px] font-medium hover:bg-white/[0.04]"
				>
					{copied ? t.copied : t.copyLog}
				</button>
			</div>
		</Sheet>
	);
}
