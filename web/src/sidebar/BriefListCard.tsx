import { CalendarDays } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { formatIsoShortLocal } from "@/lib/datetime";
import { cn } from "@/lib/utils";
import type { BriefItem } from "@/sidebar/ReleaseDailyCard";

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
								<Button
									key={b.date}
									type="button"
									variant={active ? "outline" : "ghost"}
									size="sm"
									onClick={() => onSelectDate(b.date)}
									className={cn(
										"h-auto w-full justify-between gap-2 rounded-md border px-2 py-1 text-left font-mono text-xs shadow-none",
										active
											? "border-primary/40 bg-background hover:bg-background"
											: "border-transparent bg-background/40 hover:bg-background",
									)}
								>
									<span className={active ? "text-foreground" : ""}>
										#{b.date}
									</span>
									<span className="text-muted-foreground truncate text-[11px]">
										{formatIsoShortLocal(b.created_at)}
									</span>
								</Button>
							);
						})}
					</div>
				)}
			</CardContent>
		</Card>
	);
}
