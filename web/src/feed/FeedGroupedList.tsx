import { List, LoaderCircle, Newspaper, Sparkles } from "lucide-react";
import {
	type HTMLAttributes,
	type ReactNode,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";

import { Markdown } from "@/components/Markdown";
import { Button } from "@/components/ui/button";
import { FeedItems, type FeedCardListProps } from "@/feed/FeedList";
import { groupFeedItemsByDay } from "@/feed/dayGroups";
import {
	isReleaseFeedItem,
	isSocialFeedItem,
	type FeedItem,
} from "@/feed/types";
import { cn } from "@/lib/utils";

type BriefLike = {
	date: string;
	content_markdown: string;
	created_at: string;
};

const FEED_DAY_ACTION_SLOT_CLASS =
	"flex h-8 w-[152px] shrink-0 items-center justify-end";
const FEED_DAY_ACTION_BUTTON_CLASS =
	"h-auto min-h-0 w-full justify-end gap-1 rounded-none px-0 py-0 font-mono text-[15px] font-normal leading-none tracking-wide text-foreground/82 shadow-none hover:bg-transparent hover:text-foreground/82 focus-visible:border-transparent focus-visible:ring-0 disabled:text-foreground/82 disabled:opacity-100";
const FEED_BRIEF_PANEL_CLASS =
	"bg-card/58 overflow-hidden rounded-[24px] shadow-sm ring-1 ring-inset ring-border/60";

function formatGroupCountLabel(releaseCount: number, activityCount: number) {
	if (releaseCount > 0 && activityCount > 0) {
		return `${releaseCount} 条 Release · ${activityCount} 条动态`;
	}
	if (releaseCount > 0) {
		return `${releaseCount} 条 Release`;
	}
	return `${activityCount} 条动态`;
}

function FeedDayHeader(props: {
	date: string;
	releaseCount: number;
	activityCount: number;
	action?: ReactNode;
	withDividerLines?: boolean;
	className?: string;
	actionSlotProps?: HTMLAttributes<HTMLDivElement> & {
		"data-feed-day-action-slot"?: string;
	};
}) {
	const {
		date,
		releaseCount,
		activityCount,
		action,
		withDividerLines = true,
		className,
		actionSlotProps,
	} = props;

	const label = (
		<p className="text-foreground/68 shrink-0 font-mono text-[15px] leading-none tracking-wide">
			<span className="text-foreground/82">{date}</span>
			{" · "}
			{formatGroupCountLabel(releaseCount, activityCount)}
		</p>
	);

	return (
		<div
			className={cn(
				"flex min-h-8 flex-wrap items-center gap-3 sm:min-h-8",
				className,
			)}
		>
			{withDividerLines ? (
				<div className="flex min-w-0 flex-1 items-center gap-4">
					<div className="bg-border/60 h-px flex-1" />
					{label}
					<div className="bg-border/60 h-px min-w-8 flex-1" />
				</div>
			) : (
				<div className="flex min-w-0 flex-1 items-center">{label}</div>
			)}
			{action ? (
				<div
					{...actionSlotProps}
					className={cn(FEED_DAY_ACTION_SLOT_CLASS, actionSlotProps?.className)}
				>
					{action}
				</div>
			) : null}
		</div>
	);
}

function FeedDayDivider(props: {
	date: string;
	releaseCount: number;
	activityCount: number;
	action?: ReactNode;
	showDivider?: boolean;
	className?: string;
}) {
	const {
		date,
		releaseCount,
		activityCount,
		action,
		showDivider = true,
		className,
	} = props;

	if (!showDivider) {
		return null;
	}

	return (
		<FeedDayHeader
			date={date}
			releaseCount={releaseCount}
			activityCount={activityCount}
			action={action}
			className={cn("px-1", className)}
		/>
	);
}

function FeedBriefBody(props: {
	brief: BriefLike | null;
	onOpenRelease?: (releaseId: string) => void;
}) {
	const { brief, onOpenRelease } = props;

	return (
		<div className="px-5 pb-4 pt-4 sm:px-6 sm:pb-5">
			{brief ? (
				<Markdown
					content={brief.content_markdown}
					onInternalReleaseClick={onOpenRelease}
				/>
			) : (
				<div className="space-y-3">
					<p className="text-muted-foreground text-sm">
						正在生成这一天的日报摘要…
					</p>
					<div className="space-y-2">
						<div className="bg-muted h-4 w-40 animate-pulse rounded" />
						<div className="bg-muted h-3 w-full animate-pulse rounded" />
						<div className="bg-muted h-3 w-11/12 animate-pulse rounded" />
						<div className="bg-muted h-3 w-9/12 animate-pulse rounded" />
					</div>
				</div>
			)}
		</div>
	);
}

function HistoricalBriefPanel(props: {
	brief: BriefLike | null;
	onOpenReleaseFromBrief?: (releaseId: string) => void;
}) {
	const { brief, onOpenReleaseFromBrief } = props;
	return (
		<div className={FEED_BRIEF_PANEL_CLASS}>
			<div className="flex items-center gap-2 border-b border-dashed border-border/55 px-5 py-[10px] text-foreground/82 sm:px-6">
				<Newspaper className="size-4" />
				<span className="font-mono text-[13px] tracking-wide">日报摘要</span>
			</div>
			<FeedBriefBody brief={brief} onOpenRelease={onOpenReleaseFromBrief} />
		</div>
	);
}

function FeedHistoricalDayGroup(props: {
	date: string;
	releaseCount: number;
	activityCount: number;
	action: ReactNode;
	showDivider: boolean;
	showBriefPanel: boolean;
	releaseOnly: boolean;
	brief: BriefLike | null;
	onOpenReleaseFromBrief?: (releaseId: string) => void;
	items: FeedItem[];
	feedCardProps: Omit<FeedCardListProps, "items">;
}) {
	const {
		date,
		releaseCount,
		activityCount,
		action,
		showDivider,
		showBriefPanel,
		releaseOnly,
		brief,
		onOpenReleaseFromBrief,
		items,
		feedCardProps,
	} = props;
	const releaseItems = items.filter((item) => isReleaseFeedItem(item));

	if (!showBriefPanel) {
		return (
			<div className="space-y-4">
				<FeedDayDivider
					date={date}
					releaseCount={releaseCount}
					activityCount={activityCount}
					action={action}
					showDivider={showDivider}
				/>
				<FeedItems
					items={releaseOnly ? releaseItems : items}
					{...feedCardProps}
				/>
			</div>
		);
	}

	const firstReleaseIndex = items.findIndex((item) => isReleaseFeedItem(item));
	const leadingItems =
		firstReleaseIndex > 0
			? items
					.slice(0, firstReleaseIndex)
					.filter((item) => isSocialFeedItem(item))
			: [];
	const trailingItems =
		firstReleaseIndex >= 0
			? items
					.slice(firstReleaseIndex + 1)
					.filter((item) => isSocialFeedItem(item))
			: items.filter((item) => isSocialFeedItem(item));

	return (
		<div className="space-y-4">
			<div className="px-1">
				<FeedDayHeader
					date={date}
					releaseCount={releaseCount}
					activityCount={activityCount}
					action={action}
					withDividerLines={showDivider}
					actionSlotProps={{
						"data-feed-day-action-slot": "true",
					}}
				/>
			</div>
			{leadingItems.length > 0 ? (
				<FeedItems items={leadingItems} {...feedCardProps} />
			) : null}
			<HistoricalBriefPanel
				brief={brief}
				onOpenReleaseFromBrief={onOpenReleaseFromBrief}
			/>
			{trailingItems.length > 0 ? (
				<FeedItems items={trailingItems} {...feedCardProps} />
			) : null}
		</div>
	);
}

export function FeedGroupedList(
	props: FeedCardListProps & {
		items: FeedItem[];
		error: string | null;
		loadingInitial: boolean;
		loadingMore: boolean;
		hasMore: boolean;
		onLoadMore: () => void;
		mode: "all" | "releases" | "stars" | "followers";
		briefs: BriefLike[];
		dailyBoundaryLocal: string | null | undefined;
		dailyBoundaryTimeZone: string | null | undefined;
		dailyBoundaryUtcOffsetMinutes: number | null | undefined;
		now?: Date;
		onOpenReleaseFromBrief?: (releaseId: string) => void;
		onGenerateBriefForDate?: (date: string) => Promise<void>;
	},
) {
	const {
		items,
		error,
		loadingInitial,
		loadingMore,
		hasMore,
		onLoadMore,
		mode,
		briefs,
		dailyBoundaryLocal,
		dailyBoundaryTimeZone,
		dailyBoundaryUtcOffsetMinutes,
		now,
		onOpenReleaseFromBrief,
		onGenerateBriefForDate,
		...feedCardProps
	} = props;

	const sentinelRef = useRef<HTMLDivElement | null>(null);
	const sentinelVisibleRef = useRef(false);
	const [releaseOnlyGroupIds, setReleaseOnlyGroupIds] = useState<Set<string>>(
		() => new Set<string>(),
	);
	const [pendingBriefDates, setPendingBriefDates] = useState<Set<string>>(
		() => new Set<string>(),
	);

	useEffect(() => {
		if (!hasMore || loadingInitial || loadingMore) return;
		const el = sentinelRef.current;
		if (!el) return;

		const obs = new IntersectionObserver(
			(entries) => {
				const isIntersecting = entries.some((entry) => entry.isIntersecting);
				if (isIntersecting && !sentinelVisibleRef.current) {
					sentinelVisibleRef.current = true;
					onLoadMore();
					return;
				}
				if (!isIntersecting) {
					sentinelVisibleRef.current = false;
				}
			},
			{ rootMargin: "900px 0px", threshold: 0.01 },
		);

		obs.observe(el);
		return () => obs.disconnect();
	}, [hasMore, loadingInitial, loadingMore, onLoadMore]);

	const groups = useMemo(
		() =>
			groupFeedItemsByDay(
				items,
				dailyBoundaryLocal,
				dailyBoundaryTimeZone,
				dailyBoundaryUtcOffsetMinutes,
				now,
			),
		[
			items,
			dailyBoundaryLocal,
			dailyBoundaryTimeZone,
			dailyBoundaryUtcOffsetMinutes,
			now,
		],
	);
	const briefByDate = useMemo(
		() => new Map(briefs.map((brief) => [brief.date, brief])),
		[briefs],
	);

	useEffect(() => {
		setReleaseOnlyGroupIds((current) => {
			const next = new Set<string>();
			for (const group of groups) {
				if (!current.has(group.id)) continue;
				next.add(group.id);
			}
			if (
				next.size === current.size &&
				Array.from(next).every((groupId) => current.has(groupId))
			) {
				return current;
			}
			return next;
		});
		setPendingBriefDates((current) => {
			const next = new Set<string>();
			for (const date of current) {
				const stillInFeed = groups.some((group) => group.briefDate === date);
				if (!stillInFeed) continue;
				if (!briefByDate.has(date)) {
					next.add(date);
				}
			}
			if (
				next.size === current.size &&
				Array.from(next).every((date) => current.has(date))
			) {
				return current;
			}
			return next;
		});
	}, [groups, briefByDate]);

	const skeletons = useMemo(() => Array.from({ length: 6 }, (_, i) => i), []);

	return (
		<div className="space-y-4">
			{error ? <p className="text-destructive text-sm">{error}</p> : null}

			{loadingInitial && items.length === 0 ? (
				<div className="space-y-4">
					{skeletons.map((i) => (
						<div
							key={i}
							className="bg-card/70 animate-pulse rounded-xl border p-6 shadow-sm"
						>
							<div className="h-3 w-48 rounded bg-muted" />
							<div className="mt-4 h-5 w-3/4 rounded bg-muted" />
							<div className="mt-2 h-3 w-2/3 rounded bg-muted" />
						</div>
					))}
				</div>
			) : null}

			{groups.map((group, index) => {
				const brief = briefByDate.get(group.briefDate) ?? null;
				const hasReleases = group.releaseCount > 0;
				const isHistoricalAllGroup =
					mode === "all" && !group.isCurrent && hasReleases;
				const pendingBrief = pendingBriefDates.has(group.briefDate);
				const showBriefPanel =
					isHistoricalAllGroup &&
					(pendingBrief ||
						(Boolean(brief) && !releaseOnlyGroupIds.has(group.id)));
				const showDivider = index > 0;
				let groupAction: ReactNode = null;

				if (isHistoricalAllGroup) {
					if (pendingBrief) {
						groupAction = (
							<Button
								type="button"
								variant="ghost"
								size="sm"
								disabled
								className={FEED_DAY_ACTION_BUTTON_CLASS}
							>
								<LoaderCircle className="size-4 animate-spin" />
								生成日报
							</Button>
						);
					} else if (brief && !releaseOnlyGroupIds.has(group.id)) {
						groupAction = (
							<Button
								type="button"
								variant="ghost"
								size="sm"
								className={FEED_DAY_ACTION_BUTTON_CLASS}
								onClick={() => {
									setReleaseOnlyGroupIds((current) => {
										const next = new Set(current);
										next.add(group.id);
										return next;
									});
								}}
							>
								<List className="size-4" />
								Releases
							</Button>
						);
					} else if (brief) {
						groupAction = (
							<Button
								type="button"
								variant="ghost"
								size="sm"
								className={FEED_DAY_ACTION_BUTTON_CLASS}
								onClick={() => {
									setReleaseOnlyGroupIds((current) => {
										const next = new Set(current);
										next.delete(group.id);
										return next;
									});
								}}
							>
								<Newspaper className="size-4" />
								日报
							</Button>
						);
					} else if (onGenerateBriefForDate) {
						groupAction = (
							<Button
								type="button"
								variant="ghost"
								size="sm"
								className={FEED_DAY_ACTION_BUTTON_CLASS}
								onClick={() => {
									setPendingBriefDates((current) => {
										const next = new Set(current);
										next.add(group.briefDate);
										return next;
									});
									void onGenerateBriefForDate(group.briefDate).catch(() => {
										setPendingBriefDates((current) => {
											const next = new Set(current);
											next.delete(group.briefDate);
											return next;
										});
									});
								}}
							>
								<Sparkles className="size-4" />
								生成日报
							</Button>
						);
					}
				}

				return (
					<section
						key={group.id}
						className="space-y-4"
						data-feed-group-id={group.id}
						data-feed-brief-date={group.briefDate}
						data-feed-group-type={
							isHistoricalAllGroup ? "historical" : "default"
						}
						data-feed-group-view={
							isHistoricalAllGroup
								? showBriefPanel
									? "brief"
									: "releases"
								: "default"
						}
					>
						{isHistoricalAllGroup ? (
							<FeedHistoricalDayGroup
								date={group.displayDate}
								releaseCount={group.releaseCount}
								activityCount={group.activityCount}
								action={groupAction}
								showDivider={showDivider}
								showBriefPanel={showBriefPanel}
								releaseOnly={brief != null && releaseOnlyGroupIds.has(group.id)}
								brief={pendingBrief ? null : brief}
								onOpenReleaseFromBrief={onOpenReleaseFromBrief}
								items={group.items}
								feedCardProps={feedCardProps}
							/>
						) : (
							<>
								<FeedDayDivider
									date={group.displayDate}
									releaseCount={group.releaseCount}
									activityCount={group.activityCount}
									action={groupAction}
									showDivider={showDivider}
								/>
								<FeedItems items={group.items} {...feedCardProps} />
							</>
						)}
					</section>
				);
			})}

			<div ref={sentinelRef} />

			{loadingMore ? (
				<p className="text-muted-foreground font-mono text-xs">加载中…</p>
			) : null}

			{!hasMore && items.length > 0 ? (
				<p className="text-muted-foreground font-mono text-xs">
					已到尽头（共 {items.length} 条）
				</p>
			) : null}
		</div>
	);
}
