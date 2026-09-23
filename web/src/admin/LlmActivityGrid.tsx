import { AlertCircle, LoaderCircle, RefreshCw } from "lucide-react";
import {
	memo,
	type KeyboardEvent as ReactKeyboardEvent,
	type ReactElement,
	useMemo,
} from "react";

import type { AdminLlmActivityResponse } from "@/api";
import {
	ActivityGrid,
	type ActivityGridCell,
	type ActivityGridMatrixColumn,
	type ActivityGridMatrixRow,
	type ActivityGridModel,
} from "@/admin/ActivityGrid";
import {
	LlmCallActionsMenu,
	LlmCallContextMenu,
	type LlmCallDrilldown,
} from "@/admin/LlmCallContextMenu";
import { Button } from "@/components/ui/button";

type LlmActivityGridProps = {
	data: AdminLlmActivityResponse | null;
	loading?: boolean;
	refreshing?: boolean;
	error?: string | null;
	onRetry?: () => void;
	onOpenCalls?: (
		target: LlmCallDrilldown & { status: "all" | "failed" },
	) => void;
};

const localTime = (value: string) =>
	new Intl.DateTimeFormat(undefined, {
		month: "2-digit",
		day: "2-digit",
		hour: "2-digit",
		minute: "2-digit",
		hour12: false,
	}).format(new Date(value));

type ActivityOutcome = "idle" | "healthy" | "degraded" | "failed";

function activityOutcome(succeeded: number, failed: number): ActivityOutcome {
	const total = succeeded + failed;
	if (total === 0) return "idle";
	if (failed === total) return "failed";
	if (failed > 0) return "degraded";
	return "healthy";
}

function activityClass(succeeded: number, failed: number, maximum: number) {
	const outcome = activityOutcome(succeeded, failed);
	if (outcome === "idle") return "bg-muted/80 ring-border/50";
	if (outcome === "failed") return "bg-destructive ring-destructive/70";
	if (outcome === "degraded")
		return "bg-amber-400 ring-amber-500/70 dark:bg-amber-500";
	const level = Math.ceil((4 * (succeeded + failed)) / Math.max(1, maximum));
	return [
		"bg-sky-200 ring-sky-300/50 dark:bg-sky-950 dark:ring-sky-800/60",
		"bg-cyan-300 ring-cyan-400/50 dark:bg-cyan-800 dark:ring-cyan-700/70",
		"bg-emerald-400 ring-emerald-500/50 dark:bg-emerald-700 dark:ring-emerald-600/70",
		"bg-green-500 ring-green-600/50 dark:bg-green-500 dark:ring-green-400/70",
	][Math.max(0, Math.min(3, level - 1))];
}

function percent(numerator: number, denominator: number) {
	return denominator === 0
		? "--"
		: `${Math.round((100 * numerator) / denominator)}%`;
}

function ActivityColorLegend() {
	return (
		<ul
			className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground"
			aria-label="活动图颜色图例"
		>
			<li className="inline-flex items-center gap-1.5">
				<span
					className="size-2.5 rounded-[2px] bg-muted/80 ring-1 ring-border/50"
					aria-hidden="true"
				/>
				<span>无调用</span>
			</li>
			<li className="inline-flex items-center gap-1.5">
				<span className="inline-flex gap-0.5" aria-hidden="true">
					<span className="size-2.5 rounded-[2px] bg-sky-200 ring-1 ring-sky-300/50 dark:bg-sky-950" />
					<span className="size-2.5 rounded-[2px] bg-cyan-300 ring-1 ring-cyan-400/50 dark:bg-cyan-800" />
					<span className="size-2.5 rounded-[2px] bg-emerald-400 ring-1 ring-emerald-500/50 dark:bg-emerald-700" />
					<span className="size-2.5 rounded-[2px] bg-green-500 ring-1 ring-green-600/50 dark:bg-green-500" />
				</span>
				<span>调用量低至高</span>
			</li>
			<li className="inline-flex items-center gap-1.5">
				<span
					className="size-2.5 rounded-[2px] bg-amber-400 ring-1 ring-amber-500/70"
					aria-hidden="true"
				/>
				<span>含失败</span>
			</li>
			<li className="inline-flex items-center gap-1.5">
				<span
					className="size-2.5 rounded-[2px] bg-destructive ring-1 ring-destructive/70"
					aria-hidden="true"
				/>
				<span>全部失败</span>
			</li>
		</ul>
	);
}

