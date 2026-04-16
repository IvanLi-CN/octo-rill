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
import {
	type BriefSnapshotCandidate,
	groupFeedItemsByDay,
} from "@/feed/dayGroups";
import { isReleaseFeedItem, type FeedItem } from "@/feed/types";
import { cn } from "@/lib/utils";

type BriefLike = BriefSnapshotCandidate & {
	date: string;
	window_start?: string | null;
	effective_time_zone?: string | null;
	effective_local_boundary?: string | null;
	release_count?: number;
	content_markdown: string;
	created_at: string;
};

const FEED_DAY_ACTION_SLOT_CLASS =
	"flex w-full items-start justify-start pt-1 sm:h-8 sm:w-[152px] sm:shrink-0 sm:items-center sm:justify-end sm:pt-0";
const FEED_DAY_ACTION_BUTTON_CLASS =
	"h-auto min-h-0 w-auto justify-start gap-1 rounded-none px-0 py-0 font-mono text-[14px] font-normal leading-[1.35] tracking-[0.02em] text-foreground/82 shadow-none hover:bg-transparent hover:text-foreground/82 focus-visible:border-transparent focus-visible:ring-0 disabled:text-foreground/82 disabled:opacity-100 sm:w-full sm:justify-end sm:text-[15px] sm:leading-none sm:tracking-wide";
