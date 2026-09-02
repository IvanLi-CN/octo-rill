import {
	type RefObject,
	useCallback,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import { ArrowDown, Copy, List, Newspaper, RefreshCcw } from "lucide-react";

import { Markdown } from "@/components/Markdown";
import { Button } from "@/components/ui/button";
import { Chip } from "@/components/ui/chip";
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

type PaginationFeedbackState =
	| { kind: "load-more" }
	| { kind: "loading" }
	| { kind: "error"; message: string };

const noop = () => {};
const loadingChipWidth = 56;
const loadMoreChipFallbackWidth = 94;
const feedbackExitDurationMs = 180;

function useMeasuredChipWidth(
	measureRef: RefObject<HTMLElement | null>,
	deps: readonly unknown[],
) {
	const [width, setWidth] = useState<number | null>(null);

	useLayoutEffect(() => {
		const measure = measureRef.current;
		if (!measure) return;
		const update = () => {
			const nextWidth = Math.ceil(
				Math.max(measure.getBoundingClientRect().width, measure.scrollWidth),
			);
			setWidth((current) => (current === nextWidth ? current : nextWidth));
		};
		update();
		const observer =
			typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
		observer?.observe(measure);
		void document.fonts?.ready.then(update);
		return () => observer?.disconnect();
	}, deps);

	return width;
}

function getPaginationFeedback(
	hasMore: boolean,
	loading: boolean,
	error: ReadableSectionsError | null,
): PaginationFeedbackState | null {
	if (error?.phase === "append") {
		return { kind: "error", message: error.message };
	}
	if (loading) return { kind: "loading" };
	return hasMore ? { kind: "load-more" } : null;
}

function PaginationFeedbackChip(props: {
	feedback: PaginationFeedbackState;
	errorMessage: string;
	onLoadMore: () => void;
	onRetry: () => void;
	leaving: boolean;
}) {
	const { feedback, errorMessage, onLoadMore, onRetry, leaving } = props;
	const loadMoreMeasureRef = useRef<HTMLSpanElement | null>(null);
	const errorMeasureRef = useRef<HTMLSpanElement | null>(null);
	const loadMoreWidth = useMeasuredChipWidth(loadMoreMeasureRef, []);
	const errorWidth = useMeasuredChipWidth(errorMeasureRef, [errorMessage]);
	const isLoadMore = feedback.kind === "load-more";
	const isLoading = feedback.kind === "loading";
	const isError = feedback.kind === "error";

	const chipWidth = isLoading
		? loadingChipWidth
		: isLoadMore
			? (loadMoreWidth ?? loadMoreChipFallbackWidth)
			: (errorWidth ?? 248);
	const chip = (
		<Chip
			variant={isError ? "destructive" : "neutral"}
			role={isLoading && !leaving ? "status" : undefined}
			aria-label={isLoading && !leaving ? "加载中" : undefined}
			aria-hidden={leaving || undefined}
			data-readable-pagination-loading={
				isLoading && !leaving ? "true" : undefined
			}
			data-feed-pagination-loading={isLoading && !leaving ? "true" : undefined}
			data-feed-pagination-chip="true"
			data-feed-pagination-state={feedback.kind}
			data-feed-pagination-leaving={leaving || undefined}
			className="feed-pagination-feedback-chip relative isolate max-w-full shrink-0 px-0"
			style={{ width: `min(100%, ${chipWidth}px)` }}
		>
			<span
				ref={loadMoreMeasureRef}
				aria-hidden="true"
				className="pointer-events-none invisible absolute left-0 top-0 inline-flex h-6 items-center gap-1.5 whitespace-nowrap px-2.5 text-xs font-medium leading-4"
			>
				<ArrowDown className="size-3.5 shrink-0" />
				<span>加载更多</span>
			</span>
			<span
				ref={errorMeasureRef}
				aria-hidden="true"
				className="pointer-events-none invisible absolute left-0 top-0 inline-flex h-6 items-center gap-1.5 whitespace-nowrap px-2.5 text-xs font-medium leading-4"
			>
				<span>{errorMessage}</span>
				<RefreshCcw className="size-3.5 shrink-0" />
			</span>
			<span
				aria-hidden={!isLoading || leaving}
				className="feed-pagination-feedback-loader absolute inset-0 flex items-center justify-center gap-1.5"
			>
				{[0, 1, 2].map((index) => (
					<span
						key={index}
						className="feed-pagination-wave-dot size-1.5 rounded-full bg-foreground/70"
						data-feed-pagination-wave-dot="true"
					/>
				))}
			</span>
			<button
				type="button"
				aria-hidden={!isLoadMore || leaving}
				tabIndex={isLoadMore && !leaving ? 0 : -1}
				aria-label="加载更多"
				data-feed-pagination-load-more-chip={isLoadMore ? "true" : undefined}
				onClick={onLoadMore}
				className="feed-pagination-feedback-load-more absolute -inset-px flex min-w-0 items-center gap-1.5 rounded-full px-2.5 text-left outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
			>
				<ArrowDown className="size-3.5 shrink-0" />
				<span>加载更多</span>
			</button>
			<button
				type="button"
				aria-hidden={!isError || leaving}
				tabIndex={isError && !leaving ? 0 : -1}
				aria-label={`重试加载：${errorMessage}`}
				data-feed-pagination-error-chip={isError ? "true" : undefined}
				onClick={onRetry}
				className="feed-pagination-feedback-error absolute -inset-px flex min-w-0 items-center gap-1.5 rounded-full px-2.5 text-left outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
			>
				<span className="min-w-0 truncate">{errorMessage}</span>
				<RefreshCcw className="size-3.5 shrink-0" />
			</button>
		</Chip>
	);

	return (
		<div
			role={isError && !leaving ? "alert" : undefined}
			aria-live={isError && !leaving ? "assertive" : undefined}
			className="contents"
		>
			<TooltipProvider delayDuration={500}>
				<Tooltip>
					<TooltipTrigger asChild>{chip}</TooltipTrigger>
					<TooltipContent side="top" sideOffset={8}>
						{isLoadMore ? "加载更多" : isError ? "重试加载" : "加载中"}
					</TooltipContent>
				</Tooltip>
			</TooltipProvider>
		</div>
	);
}

function PaginationFeedback(props: {
	hasMore?: boolean;
	loading: boolean;
	error: ReadableSectionsError | null;
	onLoadMore?: () => void;
	onRetry?: () => void;
}) {
	const {
		hasMore = false,
		loading,
		error,
		onLoadMore = noop,
		onRetry = noop,
	} = props;
	const desiredFeedback = getPaginationFeedback(hasMore, loading, error);
	const desiredKey = desiredFeedback
		? desiredFeedback.kind === "error"
			? `error:${desiredFeedback.message}`
			: desiredFeedback.kind
		: "idle";
	const [currentFeedback, setCurrentFeedback] = useState(desiredFeedback);
	const [errorMessage, setErrorMessage] = useState(
		desiredFeedback?.kind === "error" ? desiredFeedback.message : "",
	);
	const [leaving, setLeaving] = useState(false);
	const currentFeedbackRef = useRef(currentFeedback);

	useEffect(() => {
		if (desiredFeedback) {
			if (desiredFeedback.kind === "error") {
				setErrorMessage(desiredFeedback.message);
			}
			currentFeedbackRef.current = desiredFeedback;
			setCurrentFeedback(desiredFeedback);
			setLeaving(false);
			return;
		}
		if (!currentFeedbackRef.current) return;
		setLeaving(true);
		const reduceMotion = window.matchMedia?.(
			"(prefers-reduced-motion: reduce)",
		).matches;
		const timeout = window.setTimeout(
			() => {
				currentFeedbackRef.current = null;
				setCurrentFeedback(null);
				setLeaving(false);
			},
			reduceMotion ? 0 : feedbackExitDurationMs,
		);
		return () => window.clearTimeout(timeout);
	}, [desiredKey]);

	if (!currentFeedback) return null;

	return (
		<div
			className="feed-pagination-feedback-slot"
			data-feed-pagination-feedback="true"
			data-feed-pagination-feedback-state={currentFeedback?.kind ?? "idle"}
		>
			<PaginationFeedbackChip
				feedback={currentFeedback}
				errorMessage={errorMessage}
				onLoadMore={onLoadMore}
				onRetry={onRetry}
				leaving={leaving}
			/>
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
	autoLoadMore?: boolean;
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
		autoLoadMore = true,
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
		if (
			!autoLoadMore ||
			!hasMore ||
			loadingInitial ||
			loadingMore ||
			error?.phase === "append"
		)
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
	}, [
		autoLoadMore,
		error?.phase,
		hasMore,
		loadingInitial,
		loadingMore,
		onLoadMore,
	]);

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
							<PaginationFeedback loading error={null} />
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
			<PaginationFeedback
				hasMore={hasMore}
				loading={loadingMore}
				error={error}
				onLoadMore={onLoadMore}
				onRetry={onRetry}
			/>
			{!hasMore && !error?.phase && sections.length > 0 ? (
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