function ModelLegend({ data }: { data: AdminLlmActivityResponse }) {
	return (
		<ul
			className="grid gap-1 text-xs text-muted-foreground sm:hidden"
			aria-label="模型图例"
		>
			{data.models.map((model) => (
				<li key={model.model} className="flex min-w-0 items-center gap-2">
					<span className="w-5 shrink-0 text-center font-mono text-[10px]">
						{model.priority || "·"}
					</span>
					<span className="min-w-0 truncate font-mono">{model.model}</span>
				</li>
			))}
		</ul>
	);
}

function SkeletonModel(): ActivityGridModel {
	const columns: ActivityGridMatrixColumn[] = Array.from(
		{ length: 12 },
		(_, index) => ({ id: `skeleton-${index}`, label: "" }),
	);
	const rows: ActivityGridMatrixRow[] = Array.from(
		{ length: 3 },
		(_, index) => ({
			id: `skeleton-row-${index}`,
			label: "",
			cells: columns.map((column) => ({
				id: `${column.id}:${index}`,
				ariaLabel: "加载中",
				className: "bg-muted",
				preview: { title: "加载中", lines: [] },
			})),
		}),
	);
	return {
		layout: "matrix",
		scrollPolicy: "panel",
		ariaLabel: "模型活动",
		columns,
		rows,
		cellSize: 12,
		cellGap: 2,
		testId: "llm-activity-surface",
	};
}

function makeModel(
	data: AdminLlmActivityResponse,
	onOpenCalls?: LlmActivityGridProps["onOpenCalls"],
): ActivityGridModel {
	const maximum = Math.max(
		0,
		...data.buckets.flatMap((bucket) =>
			bucket.counts.map((count) => count.succeeded + count.failed),
		),
	);
	const columns: ActivityGridMatrixColumn[] = data.buckets.map((bucket) => ({
		id: bucket.started_at,
		label: localTime(bucket.started_at).slice(0, 5),
		title: localTime(bucket.started_at),
	}));
	const rows: ActivityGridMatrixRow[] = data.models.map((model) => ({
		id: model.model,
		label: `${model.configured && model.priority ? `${model.priority}. ` : ""}${model.model}`,
		mobileLabel: model.priority ? String(model.priority) : "·",
		cells: data.buckets.map((bucket, index): ActivityGridCell => {
			const count = bucket.counts.find(
				(item) => item.model === model.model,
			) ?? { succeeded: 0, failed: 0 };
			const outcome = activityOutcome(count.succeeded, count.failed);
			const bucketSucceeded = bucket.counts.reduce(
				(sum, item) => sum + item.succeeded,
				0,
			);
			const target: LlmCallDrilldown = {
				model: model.model,
				finishedFrom: bucket.started_at,
				finishedBefore: bucket.ended_at,
			};
			return {
				id: `${model.model}:${bucket.started_at}`,
				ariaLabel: `${localTime(bucket.started_at)}，${model.model}，成功 ${count.succeeded}，失败 ${count.failed}`,
				ariaControls: "llm-activity-summary",
				dataOutcome: outcome,
				className: activityClass(count.succeeded, count.failed, maximum),
				data: { model: model.model, column: index },
				preview: {
					title: `${model.model} 在 ${localTime(bucket.started_at)} 的调用摘要`,
					lines: [],
					content: (
						<div className="mt-2 grid grid-cols-[minmax(0,1fr)_auto_auto_auto_auto] gap-x-3 gap-y-1 text-xs">
							<span className="text-muted-foreground">模型</span>
							<span className="text-right text-muted-foreground">成功</span>
							<span className="text-right text-muted-foreground">失败</span>
							<span className="text-right text-muted-foreground">成功率</span>
							<span className="text-right text-muted-foreground">使用率</span>
							{bucket.counts.map((item) => (
								<div key={item.model} className="contents">
									<span className="truncate font-mono" title={item.model}>
										{item.model}
									</span>
									<span className="text-right tabular-nums">
										{item.succeeded}
									</span>
									<span className="text-right tabular-nums">{item.failed}</span>
									<span className="text-right tabular-nums">
										{percent(item.succeeded, item.succeeded + item.failed)}
									</span>
									<span className="text-right tabular-nums">
										{percent(item.succeeded, bucketSucceeded)}
									</span>
								</div>
							))}
							{onOpenCalls ? (
								<div className="col-span-full mt-2 flex justify-end">
									<LlmCallActionsMenu
										target={target}
										onOpen={onOpenCalls}
										label={`${model.model} 的调用操作`}
									/>
								</div>
							) : null}
						</div>
					),
				},
				decorate: (element) =>
					onOpenCalls ? (
						<LlmCallContextMenu target={target} onOpen={onOpenCalls}>
							{
								element as ReactElement<{
									onKeyDown?: (event: ReactKeyboardEvent<HTMLElement>) => void;
								}>
							}
						</LlmCallContextMenu>
					) : (
						element
					),
			};
		}),
	}));
	return {
		layout: "matrix",
		scrollPolicy: "panel",
		ariaLabel: "模型活动",
		columns,
		rows,
		cellSize: 12,
		cellGap: 2,
		rowLabelWidth: 152,
		mobileRowLabelWidth: 28,
		testId: "llm-activity-surface",
	};
}

