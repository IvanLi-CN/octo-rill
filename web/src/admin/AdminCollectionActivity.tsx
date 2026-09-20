import {
	AlertCircle,
	Check,
	Files,
	LoaderCircle,
	RefreshCw,
} from "lucide-react";
import {
	type KeyboardEvent,
	useCallback,
	useEffect,
	useLayoutEffect,
	memo,
	useMemo,
	useRef,
	useState,
} from "react";

import type {
	AdminCollectionActivityBucket,
	AdminCollectionActivityCell,
	AdminCollectionActivityResponse,
	AdminCollectionActivityStatus,
	AdminCollectionRecordItem,
} from "@/api";
import { Button } from "@/components/ui/button";

const DOM_CELL_LIMIT = 8_000;
const CELL_SIZE = 24;
const CELL_GAP = 3;
const HOUR_LABEL_WIDTH = 70;
const VIEWPORT_HEIGHT = 320;
const CELL_STEP = CELL_SIZE + CELL_GAP;

const STATUS_LABELS: Record<AdminCollectionActivityStatus, string> = {
	completed: "已完成",
	processing: "处理中",
	exception: "异常",
	neutral: "中性",
};

const PIPELINE_STATUS_LABELS: Record<string, string> = {
	not_started: "未开始",
	queued: "排队中",
	running: "运行中",
	succeeded: "已完成",
	failed: "失败",
	missing: "缺失",
	disabled: "已禁用",
	historical_unknown: "历史状态未知",
	legacy_cached: "旧缓存",
	legacy_conflict: "旧数据冲突",
	deferred_provider: "等待服务恢复",
	blocked_config: "配置阻塞",
	cancelled: "已取消",
	superseded: "已替代",
	not_applicable: "不适用",
};

const STATUS_CLASS: Record<AdminCollectionActivityStatus, string> = {
	completed: "bg-emerald-500 ring-emerald-600/30",
	processing: "bg-amber-400 ring-amber-500/40",
	exception: "bg-destructive ring-destructive/40",
	neutral: "bg-muted-foreground/45 ring-muted-foreground/25",
};

const STATUS_COLOR: Record<AdminCollectionActivityStatus, string> = {
	completed: "#22c55e",
	processing: "#f59e0b",
	exception: "#dc2626",
	neutral: "#9ca3af",
};

function formatTime(value: string) {
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return value;
	return new Intl.DateTimeFormat("zh-CN", {
		month: "2-digit",
		day: "2-digit",
		hour: "2-digit",
		minute: "2-digit",
		hour12: false,
	}).format(date);
}

function formatHour(value: string) {
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return value;
	return new Intl.DateTimeFormat("zh-CN", {
		hour: "2-digit",
		hour12: false,
	}).format(date);
}

function cellLabel(cell: AdminCollectionActivityCell) {
	const repository = cell.repository ? `${cell.repository}，` : "";
	const translation = cell.translation_status
		? `，翻译 ${PIPELINE_STATUS_LABELS[cell.translation_status] ?? cell.translation_status}`
		: "";
	return `${cell.title}，${repository}来源时间 ${formatTime(cell.source_time)}，综合状态 ${STATUS_LABELS[cell.composite_status]}${translation}，润色 ${PIPELINE_STATUS_LABELS[cell.polish_status] ?? cell.polish_status}`;
}

function StatusLegend({ neutralCount }: { neutralCount: number }) {
	return (
		<ul className="flex flex-wrap items-center gap-x-4 gap-y-2 text-xs text-muted-foreground">
			{(["completed", "processing", "exception"] as const).map((status) => (
				<li key={status} className="inline-flex items-center gap-1.5">
					<span
						className={`size-2.5 rounded-[2px] ring-1 ${STATUS_CLASS[status]}`}
						aria-hidden="true"
					/>
					<span>{STATUS_LABELS[status]}</span>
				</li>
			))}
			<li className="inline-flex items-center gap-1.5">
				<span
					className={`size-2.5 rounded-[2px] ring-1 ${STATUS_CLASS.neutral}`}
					aria-hidden="true"
				/>
				<span>中性 {neutralCount}</span>
			</li>
		</ul>
	);
}

