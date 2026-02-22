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
	brief: BriefItem | null;
	busy: boolean;
	onGenerate: () => void;
}) {
	const { brief, busy, onGenerate } = props;
	const windowText = brief ? formatWindow(brief) : null;

	return (
		<Card className="bg-card/80 shadow-sm">
			<CardHeader>
				<div className="flex items-start justify-between gap-3">
					<div>
						<CardTitle>Release 日报</CardTitle>
						<CardDescription className="font-mono text-xs">
							{brief ? (
								<>
									<span className="text-foreground">#{brief.date}</span>
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
				{brief ? (
					<div className="space-y-3">
						<div className="bg-muted/10 max-h-96 overflow-auto rounded-lg border p-4">
							<Markdown content={brief.content_markdown} />
						</div>

						<details className="group">
							<summary className="text-muted-foreground cursor-pointer select-none font-mono text-xs">
								查看原始 Markdown
							</summary>
							<pre className="bg-muted/20 mt-2 max-h-72 overflow-auto rounded-lg border p-4 text-sm whitespace-pre-wrap">
								{brief.content_markdown}
							</pre>
						</details>
					</div>
				) : (
					<p className="text-muted-foreground text-sm">
						提示：日报基于 <code>AI_DAILY_AT_LOCAL</code>{" "}
						的时间边界统计“昨日更新”。
					</p>
				)}
			</CardContent>
		</Card>
	);
}