export const LlmActivityGrid = memo(function LlmActivityGrid({
	data,
	loading = false,
	refreshing = false,
	error = null,
	onRetry,
	onOpenCalls,
}: LlmActivityGridProps) {
	const model = useMemo(
		() => (data ? makeModel(data, onOpenCalls) : SkeletonModel()),
		[data, onOpenCalls],
	);

	if (!data && error) {
		return (
			<div
				className="flex min-h-44 flex-col items-center justify-center gap-3 rounded-md border border-dashed p-6 text-center"
				role="alert"
				aria-live="assertive"
			>
				<AlertCircle className="size-5 text-destructive" />
				<p className="max-w-md text-sm text-muted-foreground">{error}</p>
				{onRetry ? (
					<Button type="button" variant="outline" size="sm" onClick={onRetry}>
						<RefreshCw />
						重试
					</Button>
				) : null}
			</div>
		);
	}

	return (
		<div className="min-w-0" data-testid="llm-activity-grid">
			<div className="mb-2 flex min-h-8 flex-col items-start justify-between gap-2 sm:flex-row sm:items-center sm:gap-3">
				<div className="flex shrink-0 items-center gap-2">
					<p className="text-xs text-muted-foreground">
						最近 {data?.buckets.length ?? 12} 小时 · 本地时间
					</p>
					{refreshing ? (
						<span
							className="inline-flex items-center gap-1 text-xs text-muted-foreground"
							role="status"
						>
							<LoaderCircle className="size-3 animate-spin" />
							更新中
						</span>
					) : null}
				</div>
				<ActivityColorLegend />
			</div>
			{data && data.buckets.length > 0 ? (
				<div
					className="mb-2 flex items-center justify-between font-mono text-[10px] tabular-nums text-muted-foreground sm:hidden"
					data-testid="llm-activity-mobile-range"
				>
					<span>{localTime(data.buckets[0].started_at)}</span>
					<span aria-hidden="true">至</span>
					<span>
						{localTime(data.buckets.at(-1)?.ended_at ?? data.window_ended_at)}
					</span>
				</div>
			) : null}
			<ActivityGrid
				model={model}
				loading={loading || (!data && !error)}
				onActivate={() => undefined}
				previewTestId="llm-activity-summary"
			/>
			{data ? <ModelLegend data={data} /> : null}
			{error && data ? (
				<p
					className="mt-2 text-xs text-amber-700 dark:text-amber-300"
					role="status"
				>
					{error}
				</p>
			) : null}
		</div>
	);
});
