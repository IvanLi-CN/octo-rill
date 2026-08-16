import { AlertCircle, LoaderCircle, RefreshCw } from "lucide-react";
import {
	type CSSProperties,
	type KeyboardEvent,
	useCallback,
	useEffect,
	useLayoutEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";

import type { AdminLlmActivityResponse } from "@/api";
import { Button } from "@/components/ui/button";
import { useMediaQuery } from "@/lib/useMediaQuery";

type LlmActivityGridProps = {
	data: AdminLlmActivityResponse | null;
	loading?: boolean;
	refreshing?: boolean;
	error?: string | null;
	onRetry?: () => void;
};

type ActivityRect = {
	bottom: number;
	height: number;
	left: number;
	right: number;
	top: number;
	width: number;
};

type TooltipAnchor = {
	x: number;
	y: number;
};

type TooltipPosition = {
	left: number;
	top: number;
};

const ACTIVITY_SUMMARY_ID = "llm-activity-summary";
const MAX_BUCKET_COUNT = 50;
const TOOLTIP_GAP = 12;
const TOOLTIP_VIEWPORT_MARGIN = 12;

const percent = (numerator: number, denominator: number) =>
	denominator === 0 ? "--" : `${Math.round((100 * numerator) / denominator)}%`;

const localTime = (value: string) =>
	new Intl.DateTimeFormat(undefined, {
		month: "2-digit",
		day: "2-digit",
		hour: "2-digit",
		minute: "2-digit",
		hour12: false,
	}).format(new Date(value));

const activityClass = (count: number, maximum: number) => {
	if (count === 0 || maximum === 0) return "bg-muted/80 ring-border/50";
	const level = Math.ceil((4 * count) / maximum);
	return [
		"bg-sky-200 ring-sky-300/50 dark:bg-sky-950 dark:ring-sky-800/60",
		"bg-cyan-300 ring-cyan-400/50 dark:bg-cyan-800 dark:ring-cyan-700/70",
		"bg-emerald-400 ring-emerald-500/50 dark:bg-emerald-700 dark:ring-emerald-600/70",
		"bg-green-500 ring-green-600/50 dark:bg-green-500 dark:ring-green-400/70",
	][Math.max(0, level - 1)];
};

const toActivityRect = (rect: DOMRect): ActivityRect => ({
	bottom: rect.bottom,
	height: rect.height,
	left: rect.left,
	right: rect.right,
	top: rect.top,
	width: rect.width,
});

const containsPoint = (rect: ActivityRect, x: number, y: number) =>
	x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;

const overlaps = (
	left: number,
	top: number,
	width: number,
	height: number,
	rect: ActivityRect,
) =>
	left < rect.right &&
	left + width > rect.left &&
	top < rect.bottom &&
	top + height > rect.top;

const clamp = (value: number, minimum: number, maximum: number) =>
	Math.min(Math.max(value, minimum), maximum);

export function LlmActivityGrid({
	data,
	loading = false,
	refreshing = false,
	error = null,
	onRetry,
}: LlmActivityGridProps) {
	const rootRef = useRef<HTMLDivElement>(null);
	const gridSurfaceRef = useRef<HTMLDivElement>(null);
	const tooltipRef = useRef<HTMLDivElement>(null);
	const cellRefs = useRef(new Map<string, HTMLButtonElement>());
	const [hoveredColumn, setHoveredColumn] = useState<number | null>(null);
	const [pinnedColumn, setPinnedColumn] = useState<number | null>(null);
	const [tooltipAnchor, setTooltipAnchor] = useState<TooltipAnchor | null>(
		null,
	);
	const [tooltipPosition, setTooltipPosition] =
		useState<TooltipPosition | null>(null);
	const [gridSurfaceWidth, setGridSurfaceWidth] = useState(0);
	const isDesktop = useMediaQuery("(min-width: 1024px)");
	const isTablet = useMediaQuery("(min-width: 640px)");
	const isMobile = !isTablet;
	const activeColumn = pinnedColumn ?? hoveredColumn;
	const gridLabelWidth = isDesktop ? 152 : isTablet ? 112 : 28;
	const minimumCellSize = isDesktop ? 12 : isTablet ? 11 : 9;
	const gridGap = isMobile ? 2 : 3;
	const fallbackBucketCount = isDesktop ? MAX_BUCKET_COUNT : isTablet ? 36 : 25;
	const bucketCapacity =
		gridSurfaceWidth > 0
			? Math.floor(
					Math.max(0, gridSurfaceWidth - gridLabelWidth) /
						(minimumCellSize + gridGap),
				)
			: fallbackBucketCount;
	const visibleBucketCount = Math.min(
		data?.buckets.length ?? MAX_BUCKET_COUNT,
		Math.max(1, bucketCapacity),
	);

	useLayoutEffect(() => {
		const surface = gridSurfaceRef.current;
		if (!surface) return;
		const updateWidth = () => {
			const width = Math.round(surface.getBoundingClientRect().width);
			setGridSurfaceWidth((current) => (current === width ? current : width));
		};
		updateWidth();
		const observer = new ResizeObserver(updateWidth);
		observer.observe(surface);
		return () => observer.disconnect();
	}, [data]);

	const visibleMax = useMemo(
		() =>
			Math.max(
				0,
				...(data?.buckets.flatMap((bucket) =>
					bucket.counts.map((count) => count.succeeded + count.failed),
				) ?? []),
			),
		[data],
	);

	const setAnchorFromElement = useCallback((element: HTMLButtonElement) => {
		const rect = toActivityRect(element.getBoundingClientRect());
		setTooltipAnchor({
			x: rect.left + rect.width / 2,
			y: rect.top + rect.height / 2,
		});
	}, []);

	const closeActivityTooltip = useCallback(() => {
		setHoveredColumn(null);
		setPinnedColumn(null);
		setTooltipAnchor(null);
		setTooltipPosition(null);
	}, []);

	useEffect(() => {
		if (activeColumn === null) return;
		const closeOutside = (event: MouseEvent) => {
			if (
				pinnedColumn !== null &&
				!rootRef.current?.contains(event.target as Node)
			) {
				closeActivityTooltip();
			}
		};
		const closeOnEscape = (event: globalThis.KeyboardEvent) => {
			if (event.key === "Escape") closeActivityTooltip();
		};
		document.addEventListener("mousedown", closeOutside);
		document.addEventListener("keydown", closeOnEscape);
		return () => {
			document.removeEventListener("mousedown", closeOutside);
			document.removeEventListener("keydown", closeOnEscape);
		};
	}, [activeColumn, closeActivityTooltip, pinnedColumn]);

	useEffect(() => {
		if (pinnedColumn !== null || activeColumn === null) return;
		const closeWhenOutsideSafeZone = (event: PointerEvent) => {
			const gridSurface = gridSurfaceRef.current;
			const tooltip = tooltipRef.current;
			if (!gridSurface) return;
			const inGrid = containsPoint(
				toActivityRect(gridSurface.getBoundingClientRect()),
				event.clientX,
				event.clientY,
			);
			const inTooltip = tooltip
				? containsPoint(
						toActivityRect(tooltip.getBoundingClientRect()),
						event.clientX,
						event.clientY,
					)
				: false;
			if (!inGrid && !inTooltip) {
				setHoveredColumn(null);
				setTooltipAnchor(null);
				setTooltipPosition(null);
			}
		};
		document.addEventListener("pointermove", closeWhenOutsideSafeZone);
		return () =>
			document.removeEventListener("pointermove", closeWhenOutsideSafeZone);
	}, [activeColumn, pinnedColumn]);

	useEffect(() => {
		if (activeColumn !== null && activeColumn >= (data?.buckets.length ?? 0)) {
			closeActivityTooltip();
		}
	}, [activeColumn, closeActivityTooltip, data]);

	const selectedBucket =
		activeColumn === null ? null : (data?.buckets[activeColumn] ?? null);

	const updateTooltipPosition = useCallback(() => {
		const tooltip = tooltipRef.current;
		const gridSurface = gridSurfaceRef.current;
		if (!tooltip || !gridSurface || !tooltipAnchor) return;

		const tooltipRect = toActivityRect(tooltip.getBoundingClientRect());
		const gridRect = toActivityRect(gridSurface.getBoundingClientRect());
		const minimumLeft = TOOLTIP_VIEWPORT_MARGIN;
		const maximumLeft = Math.max(
			minimumLeft,
			window.innerWidth - tooltipRect.width - TOOLTIP_VIEWPORT_MARGIN,
		);
		const minimumTop = TOOLTIP_VIEWPORT_MARGIN;
		const maximumTop = Math.max(
			minimumTop,
			window.innerHeight - tooltipRect.height - TOOLTIP_VIEWPORT_MARGIN,
		);
		const candidates = [
			{
				left: tooltipAnchor.x - tooltipRect.width / 2,
				top: gridRect.bottom + TOOLTIP_GAP,
			},
			{
				left: tooltipAnchor.x - tooltipRect.width / 2,
				top: gridRect.top - tooltipRect.height - TOOLTIP_GAP,
			},
			{
				left: gridRect.right + TOOLTIP_GAP,
				top: tooltipAnchor.y - tooltipRect.height / 2,
			},
			{
				left: gridRect.left - tooltipRect.width - TOOLTIP_GAP,
				top: tooltipAnchor.y - tooltipRect.height / 2,
			},
		].map((candidate) => ({
			left: clamp(candidate.left, minimumLeft, maximumLeft),
			top: clamp(candidate.top, minimumTop, maximumTop),
		}));
		const position =
			candidates.find(
				(candidate) =>
					!overlaps(
						candidate.left,
						candidate.top,
						tooltipRect.width,
						tooltipRect.height,
						gridRect,
					),
			) ?? candidates[0];
		setTooltipPosition((current) =>
			current?.left === position.left && current.top === position.top
				? current
				: position,
		);
	}, [tooltipAnchor]);

	useLayoutEffect(() => {
		if (!selectedBucket || !tooltipAnchor) return;
		updateTooltipPosition();
		const update = () => window.requestAnimationFrame(updateTooltipPosition);
		window.addEventListener("resize", update);
		window.addEventListener("scroll", update, true);
		const observer = new ResizeObserver(update);
		if (tooltipRef.current) observer.observe(tooltipRef.current);
		if (gridSurfaceRef.current) observer.observe(gridSurfaceRef.current);
		return () => {
			window.removeEventListener("resize", update);
			window.removeEventListener("scroll", update, true);
			observer.disconnect();
		};
	}, [selectedBucket, tooltipAnchor, updateTooltipPosition]);

	const handleKeyDown = (
		event: KeyboardEvent<HTMLButtonElement>,
		model: string,
		column: number,
	) => {
		if (event.key === "Escape") {
			closeActivityTooltip();
			return;
		}
		if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
		event.preventDefault();
		const direction = event.key === "ArrowLeft" ? -1 : 1;
		const firstVisibleColumn = Math.max(
			0,
			(data?.buckets.length ?? 0) - visibleBucketCount,
		);
		const next = Math.min(
			(data?.buckets.length ?? 1) - 1,
			Math.max(firstVisibleColumn, column + direction),
		);
		setHoveredColumn(next);
		setPinnedColumn((current) => (current === null ? null : next));
		const nextCell = cellRefs.current.get(`${model}:${next}`);
		if (nextCell) {
			nextCell.focus();
			setAnchorFromElement(nextCell);
		}
	};

	if (!data && loading) {
		return (
			<div
				className="text-muted-foreground flex min-h-44 items-center justify-center gap-2 rounded-md border border-dashed text-sm"
				role="status"
			>
				<LoaderCircle className="size-4 animate-spin" />
				正在加载模型活动
			</div>
		);
	}

	if (!data && error) {
		return (
			<div className="flex min-h-44 flex-col items-center justify-center gap-3 rounded-md border border-dashed p-6 text-center">
				<AlertCircle className="text-destructive size-5" />
				<p className="text-muted-foreground max-w-md text-sm">{error}</p>
				{onRetry ? (
					<Button type="button" variant="outline" size="sm" onClick={onRetry}>
						<RefreshCw />
						重试
					</Button>
				) : null}
			</div>
		);
	}

	if (!data) return null;

	const bucketSucceeded =
		selectedBucket?.counts.reduce((sum, count) => sum + count.succeeded, 0) ??
		0;
	const visibleBucketStart = Math.max(
		0,
		data.buckets.length - visibleBucketCount,
	);
	const visibleBuckets = data.buckets.slice(visibleBucketStart);
	const gridStyle = {
		gridTemplateColumns: `${gridLabelWidth}px repeat(${visibleBuckets.length}, minmax(0, 1fr))`,
	} as CSSProperties;

	const tooltip = selectedBucket
		? createPortal(
				<div
					ref={tooltipRef}
					id={ACTIVITY_SUMMARY_ID}
					className="bg-popover text-popover-foreground pointer-events-none fixed z-50 w-[min(28rem,calc(100vw-1.5rem))] rounded-md border p-3 shadow-lg"
					style={
						tooltipPosition
							? { left: tooltipPosition.left, top: tooltipPosition.top }
							: { left: 0, top: 0, visibility: "hidden" }
					}
					role="tooltip"
					aria-live="polite"
					aria-atomic="true"
					aria-label={`${localTime(selectedBucket.started_at)} 模型活动`}
					data-testid="llm-activity-summary"
				>
					<p className="text-sm font-medium">
						{localTime(selectedBucket.started_at)}
					</p>
					<div className="mt-2 grid grid-cols-[minmax(0,1fr)_auto_auto_auto_auto] gap-x-3 gap-y-1 text-xs">
						<span className="text-muted-foreground">模型</span>
						<span className="text-muted-foreground text-right">成功</span>
						<span className="text-muted-foreground text-right">失败</span>
						<span className="text-muted-foreground text-right">成功率</span>
						<span className="text-muted-foreground text-right">使用率</span>
						{selectedBucket.counts.map((count) => (
							<div key={count.model} className="contents">
								<span className="truncate font-mono" title={count.model}>
									{count.model}
								</span>
								<span className="text-right tabular-nums">
									{count.succeeded}
								</span>
								<span className="text-right tabular-nums">{count.failed}</span>
								<span className="text-right tabular-nums">
									{percent(count.succeeded, count.succeeded + count.failed)}
								</span>
								<span className="text-right tabular-nums">
									{percent(count.succeeded, bucketSucceeded)}
								</span>
							</div>
						))}
					</div>
				</div>,
				document.body,
			)
		: null;

	return (
		<div ref={rootRef} className="min-w-0" data-testid="llm-activity-grid">
			<div className="mb-2 flex min-h-8 items-center justify-between gap-3">
				<p className="text-muted-foreground text-xs">
					最近 {visibleBuckets.length} 小时 · 本地时间
				</p>
				{refreshing ? (
					<span
						className="text-muted-foreground inline-flex items-center gap-1 text-xs"
						role="status"
					>
						<LoaderCircle className="size-3 animate-spin" />
						更新中
					</span>
				) : null}
			</div>
			{isMobile && visibleBuckets.length > 0 ? (
				<div
					className="text-muted-foreground mb-2 flex items-center justify-between font-mono text-[10px] tabular-nums"
					data-testid="llm-activity-mobile-range"
				>
					<span>{localTime(visibleBuckets[0].started_at)}</span>
					<span aria-hidden="true">至</span>
					<span>
						{localTime(visibleBuckets[visibleBuckets.length - 1].ended_at)}
					</span>
				</div>
			) : null}
			<div
				ref={gridSurfaceRef}
				className="min-w-0"
				data-testid="llm-activity-surface"
			>
				<div
					className={`grid w-full gap-y-1 ${isMobile ? "gap-x-0.5" : "gap-x-[3px]"}`}
					style={gridStyle}
				>
					<div className="bg-card sticky left-0 z-20 h-0 sm:h-auto" />
					{visibleBuckets.map((bucket, index) => (
						<div
							key={bucket.started_at}
							className="text-muted-foreground relative h-0 text-xs sm:h-8"
							title={localTime(bucket.started_at)}
						>
							{index % 6 === 0 || index === visibleBuckets.length - 1 ? (
								<span
									className={`absolute bottom-0 hidden rotate-[-45deg] whitespace-nowrap sm:inline-block ${
										index === visibleBuckets.length - 1
											? "right-0 origin-bottom-right"
											: "left-0 origin-bottom-left"
									}`}
								>
									{localTime(bucket.started_at).slice(0, 5)}
								</span>
							) : null}
						</div>
					))}
					{data.models.length === 0 ? (
						<div className="text-muted-foreground col-span-full py-10 text-center text-sm">
							窗口内暂无模型活动
						</div>
					) : null}
					{data.models.map((model) => [
						<div
							key={`${model.model}:label`}
							className="bg-card sticky left-0 z-10 flex min-w-0 items-center pr-1 sm:h-4 sm:pr-2"
						>
							{isMobile ? (
								<span
									className="text-muted-foreground w-full text-center font-mono text-[10px] tabular-nums"
									aria-hidden="true"
								>
									{model.priority || "·"}
								</span>
							) : (
								<span
									className="truncate font-mono text-xs"
									title={model.model}
								>
									{model.configured ? `${model.priority}. ` : ""}
									{model.model}
								</span>
							)}
							<span className="sr-only">{model.model}</span>
						</div>,
						...visibleBuckets.map((bucket, visibleIndex) => {
							const column = visibleBucketStart + visibleIndex;
							const count = bucket.counts.find(
								(item) => item.model === model.model,
							) ?? {
								succeeded: 0,
								failed: 0,
							};
							const total = count.succeeded + count.failed;
							const key = `${model.model}:${column}`;
							return (
								<button
									key={key}
									ref={(node) => {
										if (node) cellRefs.current.set(key, node);
										else cellRefs.current.delete(key);
									}}
									type="button"
									className={`aspect-square w-full min-w-0 rounded-[2px] ring-1 transition-[filter] hover:brightness-95 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring sm:rounded-[3px] ${activityClass(total, visibleMax)} ${activeColumn === column ? "brightness-90 ring-1 ring-foreground/60" : ""}`}
									aria-label={`${localTime(bucket.started_at)}，${model.model}，成功 ${count.succeeded}，失败 ${count.failed}`}
									aria-controls={ACTIVITY_SUMMARY_ID}
									aria-describedby={
										activeColumn === column ? ACTIVITY_SUMMARY_ID : undefined
									}
									aria-expanded={activeColumn === column}
									onPointerMove={(event) => {
										if (pinnedColumn !== null) return;
										setHoveredColumn(column);
										setTooltipAnchor({ x: event.clientX, y: event.clientY });
									}}
									onFocus={(event) => {
										if (pinnedColumn === null) setHoveredColumn(column);
										setAnchorFromElement(event.currentTarget);
									}}
									onBlur={() => {
										if (pinnedColumn === null) setHoveredColumn(null);
									}}
									onPointerDown={(event) => {
										setHoveredColumn(column);
										setPinnedColumn(column);
										setTooltipAnchor({ x: event.clientX, y: event.clientY });
									}}
									onClick={(event) => {
										setPinnedColumn(column);
										setAnchorFromElement(event.currentTarget);
									}}
									onKeyDown={(event) =>
										handleKeyDown(event, model.model, column)
									}
								/>
							);
						}),
					])}
				</div>
			</div>
			{isMobile && data.models.length > 0 ? (
				<ul
					className="mt-3 grid gap-x-4 gap-y-1 text-xs sm:hidden"
					aria-label="模型图例"
				>
					{data.models.map((model) => (
						<li key={model.model} className="flex min-w-0 items-center gap-2">
							<span className="bg-muted text-muted-foreground inline-flex size-5 shrink-0 items-center justify-center rounded font-mono text-[10px] tabular-nums">
								{model.priority || "·"}
							</span>
							<span className="truncate font-mono">{model.model}</span>
						</li>
					))}
				</ul>
			) : null}
			{error ? (
				<p className="text-destructive mt-2 text-xs">更新失败：{error}</p>
			) : null}
			{tooltip}
		</div>
	);
}
