import { ChevronDown } from "lucide-react";

import { cn } from "@/lib/utils";

type ErrorDetailDisclosureProps = {
	detail?: string | null;
	summary?: string | null;
	label?: string;
	className?: string;
};

export function ErrorDetailDisclosure(props: ErrorDetailDisclosureProps) {
	const { detail, summary, label = "查看详情", className } = props;
	const normalizedDetail = detail?.trim();
	if (!normalizedDetail) {
		return null;
	}
	if (summary?.trim() === normalizedDetail) {
		return null;
	}

	return (
		<details className={cn("group text-xs leading-5", className)}>
			<summary className="text-foreground/74 marker:hidden inline-flex cursor-pointer list-none items-center gap-1.5 rounded-full border border-destructive/14 bg-background/88 px-3 py-1.5 font-mono text-[11px] leading-none shadow-[0_2px_10px_rgba(15,23,42,0.06)] transition-colors hover:bg-background hover:text-foreground">
				<ChevronDown className="text-destructive/70 size-3.5 transition-transform group-open:rotate-180" />
				{label}
			</summary>
			<div className="mt-2.5 rounded-[0.95rem] border border-destructive/12 bg-destructive/[0.035] px-3.5 py-3 shadow-[inset_0_1px_0_rgba(255,255,255,0.36)]">
				<p className="text-foreground/82 whitespace-pre-wrap break-words">
					{normalizedDetail}
				</p>
			</div>
		</details>
	);
}