function SummaryMetrics({ data }: { data: AdminCollectionActivityResponse }) {
	const metrics = [
		{ label: "内容数", value: data.summary.content_count, icon: Files },
		{ label: "已完成", value: data.summary.completed_count, icon: Check },
		{
			label: "处理中",
			value: data.summary.processing_count,
			icon: LoaderCircle,
		},
		{ label: "异常", value: data.summary.exception_count, icon: AlertCircle },
	] as const;
	return (
		<dl className="grid grid-cols-2 gap-x-6 gap-y-3 lg:grid-cols-4">
			{metrics.map(({ label, value, icon: Icon }) => (
				<div key={label} className="min-w-0 border-l-2 border-border pl-3">
					<dt className="flex items-center gap-1.5 text-xs text-muted-foreground">
						<Icon className="size-3.5" aria-hidden="true" />
						{label}
					</dt>
					<dd className="mt-1 text-lg font-semibold tabular-nums text-foreground">
						{value.toLocaleString("zh-CN")}
					</dd>
				</div>
			))}
		</dl>
	);
}

function ActivityDetails({
	cell,
}: {
	cell: AdminCollectionActivityCell | null;
}) {
	if (!cell) return <div className="min-h-5" aria-live="polite" />;
	return (
		<div
			id="collection-activity-details"
			className="min-w-0 border-l-2 border-border pl-3 text-xs"
			role="tooltip"
			aria-live="polite"
		>
			<p className="break-words font-medium text-foreground">{cell.title}</p>
			<p className="mt-0.5 break-words text-muted-foreground">
				{cell.repository ? `${cell.repository} · ` : ""}
				{formatTime(cell.source_time)} · {STATUS_LABELS[cell.composite_status]}{" "}
				· 润色{" "}
				{PIPELINE_STATUS_LABELS[cell.polish_status] ?? cell.polish_status}
				{cell.translation_status
					? ` · 翻译 ${PIPELINE_STATUS_LABELS[cell.translation_status] ?? cell.translation_status}`
					: ""}
			</p>
		</div>
	);
}

const DomActivityRows = memo(function DomActivityRows({
	buckets,
	kind,
	onOpenRecord,
}: {
	buckets: AdminCollectionActivityBucket[];
	kind: AdminCollectionRecordItem["kind"];
	onOpenRecord: (kind: AdminCollectionRecordItem["kind"], id: string) => void;
}) {
	const [activeCell, setActiveCell] =
		useState<AdminCollectionActivityCell | null>(null);
	return (
		<div className="space-y-2" data-testid="collection-activity-dom-grid">
			<div className="max-h-80 overflow-y-auto overscroll-contain pr-1">
				{buckets.map((bucket) => (
					<div
						key={bucket.started_at}
						className="grid grid-cols-[3.5rem_minmax(0,1fr)] gap-2 border-t border-border/70 py-1 first:border-t-0"
					>
						<time
							dateTime={bucket.started_at}
							title={`${formatTime(bucket.started_at)}–${formatTime(bucket.ended_at)}`}
							className="pt-0.5 font-mono text-xs tabular-nums text-muted-foreground"
						>
							{formatHour(bucket.started_at)}
						</time>
						<div
							className="grid min-h-6 content-start justify-start gap-[3px]"
							style={{
								gridTemplateColumns: `repeat(auto-fill, ${CELL_SIZE}px)`,
							}}
						>
							{bucket.cells.map((cell) => (
								<button
									key={`${cell.id}:${cell.source_time}`}
									type="button"
									className={`size-6 rounded-[2px] ring-1 transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring ${STATUS_CLASS[cell.composite_status]}`}
									aria-label={cellLabel(cell)}
									aria-describedby={
										activeCell === cell
											? "collection-activity-details"
											: undefined
									}
									title={cellLabel(cell)}
									data-activity-status={cell.composite_status}
									onPointerEnter={() => setActiveCell(cell)}
									onFocus={() => setActiveCell(cell)}
									onPointerLeave={(event) => {
										if (document.activeElement !== event.currentTarget)
											setActiveCell(null);
									}}
									onBlur={(event) => {
										if (!event.currentTarget.matches(":hover"))
											setActiveCell(null);
									}}
									onClick={() => onOpenRecord(kind, cell.id)}
								/>
							))}
							{bucket.cells.length === 0 ? (
								<span
									className="col-span-full text-xs text-muted-foreground/70"
									aria-hidden="true"
								>
									·
								</span>
							) : null}
						</div>
					</div>
				))}
			</div>
			<ActivityDetails cell={activeCell} />
		</div>
	);
});

