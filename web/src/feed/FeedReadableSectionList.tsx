import { useCallback, useEffect, useRef, useState } from "react";
import { Copy, List, Newspaper, RefreshCcw } from "lucide-react";

import { Markdown } from "@/components/Markdown";
import { Button } from "@/components/ui/button";
import {
	Tooltip,
	TooltipContent,
	TooltipProvider,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import { FeedItems, type FeedCardListProps } from "@/feed/FeedList";
import type { DashboardReadableSection, FeedItem } from "@/feed/types";
import type {
	ReadableSectionDetails,
	ReadableSectionsError,
} from "@/feed/useDashboardReadableSections";
import type { DashboardReleaseTarget } from "@/dashboard/routeState";

const actionClass =
	"h-auto min-h-0 w-auto justify-end gap-1 rounded-none px-0 py-0 font-mono text-sm font-normal leading-[1.35] text-foreground/82 shadow-none hover:bg-transparent hover:text-foreground/82 focus-visible:ring-0";

function itemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

function mergeUnique(items: FeedItem[]) {
	const seen = new Set<string>();
	return items.filter((item) => {
		const key = itemKey(item);
		if (seen.has(key)) return false;
		seen.add(key);
		return true;
	});
}

function PaginationWave({ loading }: { loading: boolean }) {
	if (!loading) return null;
	return (
		<div
			className="flex justify-center pt-1"
			data-readable-pagination-loading="true"
			data-feed-pagination-loading="true"
		>
			<TooltipProvider delayDuration={500}>
				<Tooltip>
					<TooltipTrigger asChild>
						<span
							role="status"
							aria-label="加载中"
							className="inline-flex min-h-9 items-center justify-center gap-1 rounded-full border border-border/70 bg-card/80 px-5 shadow-sm"
						>
							{[0, 1, 2].map((index) => (
								<span
									key={index}
									className="feed-pagination-wave-dot size-1.5 rounded-full bg-foreground/70"
									data-feed-pagination-wave-dot="true"
								/>
							))}
						</span>
					</TooltipTrigger>
					<TooltipContent side="top" sideOffset={8}>
						加载中
					</TooltipContent>
				</Tooltip>
			</TooltipProvider>
		</div>
	);
}

export function FeedReadableSectionList(props: {
	sections: DashboardReadableSection[];
	details: Record<string, ReadableSectionDetails>;
	error: ReadableSectionsError | null;
	loadingInitial: boolean;
	loadingMore: boolean;
	hasMore: boolean;
	onLoadMore: () => void;
	onRetry: () => void;
	onLoadSectionItems: (sectionId: string, cursor?: string | null) => void;
	feedCardProps: Omit<FeedCardListProps, "items">;
	onOpenReleaseFromBrief?: (target: DashboardReleaseTarget) => void;
	onOpenBrief?: (briefId: string) => void;
	onCopyBrief?: (briefId: string) => void;
	onGenerateBriefForDate?: (date: string) => Promise<void>;
}) {
	const {
		sections,
		details,
		error,
		loadingInitial,
		loadingMore,
		hasMore,
		onLoadMore,
		onRetry,
		onLoadSectionItems,
		feedCardProps,
		onOpenReleaseFromBrief,
		onOpenBrief,
		onCopyBrief,
		onGenerateBriefForDate,
	} = props;
	const sentinelRef = useRef<HTMLDivElement | null>(null);
	const detailSentinelsRef = useRef(new Map<string, HTMLDivElement>());
	const detailRequestVisibleRef = useRef(new Set<string>());
	const requestVisibleRef = useRef(false);
	const [listSections, setListSections] = useState<Set<string>>(
		() => new Set(),
	);
	const [generatingDate, setGeneratingDate] = useState<string | null>(null);
	const handleGenerateBrief = useCallback(
		async (date: string) => {
			if (!onGenerateBriefForDate || generatingDate !== null) return;
			setGeneratingDate(date);
			try {
				await onGenerateBriefForDate(date);
				await onRetry();
			} finally {
				setGeneratingDate(null);
			}
		},
		[generatingDate, onGenerateBriefForDate, onRetry],
	);

	useEffect(() => {
		requestVisibleRef.current = false;
	}, [hasMore, sections.length]);

	useEffect(() => {
		if (!hasMore || loadingInitial || loadingMore || error?.phase === "append")
			return;
		const element = sentinelRef.current;
		if (!element || typeof IntersectionObserver === "undefined") return;
		const observer = new IntersectionObserver(
			(entries) => {
				if (
					entries.some((entry) => entry.isIntersecting) &&
					!requestVisibleRef.current
				) {
					requestVisibleRef.current = true;
					onLoadMore();
				}
				if (entries.every((entry) => !entry.isIntersecting))
					requestVisibleRef.current = false;
			},
			{ rootMargin: "0px", threshold: 0.01 },
		);
		observer.observe(element);
		return () => observer.disconnect();
	}, [error?.phase, hasMore, loadingInitial, loadingMore, onLoadMore]);

	useEffect(() => {
		if (typeof IntersectionObserver === "undefined") return;
		const observers: IntersectionObserver[] = [];
		const detailSectionIds = new Set([
			...listSections,
			...sections
				.filter((section) => !section.brief && section.items_next_cursor)
				.map((section) => section.id),
		]);
		for (const sectionId of detailSectionIds) {
			const section = sections.find((candidate) => candidate.id === sectionId);
			const detail = details[sectionId];
			const element = detailSentinelsRef.current.get(sectionId);
			const detailCursor = detail
				? detail.nextCursor
				: (section?.items_next_cursor ?? null);
			if (
				!element ||
				!detailCursor ||
				detail?.loading ||
				detail?.error ||
				detailRequestVisibleRef.current.has(`${sectionId}:${detailCursor}`)
			)
				continue;
			const observer = new IntersectionObserver(
				(entries) => {
					if (!entries.some((entry) => entry.isIntersecting)) return;
					const requestKey = `${sectionId}:${detailCursor}`;
					detailRequestVisibleRef.current.add(requestKey);
					onLoadSectionItems(sectionId, detailCursor);
					// Let a changed cursor or an explicit retry schedule a new request.
					detailRequestVisibleRef.current.delete(requestKey);
				},
				{ rootMargin: "0px", threshold: 0.01 },
			);
			observer.observe(element);
			observers.push(observer);
		}
		return () => {
			observers.forEach((observer) => {
				observer.disconnect();
			});
		};
	}, [details, listSections, onLoadSectionItems, sections]);

	const toggleList = useCallback(
		(sectionId: string) => {
			const enteringList = !listSections.has(sectionId);
			setListSections((current) => {
				const next = new Set(current);
				if (next.has(sectionId)) next.delete(sectionId);
				else next.add(sectionId);
				return next;
			});
			if (enteringList) onLoadSectionItems(sectionId);
		},
		[listSections, onLoadSectionItems],
	);

	if (loadingInitial && sections.length === 0) {
		return (
			<div
				className="space-y-3"
				data-readable-loading-initial="true"
				aria-busy="true"
			/>
		);
	}
	if (error?.phase === "initial" && sections.length === 0) {
		return (
			<div className="flex flex-col items-center gap-3 py-12 text-center">
				<p className="text-sm text-muted-foreground">{error.message}</p>
				<Button type="button" variant="outline" size="sm" onClick={onRetry}>
					<RefreshCcw className="size-4" />
					重试
				</Button>
			</div>
		);
	}

	return (
		<div className="space-y-4" data-readable-section-list="true">
			{sections.map((section) => {
				const sectionDate =
					section.date ||
					(section as DashboardReadableSection & { display_date?: string })
						.display_date ||
					"";
				const sectionCount =
					section.item_count ??
					(section as DashboardReadableSection & { activity_count?: number })
						.activity_count ??
					0;
				const brief = section.brief;
				const detail = details[section.id];
				const inList = listSections.has(section.id);
				const rawItems = brief
					? inList
						? (detail?.items ?? [])
						: []
					: mergeUnique([...(section.items ?? []), ...(detail?.items ?? [])]);
				const detailCursor = detail
					? detail.nextCursor
					: (section.items_next_cursor ?? null);
				const detailLoading = detail?.loading ?? false;
				const supplemental = section.supplemental_items ?? [];
				const shownItems = mergeUnique([...rawItems, ...supplemental]);
				return (
					<section
						key={section.id}
						className="space-y-3 sm:space-y-4"
						data-readable-section-id={section.id}
						data-feed-group-type={brief ? "historical" : "raw"}
						data-feed-brief-date={sectionDate}
					>
						<div
							className="flex items-center gap-3 px-1 text-sm text-muted-foreground"
							data-readable-section-header="true"
						>
							<span className="h-px flex-1 bg-border/70" />
							<span className="font-mono text-xs">
								{sectionDate} · {sectionCount} 条动态
							</span>
							<span className="h-px flex-1 bg-border/70" />
						</div>
						{brief && !inList ? (
							<div
								className="overflow-hidden rounded-[22px] bg-card/58 shadow-sm ring-1 ring-inset ring-border/60"
								data-readable-brief="true"
							>
								<div className="flex items-center justify-between gap-3 border-b border-dashed border-border/55 px-4 py-3 sm:px-6">
									<span className="inline-flex items-center gap-2 font-mono text-xs text-foreground/82">
										<Newspaper className="size-4" />
										日报
									</span>
									<div className="flex items-center gap-2">
										{onCopyBrief ? (
											<Button
												type="button"
												variant="ghost"
												size="icon"
												className="size-8 rounded-full"
												aria-label="复制"
												title="复制日报"
												onClick={() => onCopyBrief(brief.id)}
											>
												<Copy className="size-4" />
											</Button>
										) : null}
										{onOpenBrief ? (
											<Button
												type="button"
												variant="ghost"
												size="icon"
												className="size-8 rounded-full"
												aria-label="去日报"
												title="去日报"
												onClick={() => onOpenBrief(brief.id)}
											>
												<Newspaper className="size-4" />
											</Button>
										) : null}
									</div>
								</div>
								<div
									className="space-y-4 px-4 py-4 sm:px-6"
									data-brief-content-id={brief.id}
								>
									<Markdown
										content={brief.content_markdown}
										onInternalReleaseClick={onOpenReleaseFromBrief}
									/>
								</div>
							</div>
						) : null}
						{brief ? (
							<div className="flex justify-end px-1">
								<Button
									type="button"
									variant="ghost"
									size="sm"
									className={actionClass}
									onClick={() => toggleList(section.id)}
								>
									{inList ? (
										<Newspaper className="size-4" />
									) : (
										<List className="size-4" />
									)}
									{inList ? "日报" : "列表"}
								</Button>
							</div>
						) : null}
						{(inList || !brief) && detailLoading ? (
							<PaginationWave loading />
						) : null}
						{(inList || !brief) && detail?.error ? (
							<div className="flex items-center justify-end gap-2 px-1 text-xs text-destructive">
								<span>{detail.error}</span>
								<TooltipProvider delayDuration={500}>
									<Tooltip>
										<TooltipTrigger asChild>
											<Button
												type="button"
												variant="ghost"
												size="icon"
												className="size-8 rounded-full"
												aria-label="重试列表"
												onClick={() =>
													onLoadSectionItems(section.id, detail.nextCursor)
												}
											>
												<RefreshCcw className="size-4" />
											</Button>
										</TooltipTrigger>
										<TooltipContent>重试列表</TooltipContent>
									</Tooltip>
								</TooltipProvider>
							</div>
						) : null}
						{shownItems.length > 0 ? (
							<FeedItems items={shownItems} {...feedCardProps} />
						) : null}
						{(inList || !brief) && detailCursor ? (
							<div
								ref={(element) => {
									if (element)
										detailSentinelsRef.current.set(section.id, element);
									else detailSentinelsRef.current.delete(section.id);
								}}
								data-readable-detail-pagination-sentinel="true"
								className="h-1"
							/>
						) : null}
						{!brief && onGenerateBriefForDate ? (
							<div className="flex justify-end px-1">
								<Button
									type="button"
									variant="ghost"
									size="sm"
									className={actionClass}
									disabled={generatingDate === sectionDate}
									onClick={() => void handleGenerateBrief(sectionDate)}
								>
									<Newspaper className="size-4" />
									生成日报
								</Button>
							</div>
						) : null}
					</section>
				);
			})}
			<div
				ref={sentinelRef}
				data-readable-pagination-sentinel="true"
				data-feed-pagination-sentinel="true"
				className="h-1"
			/>
			<PaginationWave loading={loadingMore} />
			{error?.phase === "append" ? (
				<div className="flex items-center justify-center gap-2 text-xs text-destructive">
					<span>{error.message}</span>
					<TooltipProvider delayDuration={500}>
						<Tooltip>
							<TooltipTrigger asChild>
								<Button
									type="button"
									variant="ghost"
									size="icon"
									className="rounded-full"
									aria-label="重试加载"
									onClick={onRetry}
								>
									<RefreshCcw className="size-4" />
								</Button>
							</TooltipTrigger>
							<TooltipContent>重试加载</TooltipContent>
						</Tooltip>
					</TooltipProvider>
				</div>
			) : null}
			{!hasMore && sections.length > 0 ? (
				<p
					className="w-full text-center font-mono text-xs text-muted-foreground"
					data-readable-pagination-end="true"
				>
					已到尽头（共{" "}
					{sections.reduce(
						(sum, section) => sum + (section.item_count ?? 0),
						0,
					)}{" "}
					条）
				</p>
			) : null}
		</div>
	);
}
