import {
	AlertCircle,
	Check,
	Files,
	LoaderCircle,
	RefreshCw,
} from "lucide-react";
import { memo, useMemo } from "react";

import type {
	AdminCollectionActivityBucket,
	AdminCollectionActivityCell,
	AdminCollectionActivityResponse,
	AdminCollectionActivityStatus,
	AdminCollectionRecordItem,
} from "@/api";
import {
	ActivityGrid,
	type ActivityGridCell,
	type ActivityGridModel,
	type ActivityGridWrappedRow,
} from "@/admin/ActivityGrid";
import { Button } from "@/components/ui/button";

const CELL_SIZE = 12;
const CELL_GAP = 2;

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

function statusText(value: string | null | undefined) {
	return value ? (PIPELINE_STATUS_LABELS[value] ?? value) : "-";
}

function cellLabel(cell: AdminCollectionActivityCell) {
	const repository = cell.repository ? `${cell.repository}，` : "";
	const translation = cell.translation_status
		? `，翻译 ${statusText(cell.translation_status)}`
		: "";
	return `${cell.title}，${repository}来源时间 ${formatTime(cell.source_time)}，综合状态 ${STATUS_LABELS[cell.composite_status]}${translation}，润色 ${statusText(cell.polish_status)}`;
}

function StatusLegend({ neutralCount }: { neutralCount: number }) {
	return (
		<ul
			className="flex flex-wrap items-center gap-x-4 gap-y-2 text-xs text-muted-foreground"
			aria-label="活动图状态图例"
		>
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
				<div key={label} className="min-w-0 border-l border-border pl-3">
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

function skeletonModel(): ActivityGridModel {
	return {
		layout: "wrapped-rows",
		scrollPolicy: "page",
		ariaLabel: "最近十二小时内容活动",
		cellSize: CELL_SIZE,
		cellGap: CELL_GAP,
		testId: "collection-activity-dom-grid",
		rows: Array.from({ length: 12 }, (_, index) => ({
			id: `skeleton-${index}`,
			label: `${String(11 - index).padStart(2, "0")}时`,
			title: "最近十二小时内容活动",
			cells: [],
		})),
	};
}

function makeGridModel(
	data: AdminCollectionActivityResponse,
): ActivityGridModel {
	const rows: ActivityGridWrappedRow[] = data.buckets.map(
		(bucket: AdminCollectionActivityBucket) => ({
			id: bucket.started_at,
			label: formatHour(bucket.started_at),
			title: `${formatTime(bucket.started_at)}–${formatTime(bucket.ended_at)}`,
			cells: bucket.cells.map(
				(cell): ActivityGridCell => ({
					id: `${cell.id}:${cell.source_time}`,
					ariaLabel: cellLabel(cell),
					className: STATUS_CLASS[cell.composite_status],
					color: STATUS_COLOR[cell.composite_status],
					data: cell,
					preview: {
						title: cell.title,
						lines: [
							cell.repository ? cell.repository : "无仓库",
							`来源时间 ${formatTime(cell.source_time)}`,
							`综合状态 ${STATUS_LABELS[cell.composite_status]}`,
							`翻译 ${statusText(cell.translation_status)} · 润色 ${statusText(cell.polish_status)}`,
						],
					},
				}),
			),
		}),
	);
	return {
		layout: "wrapped-rows",
		scrollPolicy: "page",
		ariaLabel: "最近十二小时内容活动",
		rows,
		cellSize: CELL_SIZE,
		cellGap: CELL_GAP,
		testId: "collection-activity-dom-grid",
	};
}

export const AdminCollectionActivity = memo(function AdminCollectionActivity({
	data,
	loading = false,
	error = null,
	selectedCellId,
	onRetry,
	onOpenRecord,
}: {
	data: AdminCollectionActivityResponse | null;
	loading?: boolean;
	error?: string | null;
	selectedCellId?: string | null;
	onRetry: () => void;
	onOpenRecord: (
		kind: AdminCollectionRecordItem["kind"],
		id: string,
		cellId: string,
	) => void;
}) {
	const model = useMemo(
		() => (data ? makeGridModel(data) : skeletonModel()),
		[data],
	);

	if (error && !data && !loading) {
		return (
			<section aria-label="内容处理活动" className="space-y-3 border-y py-4">
				<div className="flex flex-wrap items-start justify-between gap-3">
					<div className="min-w-0">
						<h3 className="text-sm font-medium">最近 12 小时活动暂不可用</h3>
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

	return (
		<section
			aria-label="内容处理活动"
			aria-busy={loading || undefined}
			className="space-y-4 border-y py-4"
		>
			<div className="flex flex-wrap items-start justify-between gap-3">
				<div className="min-w-0">
					<h3 className="text-sm font-medium">最近 12 小时</h3>
					{data ? (
						<p className="mt-1 text-xs text-muted-foreground">
							{formatTime(data.window_started_at)} –{" "}
							{formatTime(data.window_ended_at)}
						</p>
					) : (
						<div
							className="mt-2 h-3 w-44 animate-pulse rounded bg-muted"
							aria-hidden="true"
						/>
					)}
				</div>
				{data ? (
					<StatusLegend neutralCount={data.summary.neutral_count} />
				) : (
					<div
						className="h-4 w-52 animate-pulse rounded bg-muted"
						aria-hidden="true"
					/>
				)}
			</div>
			{data ? (
				<SummaryMetrics data={data} />
			) : (
				<div
					className="grid grid-cols-2 gap-x-6 gap-y-3 lg:grid-cols-4"
					aria-hidden="true"
				>
					{[0, 1, 2, 3].map((item) => (
						<div
							key={item}
							className="h-12 animate-pulse rounded border-l border-border bg-muted/40"
						/>
					))}
				</div>
			)}
			{data && loading ? (
				<p className="text-xs text-muted-foreground" role="status">
					正在更新
				</p>
			) : null}
			{data && error ? (
				<div
					className="flex flex-wrap items-center gap-2 text-xs text-amber-700 dark:text-amber-300"
					role="status"
				>
					<span>继续显示上次读取的数据：{error}</span>
					<Button type="button" variant="ghost" size="sm" onClick={onRetry}>
						重试
					</Button>
				</div>
			) : null}
			<ActivityGrid
				model={model}
				loading={loading || (!data && !error)}
				selectedCellId={selectedCellId}
				onActivate={(cell) => {
					const record = cell.data as AdminCollectionActivityCell | undefined;
					if (record) onOpenRecord(data?.kind ?? "release", record.id, cell.id);
				}}
				previewTestId="collection-activity-preview"
			/>
		</section>
	);
});
