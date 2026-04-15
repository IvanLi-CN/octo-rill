import { Sparkles } from "lucide-react";

import { Markdown } from "@/components/Markdown";
import { Button } from "@/components/ui/button";
import { formatIsoRangeInTimeZone } from "@/lib/datetime";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

export type BriefItem = {
	id: string;
	date: string;
	window_start?: string | null;
	window_end?: string | null;
	effective_time_zone?: string | null;
	effective_local_boundary?: string | null;
	release_count: number;
	release_ids: string[];
	content_markdown: string;
	created_at: string;
};

function formatWindow(brief: BriefItem) {
	return formatIsoRangeInTimeZone(
		brief.window_start,
		brief.window_end,
		brief.effective_time_zone,
	);
}

export function ReleaseDailyCard(props: {
	briefs: BriefItem[];
	selectedId: string | null;
	busy: boolean;
	onGenerate: () => void;
	onOpenRelease?: (releaseId: string) => void;
}) {
	const { briefs, selectedId, busy, onGenerate, onOpenRelease } = props;

	const selected = (() => {
		if (selectedId) {
			const found = briefs.find((b) => b.id === selectedId);
			if (found) return found;
		}
		return briefs[0] ?? null;
	})();

	const windowText = selected ? formatWindow(selected) : null;
	const boundaryText =
		selected?.effective_local_boundary && selected?.effective_time_zone
			? `${selected.effective_local_boundary} · ${selected.effective_time_zone}`
			: (selected?.effective_local_boundary ??
				selected?.effective_time_zone ??
				null);

	return (
		<Card className="bg-card/80 shadow-sm">
			<CardHeader className="px-4 pb-3 pt-4 sm:px-6 sm:pb-4 sm:pt-6">
				<div className="flex items-start justify-between gap-3">
					<div>
						<CardTitle>Release 日报</CardTitle>
						<CardDescription className="font-mono text-xs">
							{selected ? (
								<>
									<span className="text-foreground">#{selected.date}</span>
									{windowText ? ` · ${windowText}` : ""}
									{boundaryText ? ` · ${boundaryText}` : ""}
								</>
							) : (
								"还没有生成日报"
							)}
						</CardDescription>
					</div>
					<Button
						variant="secondary"
						size="sm"
						className="font-mono text-xs"
						disabled={busy}
						onClick={onGenerate}
					>
						<Sparkles className="size-4" />
						{busy ? "生成中…" : "生成"}
					</Button>
				</div>
			</CardHeader>

			<CardContent className="px-4 pb-4 pt-0 sm:px-6 sm:pb-6">
				{briefs.length === 0 ? (
					<p className="text-muted-foreground text-sm">
						提示：日报会按你保存的“本地整点 + IANA
						时区”生成快照，历史窗口不会因为之后改设置而漂移。
					</p>
				) : selected ? (
					<div className="bg-muted/10 rounded-lg border p-3.5 sm:p-4">
						<Markdown
							content={selected.content_markdown}
							onInternalReleaseClick={onOpenRelease}
						/>
					</div>
				) : (
					<p className="text-muted-foreground text-sm">
						请选择一条日报查看内容。
					</p>
				)}
			</CardContent>
		</Card>
	);
}
