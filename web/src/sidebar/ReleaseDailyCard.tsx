import { Sparkles } from "lucide-react";

import { Markdown } from "@/components/Markdown";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

export type BriefItem = {
	date: string;
	window_start?: string | null;
	window_end?: string | null;
	content_markdown: string;
	created_at: string;
};

function formatWindow(brief: BriefItem) {
	if (!brief.window_start || !brief.window_end) return null;
	return `${brief.window_start} → ${brief.window_end}`;
}

export function ReleaseDailyCard(props: {
	briefs: BriefItem[];
	selectedDate: string | null;
	busy: boolean;
	onGenerate: () => void;
	onOpenRelease?: (releaseId: string) => void;
}) {
	const { briefs, selectedDate, busy, onGenerate, onOpenRelease } = props;

	const selected = (() => {
		if (selectedDate) {
			const found = briefs.find((b) => b.date === selectedDate);
			if (found) return found;
		}
		return briefs[0] ?? null;
	})();

	const windowText = selected ? formatWindow(selected) : null;

	return (
		<Card className="bg-card/80 shadow-sm">
			<CardHeader>
				<div className="flex items-start justify-between gap-3">
					<div>
						<CardTitle>Release 日报</CardTitle>
						<CardDescription className="font-mono text-xs">
							{selected ? (
								<>
									<span className="text-foreground">#{selected.date}</span>
									{windowText ? ` · ${windowText}` : ""}
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

			<CardContent className="pt-0">
				{briefs.length === 0 ? (
					<p className="text-muted-foreground text-sm">
						提示：日报基于 <code>AI_DAILY_AT_LOCAL</code>{" "}
						的时间边界统计“昨日更新”。
					</p>
				) : selected ? (
					<div className="bg-muted/10 max-h-96 overflow-auto rounded-lg border p-4">
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