type CanvasBucketLayout = {
	start: number;
	top: number;
	headerHeight: number;
	height: number;
	lineCount: number;
};

function verticalNeighborIndex(
	buckets: AdminCollectionActivityBucket[],
	layouts: CanvasBucketLayout[],
	columns: number,
	currentIndex: number,
	currentBucketIndex: number,
	direction: -1 | 1,
) {
	const currentLayout = layouts[currentBucketIndex];
	const localIndex = currentIndex - currentLayout.start;
	const currentLine = Math.floor(localIndex / columns);
	const column = localIndex % columns;
	let bucketIndex = currentBucketIndex;
	let line = currentLine + direction;

	while (bucketIndex >= 0 && bucketIndex < buckets.length) {
		const bucket = buckets[bucketIndex];
		const layout = layouts[bucketIndex];
		if (bucket.cells.length > 0 && line >= 0 && line < layout.lineCount) {
			const lineStart = line * columns;
			const cellsInLine = Math.min(columns, bucket.cells.length - lineStart);
			return layout.start + lineStart + Math.min(column, cellsInLine - 1);
		}

		bucketIndex += direction;
		if (bucketIndex >= 0 && bucketIndex < buckets.length) {
			line = direction < 0 ? layouts[bucketIndex].lineCount - 1 : 0;
		}
	}

	return currentIndex;
}

