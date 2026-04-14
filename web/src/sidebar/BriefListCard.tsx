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
	selectedId: string | null;
	onSelectId: (id: string) => void;
}) {
	const { briefs, selectedId, onSelectId } = props;

	const list = briefs.slice(0, 12);

	return (
		<Card className="bg-card/80 shadow-sm">
			<CardHeader className="px-4 pb-3 pt-4 sm:px-6 sm:pb-4 sm:pt-6">
				<CardTitle className="inline-flex items-center gap-2">
					<CalendarDays className="size-4" />
					日报列表
				</CardTitle>
				<CardDescription className="font-mono text-xs">
					最近 {list.length} / 共 {briefs.length}
				</CardDescription>
			</CardHeader>

			<CardContent className="px-4 pb-4 pt-0 sm:px-6 sm:pb-6">
				{list.length === 0 ? (
					<p className="text-muted-foreground text-sm">
						暂无日报（右侧卡片可手动生成）
					</p>
				) : (
					<div className="space-y-1">
						{list.map((b) => {
							const active = selectedId === b.id;
							return (
								<Button
									key={b.id}
									type="button"
									variant={active ? "outline" : "ghost"}
									size="sm"
									onClick={() => onSelectId(b.id)}
									className={cn(
										"h-auto w-full justify-between gap-2 rounded-md border px-2 py-1 text-left font-mono text-xs shadow-none",
										active
											? "border-primary/40 bg-background hover:bg-background"
											: "border-transparent bg-background/40 hover:bg-background",
									)}
								>
									<span
										className={cn("min-w-0", active ? "text-foreground" : "")}
									>
										<span>#{b.date}</span>
										<span className="text-muted-foreground ml-2 text-[11px]">
											{b.release_count} 条
										</span>
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
