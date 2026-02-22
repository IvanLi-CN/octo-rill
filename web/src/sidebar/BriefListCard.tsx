import { CalendarDays } from "lucide-react";

import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { cn } from "@/lib/utils";
import type { BriefItem } from "@/sidebar/ReleaseDailyCard";

function formatIsoShort(iso: string) {
	// "2026-02-21T08:00:00Z" -> "2026-02-21 08:00:00"
	// "2026-02-21T08:00:00.123Z" -> "2026-02-21 08:00:00"
	const noZ = iso.replace("Z", "");
	const noFrac = noZ.includes(".") ? noZ.split(".")[0] : noZ;
	return noFrac.replace("T", " ");
}

export function BriefListCard(props: {
	briefs: BriefItem[];
	selectedDate: string | null;
	onSelectDate: (date: string) => void;
}) {
	const { briefs, selectedDate, onSelectDate } = props;

	const list = briefs.slice(0, 12);

	return (
		<Card className="bg-card/80 shadow-sm">
			<CardHeader>
				<CardTitle className="inline-flex items-center gap-2">
					<CalendarDays className="size-4" />
					日报列表
				</CardTitle>
				<CardDescription className="font-mono text-xs">
					最近 {list.length} / 共 {briefs.length}
				</CardDescription>
			</CardHeader>

			<CardContent className="pt-0">
				{list.length === 0 ? (
					<p className="text-muted-foreground text-sm">
						暂无日报（右侧卡片可手动生成）
					</p>
				) : (
					<div className="space-y-1">
						{list.map((b) => {
							const active = selectedDate === b.date;
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
										{formatIsoShort(b.created_at)}
									</span>
								</button>
							);
						})}
					</div>
				)}
			</CardContent>
		</Card>
	);
}
