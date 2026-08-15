import { AlertCircle, LoaderCircle, RefreshCw } from "lucide-react";
import {
	type KeyboardEvent,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";

import type { AdminLlmActivityResponse } from "@/api";
import { Button } from "@/components/ui/button";

type LlmActivityGridProps = {
	data: AdminLlmActivityResponse | null;
	loading?: boolean;
	refreshing?: boolean;
	error?: string | null;
	onRetry?: () => void;
};

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

export function LlmActivityGrid({
	data,
	loading = false,
	refreshing = false,
	error = null,
	onRetry,
}: LlmActivityGridProps) {
	const rootRef = useRef<HTMLDivElement>(null);
	const cellRefs = useRef(new Map<string, HTMLButtonElement>());
	const [hoveredColumn, setHoveredColumn] = useState<number | null>(null);
	const [pinnedColumn, setPinnedColumn] = useState<number | null>(null);
	const activeColumn = pinnedColumn ?? hoveredColumn;

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

	useEffect(() => {
		if (pinnedColumn === null) return;
		const closeOutside = (event: MouseEvent) => {
			if (!rootRef.current?.contains(event.target as Node))
				setPinnedColumn(null);
		};
		document.addEventListener("mousedown", closeOutside);
		return () => document.removeEventListener("mousedown", closeOutside);
	}, [pinnedColumn]);

	useEffect(() => {
		if (activeColumn !== null && activeColumn >= (data?.buckets.length ?? 0)) {
			setHoveredColumn(null);
			setPinnedColumn(null);
		}
	}, [activeColumn, data]);

	const handleKeyDown = (
		event: KeyboardEvent<HTMLButtonElement>,
		model: string,
		column: number,
	) => {
		if (event.key === "Escape") {
			setPinnedColumn(null);
			setHoveredColumn(null);
			return;
		}
		if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
		event.preventDefault();
		const direction = event.key === "ArrowLeft" ? -1 : 1;
		const next = Math.min(
			(data?.buckets.length ?? 1) - 1,
			Math.max(0, column + direction),
		);
		setHoveredColumn(next);
		cellRefs.current.get(`${model}:${next}`)?.focus();
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

	const selectedBucket =
		activeColumn === null ? null : (data.buckets[activeColumn] ?? null);
	const bucketSucceeded =
		selectedBucket?.counts.reduce((sum, count) => sum + count.succeeded, 0) ??
		0;
	const gridTemplateColumns = `152px repeat(${data.bucket_count}, 16px)`;

	return (
		<div ref={rootRef} className="relative" data-testid="llm-activity-grid">
			<div className="mb-2 flex min-h-5 items-center justify-between gap-3">
				<p className="text-muted-foreground text-xs">
					最近 {data.bucket_count} 小时 · 本地时间
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
			<div className="overflow-x-auto pb-2">
				<div
					className="grid w-max gap-x-[3px] gap-y-1"
					style={{ gridTemplateColumns }}
				>
					<div className="bg-card sticky left-0 z-20" />
					{data.buckets.map((bucket, column) => (
						<div
							key={bucket.started_at}
							className="text-muted-foreground h-8 text-xs"
							title={localTime(bucket.started_at)}
						>
							{column % 6 === 0 || column === data.buckets.length - 1 ? (
								<span className="inline-block origin-bottom-left rotate-[-45deg] whitespace-nowrap">
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
							className="bg-card sticky left-0 z-10 flex h-4 items-center pr-3"
						>
							<span className="truncate font-mono text-xs" title={model.model}>
								{model.configured ? `${model.priority}. ` : ""}
								{model.model}
							</span>
						</div>,
						...data.buckets.map((bucket, column) => {
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
									className={`size-4 rounded-[3px] ring-1 transition-[filter,transform] hover:brightness-95 focus-visible:z-20 focus-visible:scale-125 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${activityClass(total, visibleMax)} ${activeColumn === column ? "brightness-90 ring-2 ring-foreground/45" : ""}`}
									aria-label={`${localTime(bucket.started_at)}，${model.model}，成功 ${count.succeeded}，失败 ${count.failed}`}
									onMouseEnter={() => setHoveredColumn(column)}
									onMouseLeave={() => setHoveredColumn(null)}
									onFocus={() => setHoveredColumn(column)}
									onBlur={() => setHoveredColumn(null)}
									onPointerDown={() => setPinnedColumn(column)}
									onClick={() => setPinnedColumn(column)}
									onKeyDown={(event) =>
										handleKeyDown(event, model.model, column)
									}
								/>
							);
						}),
					])}
				</div>
			</div>
			{selectedBucket ? (
				<div
					className="bg-popover text-popover-foreground absolute right-0 top-7 z-30 w-[min(28rem,calc(100vw-3rem))] rounded-md border p-3 shadow-lg"
					role="dialog"
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
				</div>
			) : null}
			{error ? (
				<p className="text-destructive mt-2 text-xs">更新失败：{error}</p>
			) : null}
		</div>
	);
}
