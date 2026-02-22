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
import { cn } from "@/lib/utils";

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
	onSelectDate: (date: string) => void;
	busy: boolean;
	onGenerate: () => void;
}) {
	const { briefs, selectedDate, onSelectDate, busy, onGenerate } = props;

	const selected = (() => {
		if (selectedDate) {
			const found = briefs.find((b) => b.date === selectedDate);
			if (found) return found;
		}
		return briefs[0] ?? null;
	})();

	const windowText = selected ? formatWindow(selected) : null;
	const list = briefs.slice(0, 10);

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
				{briefs.length > 0 ? (
					<div className="space-y-3">
						<div className="space-y-2">
							<p className="text-muted-foreground font-mono text-[11px]">
								日报列表（最近 {list.length} / 共 {briefs.length}）
							</p>
							<div className="max-h-28 space-y-1 overflow-auto rounded-lg border bg-background/30 p-2">
								{list.map((b) => {
									const active = selected?.date === b.date;
									return (
										<button
											key={b.date}
											type="button"
											onClick={() => onSelectDate(b.date)}
											className={cn(
												"flex w-full items-center justify-between gap-2 rounded-md border px-2 py-1 text-left font-mono text-xs transition-colors hover:bg-background",
												active
													? "border-primary/40 bg-background"
													: "border-transparent bg-background/40",
											)}
										>
											<span className={active ? "text-foreground" : ""}>
												#{b.date}
											</span>
											<span className="text-muted-foreground truncate text-[11px]">
												{b.created_at.replace("T", " ").replace("Z", "")}
											</span>
										</button>
									);
								})}
							</div>
						</div>

						<div className="bg-muted/10 max-h-96 overflow-auto rounded-lg border p-4">
							<Markdown content={selected?.content_markdown ?? ""} />
						</div>

						<details className="group">
							<summary className="text-muted-foreground cursor-pointer select-none font-mono text-xs">
								查看原始 Markdown
							</summary>
							<pre className="bg-muted/20 mt-2 max-h-72 overflow-auto rounded-lg border p-4 text-sm whitespace-pre-wrap">
								{selected?.content_markdown ?? ""}
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
