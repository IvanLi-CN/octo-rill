import { Button } from "@/components/ui/button";
import { FEED_LANE_OPTIONS } from "@/feed/laneOptions";
import type { FeedLane } from "@/feed/types";
import { cn } from "@/lib/utils";

export function FeedPageLaneSelector(props: {
	value: FeedLane;
	onValueChange: (lane: FeedLane) => void;
	className?: string;
	distributed?: boolean;
	disabled?: boolean;
}) {
	const {
		value,
		onValueChange,
		className,
		distributed = false,
		disabled = false,
	} = props;

	return (
		<fieldset
			data-feed-page-lane-selector="true"
			disabled={disabled}
			aria-disabled={disabled}
			className={cn(
				distributed
					? "flex h-8 w-full items-center gap-1 rounded-lg border border-border/60 bg-muted/55 p-1"
					: "inline-flex h-8 items-center gap-1 rounded-lg border border-border/60 bg-muted/55 p-1",
				"transition-colors dark:border-border/70 dark:bg-muted/35",
				disabled && "opacity-60",
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
						disabled={disabled}
						className={cn(
							distributed
								? "h-6 min-w-0 flex-1 justify-center rounded-md px-0 text-xs font-medium shadow-none transition-colors"
								: "h-6 rounded-md px-2.5 text-[13px] font-medium shadow-none transition-colors sm:px-3",
							active
								? "border border-border/70 bg-background text-foreground shadow-xs hover:bg-background dark:border-border/80 dark:bg-background/80"
								: "border border-transparent bg-transparent text-foreground/55 hover:bg-background/45 hover:text-foreground/80 dark:text-foreground/50 dark:hover:bg-background/25 dark:hover:text-foreground/80",
							disabled && "cursor-not-allowed hover:bg-transparent",
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
