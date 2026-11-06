// A problem in words, with the error as it came a tap away.
import { useI18n } from '../i18n';
import { useExplain, type Problem } from '../lib/problem';

export function ProblemText({ problem, className = '' }: { problem: Problem; className?: string }) {
	const { t } = useI18n();
	const explain = useExplain();
	return (
		<div className={`flex flex-col gap-1 text-[13px] leading-snug ${className}`}>
			<p className="text-red-400">{explain(problem)}</p>
			<details className="text-muted">
				<summary className="cursor-pointer select-none text-xs">{t.problemDetails}</summary>
				<p className="mt-1 break-words font-mono text-[11px]">{problem.detail}</p>
			</details>
		</div>
	);
}