const FEED_BRIEF_PANEL_CLASS =
	"bg-card/58 overflow-hidden rounded-[22px] shadow-sm ring-1 ring-inset ring-border/60 sm:rounded-[24px]";

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
		<p
			className="min-w-0 max-w-full text-center font-mono text-[14px] leading-[1.35] tracking-[0.02em] text-foreground/68 whitespace-normal sm:text-[15px] sm:leading-none sm:tracking-wide"
			data-feed-day-label="true"
		>
			<span className="text-foreground/82">{date}</span>
			{" · "}
			{formatGroupCountLabel(releaseCount, activityCount)}
		</p>
	);

	return (
		<div
			className={cn(
				"flex flex-col gap-2 py-0.5 sm:min-h-8 sm:flex-row sm:flex-wrap sm:items-center sm:gap-3 sm:py-0",
				className,
			)}
			data-feed-day-header="true"
		>
			{withDividerLines ? (
				<div className="flex min-w-0 w-full flex-1 items-center gap-2 sm:w-auto sm:gap-4">
					<div
						className="bg-border/60 h-px min-w-4 flex-1 sm:min-w-8"
						data-feed-day-divider-before="true"
					/>
					{label}
					<div
						className="bg-border/60 h-px min-w-4 flex-1 sm:min-w-8"
						data-feed-day-divider-after="true"
					/>
				</div>
			) : (
				<div className="flex min-w-0 w-full flex-1 items-center sm:w-auto">
					{label}
				</div>
			)}
			{action ? (
				<div
					data-feed-day-action-slot="true"
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
		<div className="px-4 pb-4 pt-4 sm:px-6 sm:pb-5">
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
			<div className="flex items-center gap-2 border-b border-dashed border-border/55 px-4 py-[10px] text-foreground/82 sm:px-6">
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
		brief,
		onOpenReleaseFromBrief,
		items,
		feedCardProps,
	} = props;

	if (!showBriefPanel) {
		return (
			<div className="space-y-3 sm:space-y-4">
				<FeedDayDivider
					date={date}
					releaseCount={releaseCount}
					activityCount={activityCount}
					action={action}
					showDivider={showDivider}
				/>
				<FeedItems items={items} {...feedCardProps} />
			</div>
		);
	}

	const hiddenReleaseIds = new Set(brief?.release_ids ?? []);
	const firstHiddenReleaseIndex = items.findIndex(
		(item) => isReleaseFeedItem(item) && hiddenReleaseIds.has(item.id),
	);
	const leadingItems =
		firstHiddenReleaseIndex > 0
			? items
					.slice(0, firstHiddenReleaseIndex)
					.filter(
						(item) =>
							!isReleaseFeedItem(item) || !hiddenReleaseIds.has(item.id),
					)
			: [];
	const trailingItems =
		firstHiddenReleaseIndex >= 0
			? items
					.slice(firstHiddenReleaseIndex + 1)
					.filter(
						(item) =>
							!isReleaseFeedItem(item) || !hiddenReleaseIds.has(item.id),
					)
			: items;

	return (
		<div className="space-y-3 sm:space-y-4">
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
	const [rawListGroupIds, setRawListGroupIds] = useState<Set<string>>(
		() => new Set<string>(),
	);
	const [pendingGroupIds, setPendingGroupIds] = useState<Set<string>>(
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
				mode === "all" ? briefs : [],
				now,
			),
		[
			items,
			dailyBoundaryLocal,
			dailyBoundaryTimeZone,
			dailyBoundaryUtcOffsetMinutes,
			briefs,
			now,
			mode,
		],
	);
	const briefById = useMemo(
		() => new Map(briefs.map((brief) => [brief.id, brief])),
		[briefs],
	);
	const briefByDate = useMemo(
		() => new Map(briefs.map((brief) => [brief.date, brief])),
		[briefs],
	);

	useEffect(() => {
		setRawListGroupIds((current) => {
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
		setPendingGroupIds((current) => {
			const next = new Set<string>();
			for (const groupId of current) {
				const stillInFeed = groups.some((group) => group.id === groupId);
				if (!stillInFeed) continue;
				const group = groups.find((item) => item.id === groupId);
				if (group?.kind === "raw") {
					next.add(groupId);
				}
			}
			if (
				next.size === current.size &&
				Array.from(next).every((groupId) => current.has(groupId))
			) {
				return current;
			}
			return next;
		});
	}, [groups]);

	const skeletons = useMemo(() => Array.from({ length: 6 }, (_, i) => i), []);

	return (
		<div className="space-y-3 sm:space-y-4">
			{error ? <p className="text-destructive text-sm">{error}</p> : null}

			{loadingInitial && items.length === 0 ? (
				<div className="space-y-3 sm:space-y-4">
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
				const brief =
					(group.briefId ? briefById.get(group.briefId) : null) ??
					(group.kind === "historical"
						? briefByDate.get(group.briefDate)
						: null) ??
					null;
				const hasReleases = group.releaseCount > 0;
				const isHistoricalRawGroup =
					mode === "all" &&
					group.kind === "raw" &&
					!group.isCurrent &&
					hasReleases;
				const pendingBrief = pendingGroupIds.has(group.id);
				const showBriefPanel =
					mode === "all" &&
					group.kind === "historical" &&
					(Boolean(brief) || pendingBrief) &&
					!rawListGroupIds.has(group.id);
				const showDivider = index > 0;
				let groupAction: ReactNode = null;

				if (mode === "all" && group.kind === "historical") {
					if (brief && !rawListGroupIds.has(group.id)) {
						groupAction = (
							<Button
								type="button"
								variant="ghost"
								size="sm"
								className={FEED_DAY_ACTION_BUTTON_CLASS}
								onClick={() => {
									setRawListGroupIds((current) => {
										const next = new Set(current);
										next.add(group.id);
										return next;
									});
								}}
							>
								<List className="size-4" />
								列表
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
									setRawListGroupIds((current) => {
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
					}
				} else if (isHistoricalRawGroup) {
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
					} else if (onGenerateBriefForDate) {
						groupAction = (
							<Button
								type="button"
								variant="ghost"
								size="sm"
								className={FEED_DAY_ACTION_BUTTON_CLASS}
								onClick={() => {
									setPendingGroupIds((current) => {
										const next = new Set(current);
										next.add(group.id);
										return next;
									});
									void onGenerateBriefForDate(group.briefDate).catch(() => {
										setPendingGroupIds((current) => {
											const next = new Set(current);
											next.delete(group.id);
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
						className="space-y-3 sm:space-y-4"
						data-feed-group-id={group.id}
						data-feed-brief-date={group.briefDate}
						data-feed-group-type={
							group.kind === "historical" ? "historical" : "default"
						}
						data-feed-group-view={
							group.kind === "historical"
								? showBriefPanel
									? "brief"
									: "raw"
								: "default"
						}
					>
						{mode === "all" && group.kind === "historical" ? (
							<FeedHistoricalDayGroup
								date={group.displayDate}
								releaseCount={group.releaseCount}
								activityCount={group.activityCount}
								action={groupAction}
								showDivider={showDivider}
								showBriefPanel={showBriefPanel}
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
