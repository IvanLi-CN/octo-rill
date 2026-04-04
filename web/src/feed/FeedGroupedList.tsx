import { ChevronDown, ChevronRight } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";

import { Markdown } from "@/components/Markdown";
import { Button } from "@/components/ui/button";
import { FeedItems, type FeedCardListProps } from "@/feed/FeedList";
import { groupFeedItemsByDay } from "@/feed/dayGroups";
import type { FeedItem } from "@/feed/types";
import { formatIsoShortLocal } from "@/lib/datetime";

type BriefLike = {
	date: string;
	content_markdown: string;
	created_at: string;
};

function FeedDayDivider(props: {
	date: string;
	releaseCount: number;
	action?: ReactNode;
}) {
	const { date, releaseCount, action } = props;

	return (
		<div className="flex flex-wrap items-center gap-3">
			<div className="flex min-w-0 flex-1 items-center gap-3">
				<div className="bg-border/60 h-px flex-1" />
				<p className="text-muted-foreground shrink-0 font-mono text-[11px] tracking-wide">
					<span className="text-foreground/80">{date}</span>
					{" · "}
					{releaseCount} 条 Release
				</p>
				<div className="bg-border/60 h-px flex-1" />
			</div>
			{action}
		</div>
	);
}

function FeedBriefSummary(props: {
	brief: BriefLike;
	onOpenRelease?: (releaseId: string) => void;
}) {
	const { brief, onOpenRelease } = props;

	return (
		<div className="bg-card/60 rounded-xl border border-dashed p-4 shadow-sm">
			<div className="mb-3 flex items-center justify-between gap-3">
				<span className="text-muted-foreground font-mono text-[11px] tracking-wide">
					日报
				</span>
				<span className="text-muted-foreground font-mono text-[11px]">
					{formatIsoShortLocal(brief.created_at)}
				</span>
			</div>
			<Markdown
				content={brief.content_markdown}
				onInternalReleaseClick={onOpenRelease}
			/>
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
		mode: "all" | "releases";
		briefs: BriefLike[];
		dailyBoundaryLocal: string | null | undefined;
		dailyBoundaryTimeZone: string | null | undefined;
		dailyBoundaryUtcOffsetMinutes: number | null | undefined;
		now?: Date;
		onOpenReleaseFromBrief?: (releaseId: string) => void;
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
		...feedCardProps
	} = props;

	const sentinelRef = useRef<HTMLDivElement | null>(null);
	const sentinelVisibleRef = useRef(false);
	const [expandedGroupIds, setExpandedGroupIds] = useState<Set<string>>(
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
		setExpandedGroupIds((current) => {
			const next = new Set<string>();
			for (const group of groups) {
				if (!current.has(group.id)) continue;
				if (!group.isCurrent && briefByDate.has(group.briefDate)) {
					next.add(group.id);
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

			{groups.map((group) => {
				const brief = briefByDate.get(group.briefDate) ?? null;
				const canCollapseToBrief =
					mode === "all" && !group.isCurrent && Boolean(brief);
				const expanded = canCollapseToBrief
					? expandedGroupIds.has(group.id)
					: true;

				return (
					<section key={group.id} className="space-y-4">
						<FeedDayDivider
							date={group.displayDate}
							releaseCount={group.releaseCount}
							action={
								canCollapseToBrief ? (
									<Button
										type="button"
										variant="ghost"
										size="sm"
										className="text-muted-foreground h-7 rounded-full px-2 font-mono text-[11px]"
										onClick={() => {
											setExpandedGroupIds((current) => {
												const next = new Set(current);
												if (next.has(group.id)) {
													next.delete(group.id);
												} else {
													next.add(group.id);
												}
												return next;
											});
										}}
									>
										{expanded ? (
											<ChevronDown className="size-3.5" />
										) : (
											<ChevronRight className="size-3.5" />
										)}
										{expanded ? "收起 Releases" : "展开 Releases"}
									</Button>
								) : null
							}
						/>

						{canCollapseToBrief && brief ? (
							<FeedBriefSummary
								brief={brief}
								onOpenRelease={onOpenReleaseFromBrief}
							/>
						) : null}

						{expanded ? (
							<div className="space-y-4">
								{canCollapseToBrief ? (
									<p className="text-muted-foreground font-mono text-[11px]">
										原始 Releases
									</p>
								) : null}
								<FeedItems items={group.items} {...feedCardProps} />
							</div>
						) : null}
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