const CanvasActivityGrid = memo(function CanvasActivityGrid({
	buckets,
	kind,
	onOpenRecord,
}: {
	buckets: AdminCollectionActivityBucket[];
	kind: AdminCollectionRecordItem["kind"];
	onOpenRecord: (kind: AdminCollectionRecordItem["kind"], id: string) => void;
}) {
	const scrollRef = useRef<HTMLDivElement>(null);
	const canvasRef = useRef<HTMLCanvasElement>(null);
	const scrollFrameRef = useRef<number | null>(null);
	const [width, setWidth] = useState(0);
	const [scrollTop, setScrollTop] = useState(0);
	const [activeIndex, setActiveIndex] = useState<number | null>(null);
	const cells = useMemo(
		() =>
			buckets.flatMap((bucket, bucketIndex) =>
				bucket.cells.map((cell) => ({ bucketIndex, cell })),
			),
		[buckets],
	);
	const columns = Math.max(
		1,
		Math.floor((width - HOUR_LABEL_WIDTH - 8) / CELL_STEP),
	);
	const layouts = useMemo(() => {
		let top = 0;
		let start = 0;
		return buckets.map((bucket): CanvasBucketLayout => {
			const lineCount = Math.max(1, Math.ceil(bucket.cells.length / columns));
			const headerHeight = 23;
			const height = headerHeight + lineCount * CELL_STEP + 8;
			const layout = { start, top, headerHeight, height, lineCount };
			top += height;
			start += bucket.cells.length;
			return layout;
		});
	}, [buckets, columns]);
	const contentHeight = Math.max(
		VIEWPORT_HEIGHT,
		layouts.reduce((height, layout) => height + layout.height, 0),
	);
	const active = activeIndex === null ? null : (cells[activeIndex] ?? null);
	const accessibleCellId = active
		? `collection-activity-grid-cell-${activeIndex}`
		: undefined;

	useLayoutEffect(() => {
		const element = scrollRef.current;
		if (!element) return;
		const observer = new ResizeObserver(() => {
			setWidth(Math.round(element.getBoundingClientRect().width));
		});
		observer.observe(element);
		setWidth(Math.round(element.getBoundingClientRect().width));
		return () => observer.disconnect();
	}, []);

	useEffect(
		() => () => {
			if (scrollFrameRef.current !== null)
				cancelAnimationFrame(scrollFrameRef.current);
		},
		[],
	);

	useEffect(() => {
		const canvas = canvasRef.current;
		if (!canvas || width <= 0) return;
		const dpr = window.devicePixelRatio || 1;
		canvas.width = Math.round(width * dpr);
		canvas.height = Math.round(VIEWPORT_HEIGHT * dpr);
		canvas.style.width = `${width}px`;
		canvas.style.height = `${VIEWPORT_HEIGHT}px`;
		const context = canvas.getContext("2d");
		if (!context) return;
		context.setTransform(dpr, 0, 0, dpr, 0, 0);
		context.clearRect(0, 0, width, VIEWPORT_HEIGHT);
		context.font = "12px ui-monospace, SFMono-Regular, Menlo, monospace";
		context.textBaseline = "top";
		context.fillStyle = getComputedStyle(canvas).color;
		context.strokeStyle = "rgba(120, 113, 108, 0.22)";
		context.lineWidth = 1;

		for (let bucketIndex = 0; bucketIndex < buckets.length; bucketIndex += 1) {
			const bucket = buckets[bucketIndex];
			const layout = layouts[bucketIndex];
			const rowTop = layout.top - scrollTop;
			if (rowTop + layout.height < 0 || rowTop > VIEWPORT_HEIGHT) continue;
			context.beginPath();
			context.moveTo(0, rowTop + layout.height);
			context.lineTo(width, rowTop + layout.height);
			context.stroke();
			context.fillStyle = getComputedStyle(canvas).color;
			context.fillText(formatHour(bucket.started_at), 4, rowTop + 5);

			const firstLine = Math.max(
				0,
				Math.floor((scrollTop - layout.top - layout.headerHeight) / CELL_STEP),
			);
			const lastLine = Math.min(
				layout.lineCount - 1,
				Math.ceil(
					(scrollTop + VIEWPORT_HEIGHT - layout.top - layout.headerHeight) /
						CELL_STEP,
				),
			);
			for (let line = firstLine; line <= lastLine; line += 1) {
				const start = line * columns;
				const end = Math.min(bucket.cells.length, start + columns);
				for (let cellIndex = start; cellIndex < end; cellIndex += 1) {
					const cell = bucket.cells[cellIndex];
					const globalIndex = layout.start + cellIndex;
					const x = HOUR_LABEL_WIDTH + (cellIndex % columns) * CELL_STEP;
					const y = rowTop + layout.headerHeight + line * CELL_STEP;
					context.fillStyle = STATUS_COLOR[cell.composite_status];
					context.fillRect(x, y, CELL_SIZE, CELL_SIZE);
					context.strokeStyle =
						activeIndex === globalIndex ? "#111827" : "rgba(31, 41, 55, 0.22)";
					context.strokeRect(x + 0.5, y + 0.5, CELL_SIZE - 1, CELL_SIZE - 1);
				}
			}
		}
	}, [activeIndex, buckets, columns, layouts, scrollTop, width]);

	const updateScrollPosition = useCallback(() => {
		if (scrollFrameRef.current !== null)
			cancelAnimationFrame(scrollFrameRef.current);
		scrollFrameRef.current = requestAnimationFrame(() => {
			setScrollTop(scrollRef.current?.scrollTop ?? 0);
			scrollFrameRef.current = null;
		});
	}, []);

	const hitTest = useCallback(
		(clientX: number, clientY: number) => {
			const canvas = canvasRef.current;
			const scroll = scrollRef.current;
			if (!canvas || !scroll) return null;
			const rect = canvas.getBoundingClientRect();
			const x = clientX - rect.left;
			const y = clientY - rect.top + scroll.scrollTop;
			const bucketIndex = layouts.findIndex(
				(layout) => y >= layout.top && y < layout.top + layout.height,
			);
			if (bucketIndex < 0 || x < HOUR_LABEL_WIDTH) return null;
			const layout = layouts[bucketIndex];
			const line = Math.floor(
				(y - layout.top - layout.headerHeight) / CELL_STEP,
			);
			const column = Math.floor((x - HOUR_LABEL_WIDTH) / CELL_STEP);
			if (line < 0 || column < 0 || column >= columns) return null;
			const localX = (x - HOUR_LABEL_WIDTH) % CELL_STEP;
			const localY = (y - layout.top - layout.headerHeight) % CELL_STEP;
			if (localX >= CELL_SIZE || localY >= CELL_SIZE) return null;
			const cellIndex = line * columns + column;
			if (cellIndex >= buckets[bucketIndex].cells.length) return null;
			return layout.start + cellIndex;
		},
		[buckets, columns, layouts],
	);

	const onGridKeyDown = useCallback(
		(event: KeyboardEvent<HTMLDivElement>) => {
			if (cells.length === 0) return;
			const current = Math.min(activeIndex ?? 0, cells.length - 1);
			let next: number | null = null;
			switch (event.key) {
				case "ArrowLeft":
					next = Math.max(0, current - 1);
					break;
				case "ArrowRight":
					next = Math.min(cells.length - 1, current + 1);
					break;
				case "ArrowUp":
					next = verticalNeighborIndex(
						buckets,
						layouts,
						columns,
						current,
						cells[current]?.bucketIndex ?? 0,
						-1,
					);
					break;
				case "ArrowDown":
					next = verticalNeighborIndex(
						buckets,
						layouts,
						columns,
						current,
						cells[current]?.bucketIndex ?? 0,
						1,
					);
					break;
				case "Home":
					next = 0;
					break;
				case "End":
					next = cells.length - 1;
					break;
				case "Enter":
					if (cells[current]) onOpenRecord(kind, cells[current].cell.id);
					event.preventDefault();
					return;
				case "Escape":
					setActiveIndex(null);
					event.preventDefault();
					return;
				default:
					return;
			}
			event.preventDefault();
			setActiveIndex(next);
		},
		[activeIndex, buckets, cells, columns, kind, layouts, onOpenRecord],
	);

	useEffect(() => {
		if (activeIndex === null || !active) return;
		const layout = layouts[active.bucketIndex];
		const localIndex = activeIndex - layout.start;
		const cellTop =
			layout.top +
			layout.headerHeight +
			Math.floor(localIndex / columns) * CELL_STEP;
		const scroll = scrollRef.current;
		if (!scroll) return;
		if (cellTop < scroll.scrollTop) scroll.scrollTop = cellTop;
		else if (cellTop + CELL_SIZE > scroll.scrollTop + VIEWPORT_HEIGHT) {
			scroll.scrollTop = cellTop + CELL_SIZE - VIEWPORT_HEIGHT;
		}
		updateScrollPosition();
	}, [active, activeIndex, buckets, columns, layouts, updateScrollPosition]);

	return (
		<div className="space-y-2" data-testid="collection-activity-canvas-grid">
			<div
				ref={scrollRef}
				className="max-h-80 overflow-auto overscroll-contain rounded-sm focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-ring"
				onScroll={updateScrollPosition}
			>
				{/* biome-ignore lint/a11y/useSemanticElements: The virtualized canvas exposes its single active cell through ARIA grid semantics. */}
				<div
					role="grid"
					tabIndex={0}
					aria-label="最近十二小时内容活动"
					aria-activedescendant={accessibleCellId}
					onKeyDown={onGridKeyDown}
					className="relative"
					style={{ height: contentHeight }}
				>
					<div
						aria-hidden="true"
						className="sticky top-0"
						style={{ height: VIEWPORT_HEIGHT, marginBottom: -VIEWPORT_HEIGHT }}
					>
						<canvas
							ref={canvasRef}
							className="block text-muted-foreground"
							onPointerMove={(event) => {
								const index = hitTest(event.clientX, event.clientY);
								if (index !== null) {
									setActiveIndex((current) =>
										current === index ? current : index,
									);
								}
							}}
							onPointerLeave={(event) => {
								if (event.pointerType !== "touch") setActiveIndex(null);
							}}
							onClick={(event) => {
								const index = hitTest(event.clientX, event.clientY);
								if (index === null) return;
								setActiveIndex(index);
								onOpenRecord(kind, cells[index].cell.id);
							}}
						/>
					</div>
					{active ? (
						<>
							{/* biome-ignore lint/a11y/useSemanticElements: This virtual row holds only the active canvas cell. */}
							<div role="row" tabIndex={-1} className="sr-only">
								{/* biome-ignore lint/a11y/useSemanticElements: A virtual canvas cell must expose gridcell semantics. */}
								<div
									role="gridcell"
									tabIndex={-1}
									id={accessibleCellId}
									aria-label={cellLabel(active.cell)}
									aria-describedby="collection-activity-details"
								/>
							</div>
						</>
					) : null}
				</div>
			</div>
			<ActivityDetails cell={active?.cell ?? null} />
		</div>
	);
});

