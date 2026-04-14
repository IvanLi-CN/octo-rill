import { Button } from "@/components/ui/button";
import { FEED_LANE_OPTIONS } from "@/feed/laneOptions";
import type { FeedLane } from "@/feed/types";
import { cn } from "@/lib/utils";

export function FeedPageLaneSelector(props: {
	value: FeedLane;
	onValueChange: (lane: FeedLane) => void;
	className?: string;
	distributed?: boolean;
}) {
	const { value, onValueChange, className, distributed = false } = props;

	return (
		<fieldset
			className={cn(
				distributed
					? "flex h-8 w-full items-center gap-1 rounded-full border border-border/45 bg-muted/45 p-1 shadow-sm"
					: "inline-flex h-7 items-center gap-0.5 rounded-full border border-border/45 bg-muted/45 p-0.5 shadow-sm sm:h-8",
				className,
			)}
		>
			<legend className="sr-only">默认显示模式</legend>
			{FEED_LANE_OPTIONS.map((option) => {
				const Icon = option.icon;
				const active = option.lane === value;
				return (
					<Button
						key={option.lane}
						type="button"
						variant="ghost"
						size="sm"
						data-feed-page-lane={option.lane}
						aria-pressed={active}
						aria-label={option.label}
						className={cn(
							distributed
								? "h-6 min-w-0 flex-1 justify-center rounded-full px-0 text-xs font-medium shadow-none transition-all"
								: "h-6 rounded-[14px] px-2.5 text-[13px] font-medium shadow-none transition-all sm:h-7 sm:px-3.5 sm:text-sm",
							active
								? "border border-border/80 bg-background text-foreground shadow-[0_1px_2px_rgba(15,23,42,0.12),inset_0_1px_0_rgba(255,255,255,0.7)] hover:bg-background"
								: "border border-transparent bg-transparent text-foreground/40 hover:text-foreground/75",
						)}
						onClick={() => onValueChange(option.lane)}
					>
						<Icon className="size-3.5" />
						<span>{option.label}</span>
					</Button>
				);
			})}
		</fieldset>
	);
}