export const AdminCollectionActivity = memo(function AdminCollectionActivity({
	data,
	loading = false,
	error = null,
	onRetry,
	onOpenRecord,
}: {
	data: AdminCollectionActivityResponse | null;
	loading?: boolean;
	error?: string | null;
	onRetry: () => void;
	onOpenRecord: (kind: AdminCollectionRecordItem["kind"], id: string) => void;
}) {
	if (loading && !data) {
		return (
			<section
				aria-label="内容处理活动"
				aria-busy="true"
				className="space-y-4 border-y py-4"
			>
				<div className="grid grid-cols-2 gap-x-6 gap-y-3 lg:grid-cols-4">
					{[0, 1, 2, 3].map((item) => (
						<div
							key={item}
							className="min-h-12 animate-pulse border-l-2 border-border pl-3"
						/>
					))}
				</div>
				<p className="text-xs text-muted-foreground">正在读取活动数据...</p>
			</section>
		);
	}
	if (error && !data) {
		return (
			<section aria-label="内容处理活动" className="space-y-3 border-y py-4">
				<div className="flex flex-wrap items-start justify-between gap-3">
					<div className="min-w-0">
						<h3 className="font-medium text-sm">最近 12 小时活动暂不可用</h3>
						<p className="mt-1 text-xs text-muted-foreground">{error}</p>
					</div>
					<Button type="button" variant="outline" size="sm" onClick={onRetry}>
						<RefreshCw className="size-4" aria-hidden="true" />
						重试
					</Button>
				</div>
			</section>
		);
	}
	if (!data) return null;

	const cellCount = data.summary.content_count;
	const activeCellCount = data.buckets.reduce(
		(sum, bucket) => sum + bucket.cells.length,
		0,
	);
	return (
		<section
			aria-label="内容处理活动"
			data-activity-kind={data.kind}
			className="space-y-4 border-y py-4"
		>
			{error ? (
				<div
					className="flex flex-wrap items-center justify-between gap-3 border-l-2 border-destructive pl-3 text-xs"
					role="status"
				>
					<p className="min-w-0 break-words text-muted-foreground">
						活动更新失败，继续显示上次读取的数据。{error}
					</p>
					<Button type="button" variant="outline" size="sm" onClick={onRetry}>
						<RefreshCw className="size-4" aria-hidden="true" />
						重试
					</Button>
				</div>
			) : null}
			<SummaryMetrics data={data} />
			<div className="flex flex-col gap-3 border-t border-border/70 pt-3 sm:flex-row sm:items-end sm:justify-between">
				<div className="min-w-0">
					<div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
						<h3 className="font-medium text-sm">最近 12 小时</h3>
						{loading ? (
							<span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
								<LoaderCircle
									className="size-3.5 animate-spin"
									aria-hidden="true"
								/>
								正在更新
							</span>
						) : null}
						<span className="text-xs tabular-nums text-muted-foreground">
							{formatTime(data.window_started_at)} –{" "}
							{formatTime(data.window_ended_at)}
						</span>
					</div>
				</div>
				<StatusLegend neutralCount={data.summary.neutral_count} />
			</div>
			{cellCount > DOM_CELL_LIMIT ? (
				<CanvasActivityGrid
					buckets={data.buckets}
					kind={data.kind}
					onOpenRecord={onOpenRecord}
				/>
			) : (
				<DomActivityRows
					buckets={data.buckets}
					kind={data.kind}
					onOpenRecord={onOpenRecord}
				/>
			)}
			{activeCellCount !== cellCount ? (
				<p className="text-xs text-destructive">
					活动记录数量不一致，当前图表未显示。
				</p>
			) : null}
		</section>
	);
});
