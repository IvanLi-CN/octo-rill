import { RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
	Area,
	AreaChart,
	Bar,
	BarChart,
	CartesianGrid,
	Cell,
	Line,
	LineChart,
	Pie,
	PieChart,
	ResponsiveContainer,
	Tooltip,
	XAxis,
	YAxis,
} from "recharts";

import {
	type AdminDashboardResponse,
	type AdminDashboardTaskStatusItem,
	type AdminDashboardWindowValue,
	ApiError,
	apiGetAdminDashboard,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";

const NUMBER_FORMATTER = new Intl.NumberFormat();
const PERCENT_FORMATTER = new Intl.NumberFormat(undefined, {
	style: "percent",
	maximumFractionDigits: 0,
});
const DATETIME_FORMATTER = new Intl.DateTimeFormat(undefined, {
	month: "2-digit",
	day: "2-digit",
	hour: "2-digit",
	minute: "2-digit",
	hour12: false,
});

const TASK_COLORS = {
	翻译: "#3b82f6",
	智能摘要: "#6366f1",
	日报: "#10b981",
} as const;

const STATUS_COLORS = {
	queued: "#f59e0b",
	running: "#38bdf8",
	succeeded: "#10b981",
	failed: "#ef4444",
	canceled: "#94a3b8",
} as const;

const STATUS_LABELS = {
	queued: "排队",
	running: "运行中",
	succeeded: "成功",
	failed: "失败",
	canceled: "取消",
} as const;

const STATUS_ORDER = [
	"queued",
	"running",
	"succeeded",
	"failed",
	"canceled",
] as const;

const WINDOW_LABELS: Record<AdminDashboardWindowValue, string> = {
	"7d": "近 7 天",
	"30d": "近 30 天",
};

const SUBPANEL_CLASS = "rounded-lg border bg-background/70 p-3 shadow-sm";
const METRIC_CARD_CLASS = "rounded-lg border bg-background/80 p-3.5 shadow-sm";

const OVERVIEW_SKELETON_KEYS = [
	"metric-users",
	"metric-active",
	"metric-ongoing",
	"metric-total",
] as const;
const LANE_SKELETON_KEYS = [
	"lane-translate",
	"lane-summary",
	"lane-brief",
] as const;

function formatCount(value: number | null | undefined) {
	if (typeof value !== "number") return "-";
	return NUMBER_FORMATTER.format(value);
}

function formatPercent(value: number | null | undefined) {
	if (typeof value !== "number" || Number.isNaN(value)) return "-";
	return PERCENT_FORMATTER.format(value);
}

function formatGeneratedAt(value: string | null | undefined) {
	if (!value) return "-";
	const parsed = new Date(value);
	if (Number.isNaN(parsed.getTime())) return "-";
	return DATETIME_FORMATTER.format(parsed);
}

function chartTaskColor(label: string) {
	return TASK_COLORS[label as keyof typeof TASK_COLORS] ?? "#3b82f6";
}

function statusBadgeClass(status: keyof typeof STATUS_COLORS) {
	switch (status) {
		case "queued":
			return "border-amber-300 bg-amber-100/90 text-amber-900 dark:border-amber-500/60 dark:bg-amber-500/20 dark:text-amber-100";
		case "running":
			return "border-sky-300 bg-sky-100/90 text-sky-900 dark:border-sky-500/60 dark:bg-sky-500/20 dark:text-sky-100";
		case "succeeded":
			return "border-emerald-300 bg-emerald-100/90 text-emerald-900 dark:border-emerald-500/60 dark:bg-emerald-500/20 dark:text-emerald-100";
		case "failed":
			return "border-red-300 bg-red-100/90 text-red-900 dark:border-red-500/60 dark:bg-red-500/20 dark:text-red-100";
		case "canceled":
			return "border-slate-300 bg-slate-100/90 text-slate-900 dark:border-slate-500/60 dark:bg-slate-500/20 dark:text-slate-100";
	}
}

function ChartTooltip(props: {
	active?: boolean;
	label?: string;
	payload?: Array<{ name: string; value: number; color?: string }>;
}) {
	if (!props.active || !props.payload || props.payload.length === 0) {
		return null;
	}

	return (
		<div className="rounded-lg border bg-card px-3 py-2 shadow-sm">
			<p className="text-sm font-medium">{props.label}</p>
			<div className="mt-2 space-y-1.5">
				{props.payload.map((item) => (
					<div
						key={`${props.label}-${item.name}`}
						className="flex items-center justify-between gap-3 text-xs"
					>
						<div className="text-muted-foreground flex items-center gap-2">
							<span
								className="size-2 rounded-full"
								style={{ backgroundColor: item.color ?? "#64748b" }}
							/>
							<span>{item.name}</span>
						</div>
						<span className="font-medium">{formatCount(item.value)}</span>
					</div>
				))}
			</div>
		</div>
	);
}

function StatCard(props: {
	label: string;
	value: string;
	description: string;
	dotClass: string;
	meta: string;
}) {
	const { label, value, description, dotClass, meta } = props;
	return (
		<div className={cn(METRIC_CARD_CLASS, "space-y-3")}>
			<div className="flex items-center justify-between gap-3">
				<div className="flex items-center gap-2">
					<span className={cn("size-2 rounded-full", dotClass)} />
					<p className="text-muted-foreground font-mono text-xs">{label}</p>
				</div>
				<p className="text-muted-foreground font-mono text-[11px]">{meta}</p>
			</div>
			<p className="text-3xl font-semibold tracking-tight sm:text-[2.125rem]">
				{value}
			</p>
			<p className="text-muted-foreground text-xs leading-5">{description}</p>
		</div>
	);
}

function MiniMetric(props: {
	label: string;
	value: string;
	description: string;
	dotClass: string;
}) {
	const { label, value, description, dotClass } = props;
	return (
		<div className={cn(SUBPANEL_CLASS, "space-y-1.5 px-3 py-3")}>
			<div className="flex items-center gap-2">
				<span className={cn("size-2 rounded-full", dotClass)} />
				<p className="text-muted-foreground font-mono text-xs">{label}</p>
			</div>
			<p className="text-2xl font-semibold tracking-tight">{value}</p>
			<p className="text-muted-foreground text-[11px] leading-5">
				{description}
			</p>
		</div>
	);
}

function TaskLaneCard(props: {
	label: string;
	value: number;
	description: string;
	borderClass: string;
	dotClass: string;
}) {
	const { label, value, description, borderClass, dotClass } = props;
	return (
		<div className={cn(METRIC_CARD_CLASS, "border-l-4", borderClass)}>
			<div className="flex items-center justify-between gap-3">
				<div>
					<p className="font-medium">{label}</p>
					<p className="text-muted-foreground mt-1 text-[11px]">
						{description}
					</p>
				</div>
				<div className="flex items-center gap-2">
					<span className={cn("size-2 rounded-full", dotClass)} />
					<p className="text-2xl font-semibold tracking-tight">
						{formatCount(value)}
					</p>
				</div>
			</div>
		</div>
	);
}

function OverviewSummaryStrip(props: {
	items: Array<{
		label: string;
		value: string;
		detail: string;
		dotClass: string;
	}>;
}) {
	return (
		<div className="rounded-lg border bg-background/70 shadow-sm">
			<div className="grid divide-y md:grid-cols-3 md:divide-x md:divide-y-0">
				{props.items.map((item) => (
					<div key={item.label} className="space-y-1.5 px-4 py-3">
						<div className="flex items-center gap-2">
							<span className={cn("size-2 rounded-full", item.dotClass)} />
							<p className="text-muted-foreground font-mono text-xs">
								{item.label}
							</p>
						</div>
						<div className="flex items-end justify-between gap-3">
							<p className="text-2xl font-semibold tracking-tight">
								{item.value}
							</p>
							<p className="text-muted-foreground text-[11px]">{item.detail}</p>
						</div>
					</div>
				))}
			</div>
		</div>
	);
}

function TrendSnapshotStrip(props: {
	items: Array<{
		label: string;
		value: string;
		description: string;
		dotClass: string;
	}>;
}) {
	return (
		<div className="overflow-hidden rounded-lg border bg-background/70 shadow-sm">
			<div className="grid gap-px bg-border sm:grid-cols-3">
				{props.items.map((item) => (
					<div
						key={item.label}
						className="bg-background/95 px-4 py-3 sm:min-h-[5.5rem]"
					>
						<div className="flex items-center gap-2">
							<span className={cn("size-2 rounded-full", item.dotClass)} />
							<p className="text-muted-foreground font-mono text-xs">
								{item.label}
							</p>
						</div>
						<p className="mt-2 text-2xl font-semibold tracking-tight">
							{item.value}
						</p>
						<p className="text-muted-foreground mt-1 text-[11px] leading-5">
							{item.description}
						</p>
					</div>
				))}
			</div>
		</div>
	);
}

function ProgressTrack(props: { value: number; color: string }) {
	return (
		<div className="bg-muted mt-2 h-1.5 overflow-hidden rounded-full">
			<div
				className="h-full rounded-full"
				style={{
					width: `${Math.max(6, Math.min(100, props.value * 100))}%`,
					backgroundColor: props.color,
				}}
			/>
		</div>
	);
}

function OverviewSkeleton() {
	return (
		<div
			className="space-y-3 sm:space-y-4"
			data-admin-dashboard-skeleton="true"
		>
			<div className="text-muted-foreground text-sm">
				这里聚焦整体运营指标，正在准备最新统计数据…
			</div>
			<Card>
				<CardHeader>
					<div className="bg-muted h-6 w-24 animate-pulse rounded-full" />
					<div className="bg-muted h-4 w-80 animate-pulse rounded-full" />
				</CardHeader>
				<CardContent className="space-y-4">
					<div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
						{OVERVIEW_SKELETON_KEYS.map((key) => (
							<div
								key={key}
								className="bg-muted h-32 animate-pulse rounded-lg border"
							/>
						))}
					</div>
					<div className="grid gap-3 lg:grid-cols-3">
						{LANE_SKELETON_KEYS.map((key) => (
							<div
								key={key}
								className="bg-muted h-24 animate-pulse rounded-lg border"
							/>
						))}
					</div>
				</CardContent>
			</Card>
		</div>
	);
}

export function AdminDashboard() {
	const [data, setData] = useState<AdminDashboardResponse | null>(null);
	const [loading, setLoading] = useState(true);
	const [refreshing, setRefreshing] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [windowValue, setWindowValue] =
		useState<AdminDashboardWindowValue>("7d");

	const loadDashboard = useCallback(
		async (
			window: AdminDashboardWindowValue,
			mode: "initial" | "refresh" = "initial",
		) => {
			if (mode === "initial") {
				setLoading(true);
			} else {
				setRefreshing(true);
			}
			setError(null);
			try {
				const next = await apiGetAdminDashboard(window);
				setData(next);
			} catch (err) {
				if (err instanceof ApiError) {
					setError(err.message);
				} else {
					setError(err instanceof Error ? err.message : String(err));
				}
			} finally {
				setLoading(false);
				setRefreshing(false);
			}
		},
		[],
	);

	useEffect(() => {
		void loadDashboard(windowValue, "initial");
	}, [loadDashboard, windowValue]);

	const statusItems = data?.status_breakdown.items ?? [];
	const todayChartData = useMemo(
		() =>
			statusItems.map((item) => ({
				label: item.label,
				queued: item.queued,
				running: item.running,
				succeeded: item.succeeded,
				failed: item.failed,
				canceled: item.canceled,
			})),
		[statusItems],
	);

	const trendChartData = useMemo(
		() =>
			(data?.trend_points ?? []).map((item) => ({
				label: item.label,
				翻译: item.translations_total,
				智能摘要: item.summaries_total,
				日报: item.briefs_total,
				active_users: item.active_users,
				total_users: item.total_users,
			})),
		[data],
	);

	const shareChartData = useMemo(
		() =>
			(data?.task_share ?? [])
				.filter((item) => item.total > 0)
				.map((item) => ({
					name: item.label,
					value: item.total,
					fill: chartTaskColor(item.label),
					share_ratio: item.share_ratio,
					success_rate: item.success_rate,
				})),
		[data],
	);

	const highestVolumeTask = useMemo(() => {
		return statusItems.reduce<AdminDashboardTaskStatusItem | null>(
			(best, item) => {
				if (!best || item.total > best.total) return item;
				return best;
			},
			null,
		);
	}, [statusItems]);

	const totalTodayTasks = data?.status_breakdown.total ?? 0;
	const successRate =
		totalTodayTasks > 0
			? (data?.status_breakdown.succeeded_total ?? 0) / totalTodayTasks
			: 0;
	const activeRate =
		(data?.today_live.total_users ?? 0) > 0
			? (data?.today_live.active_users ?? 0) /
				(data?.today_live.total_users ?? 1)
			: 0;
	const latestTrendPoint = trendChartData.at(-1);
	const maxShareTotal = Math.max(
		1,
		...shareChartData.map((item) => item.value),
	);
	const highestVolumeShare =
		highestVolumeTask && totalTodayTasks > 0
			? highestVolumeTask.total / totalTodayTasks
			: 0;
	const failureRate =
		totalTodayTasks > 0
			? (data?.status_breakdown.failed_total ?? 0) / totalTodayTasks
			: 0;

	if (loading && !data) {
		return <OverviewSkeleton />;
	}

	return (
		<div className="space-y-3 sm:space-y-4" data-admin-dashboard-shell="true">
			{error ? (
				<p className="text-destructive text-sm">仪表盘数据加载失败：{error}</p>
			) : null}

			<p className="text-muted-foreground text-sm">
				这里聚焦整体运营指标与任务态势；历史趋势来自每日
				rollup，今日数据实时覆盖。
			</p>

			<Card>
				<CardHeader className="gap-4">
					<div className="flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
						<div>
							<CardTitle>运营总览</CardTitle>
							<CardDescription>
								展示用户规模、今日活跃与翻译 / 智能摘要 /
								日报三条链路的关键指标。
							</CardDescription>
						</div>
						<div className="flex flex-wrap items-center gap-2">
							<Badge variant="outline">系统时区 {data?.time_zone ?? "-"}</Badge>
							<Button
								size="sm"
								variant="outline"
								disabled={refreshing}
								onClick={() => void loadDashboard(windowValue, "refresh")}
							>
								<RefreshCw
									className={cn("mr-1.5 size-4", refreshing && "animate-spin")}
								/>
								刷新
							</Button>
							<Tabs
								value={windowValue}
								onValueChange={(value) =>
									setWindowValue(value as AdminDashboardWindowValue)
								}
							>
								<TabsList className="h-8">
									<TabsTrigger value="7d" className="font-mono text-xs">
										近 7 天
									</TabsTrigger>
									<TabsTrigger value="30d" className="font-mono text-xs">
										近 30 天
									</TabsTrigger>
								</TabsList>
							</Tabs>
						</div>
					</div>
				</CardHeader>
				<CardContent className="space-y-3">
					<div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
						<StatCard
							label="用户总数"
							value={formatCount(data?.summary.total_users)}
							description="当前后台可管理用户规模。"
							dotClass="bg-foreground"
							meta="规模"
						/>
						<StatCard
							label="今日活跃用户"
							value={formatCount(data?.summary.active_users_today)}
							description="按系统时区统计的活跃人数。"
							dotClass="bg-sky-500"
							meta={formatPercent(activeRate)}
						/>
						<StatCard
							label="进行中任务"
							value={formatCount(data?.summary.ongoing_tasks_total)}
							description={`排队 ${formatCount(data?.summary.queued_tasks)} · 运行中 ${formatCount(data?.summary.running_tasks)}`}
							dotClass="bg-amber-500"
							meta="实时"
						/>
						<StatCard
							label="今日任务总量"
							value={formatCount(totalTodayTasks)}
							description={`成功 ${formatCount(data?.status_breakdown.succeeded_total)} · 失败 ${formatCount(data?.status_breakdown.failed_total)}`}
							dotClass="bg-emerald-500"
							meta={formatPercent(successRate)}
						/>
					</div>

					<OverviewSummaryStrip
						items={[
							{
								label: "今日成功率",
								value: formatPercent(successRate),
								detail: "已完成任务中的成功占比",
								dotClass: "bg-sky-500",
							},
							{
								label: "当前热点链路",
								value: highestVolumeTask?.label ?? "暂无",
								detail: highestVolumeTask
									? `${formatCount(highestVolumeTask.total)} 个任务正在推进`
									: "等待今日任务进入队列",
								dotClass: "bg-indigo-500",
							},
							{
								label: "今日活跃占比",
								value: formatPercent(activeRate),
								detail: "活跃用户相对总用户的覆盖率",
								dotClass: "bg-foreground",
							},
						]}
					/>

					<div className="text-muted-foreground flex flex-wrap items-center gap-2 text-xs">
						<span>
							窗口 {data?.window_meta.window_start ?? "-"} →{" "}
							{data?.window_meta.window_end ?? "-"}
						</span>
						<span>·</span>
						<span>
							{WINDOW_LABELS[data?.window_meta.selected_window ?? windowValue]}{" "}
							趋势
						</span>
						<span>·</span>
						<span>最近更新 {formatGeneratedAt(data?.generated_at)}</span>
					</div>
				</CardContent>
			</Card>

			<div className="grid gap-4 xl:grid-cols-2 xl:items-start">
				<div className="space-y-4">
					<Card>
						<CardHeader className="gap-3">
							<div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
								<div>
									<CardTitle>今日执行状态分布</CardTitle>
									<CardDescription>
										按任务类型拆分今日排队 / 运行 / 成功 / 失败 /
										取消的实时状态。
									</CardDescription>
								</div>
								<div className="flex flex-wrap gap-2">
									{STATUS_ORDER.map((status) => (
										<Badge
											key={status}
											variant="outline"
											className={statusBadgeClass(status)}
										>
											{STATUS_LABELS[status]}{" "}
											{formatCount(
												data?.status_breakdown[`${status}_total` as const] ?? 0,
											)}
										</Badge>
									))}
								</div>
							</div>
						</CardHeader>
						<CardContent>
							<div
								className={cn(SUBPANEL_CLASS, "h-[16rem] sm:h-[17rem]")}
								data-admin-dashboard-chart-today="true"
							>
								<ResponsiveContainer width="100%" height="100%">
									<BarChart data={todayChartData} barGap={10}>
										<CartesianGrid
											stroke="#d6d3d1"
											strokeDasharray="3 3"
											vertical={false}
										/>
										<XAxis
											dataKey="label"
											tickLine={false}
											axisLine={false}
											tick={{ fill: "#78716c", fontSize: 12 }}
										/>
										<YAxis
											tickLine={false}
											axisLine={false}
											allowDecimals={false}
											tick={{ fill: "#78716c", fontSize: 12 }}
										/>
										<Tooltip content={<ChartTooltip />} />
										<Bar
											dataKey="queued"
											name="排队"
											stackId="status"
											fill={STATUS_COLORS.queued}
											radius={[6, 6, 0, 0]}
											isAnimationActive={false}
										/>
										<Bar
											dataKey="running"
											name="运行中"
											stackId="status"
											fill={STATUS_COLORS.running}
											isAnimationActive={false}
										/>
										<Bar
											dataKey="succeeded"
											name="成功"
											stackId="status"
											fill={STATUS_COLORS.succeeded}
											isAnimationActive={false}
										/>
										<Bar
											dataKey="failed"
											name="失败"
											stackId="status"
											fill={STATUS_COLORS.failed}
											isAnimationActive={false}
										/>
										<Bar
											dataKey="canceled"
											name="取消"
											stackId="status"
											fill={STATUS_COLORS.canceled}
											radius={[6, 6, 0, 0]}
											isAnimationActive={false}
										/>
									</BarChart>
								</ResponsiveContainer>
							</div>
						</CardContent>
					</Card>

					<Card>
						<CardHeader>
							<CardTitle>今日任务占比</CardTitle>
							<CardDescription>
								快速判断今天的压力主要落在哪条链路上。
							</CardDescription>
						</CardHeader>
						<CardContent className="grid gap-4 lg:grid-cols-[220px_minmax(0,1fr)]">
							<div className="space-y-3">
								<div
									className={cn(SUBPANEL_CLASS, "relative h-[14rem] p-0")}
									data-admin-dashboard-chart-share="true"
								>
									<ResponsiveContainer width="100%" height="100%">
										<PieChart>
											<Tooltip content={<ChartTooltip />} />
											<Pie
												data={shareChartData}
												dataKey="value"
												nameKey="name"
												cx="50%"
												cy="50%"
												innerRadius={54}
												outerRadius={84}
												paddingAngle={3}
												isAnimationActive={false}
											>
												{shareChartData.map((entry) => (
													<Cell key={`share-${entry.name}`} fill={entry.fill} />
												))}
											</Pie>
										</PieChart>
									</ResponsiveContainer>
									<div className="pointer-events-none absolute inset-0 flex items-center justify-center">
										<div className="rounded-full border bg-card px-4 py-3 text-center shadow-sm">
											<p className="text-muted-foreground font-mono text-[11px]">
												今日总任务
											</p>
											<p className="mt-1 text-2xl font-semibold tracking-tight">
												{formatCount(totalTodayTasks)}
											</p>
										</div>
									</div>
								</div>

								<div className={cn(SUBPANEL_CLASS, "space-y-3")}>
									<div className="flex items-center justify-between gap-3 text-sm">
										<div>
											<p className="text-muted-foreground font-mono text-[11px]">
												头部链路
											</p>
											<p className="mt-1 font-medium">
												{highestVolumeTask?.label ?? "暂无"}
											</p>
										</div>
										<p className="text-right font-semibold">
											{formatPercent(highestVolumeShare)}
										</p>
									</div>
									<div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-1">
										<div className="rounded-md border bg-card/70 px-3 py-2">
											<p className="text-muted-foreground font-mono text-[11px]">
												实时进行中
											</p>
											<p className="mt-1 text-lg font-semibold">
												{formatCount(data?.summary.ongoing_tasks_total)}
											</p>
										</div>
										<div className="rounded-md border bg-card/70 px-3 py-2">
											<p className="text-muted-foreground font-mono text-[11px]">
												失败占比
											</p>
											<p className="mt-1 text-lg font-semibold">
												{formatPercent(failureRate)}
											</p>
										</div>
									</div>
								</div>
							</div>

							<div className="space-y-3">
								{shareChartData.length > 0 ? (
									shareChartData.map((item) => (
										<div
											key={item.name}
											className={cn(SUBPANEL_CLASS, "space-y-3")}
										>
											<div className="flex items-center justify-between gap-3">
												<div className="flex items-center gap-2">
													<span
														className="size-2.5 rounded-full"
														style={{ backgroundColor: item.fill }}
													/>
													<p className="font-medium">{item.name}</p>
												</div>
												<div className="text-right">
													<p className="font-medium">
														{formatCount(item.value)}
													</p>
													<p className="text-muted-foreground text-xs">
														{formatPercent(item.share_ratio)}
													</p>
												</div>
											</div>
											<ProgressTrack
												value={item.value / maxShareTotal}
												color={item.fill}
											/>
											<p className="text-muted-foreground text-xs">
												{formatPercent(item.success_rate)} 成功率
											</p>
										</div>
									))
								) : (
									<p className="text-muted-foreground text-sm">
										今日暂无可视化任务占比数据。
									</p>
								)}
							</div>
						</CardContent>
					</Card>

					<Card>
						<CardHeader>
							<CardTitle>活跃用户与规模</CardTitle>
							<CardDescription>
								辅助判断用户活跃波动是否与任务量变化同步。
							</CardDescription>
						</CardHeader>
						<CardContent className="space-y-4">
							<div className="grid gap-3 sm:grid-cols-2">
								<MiniMetric
									label="今日活跃"
									value={formatCount(data?.today_live.active_users)}
									description={`总用户 ${formatCount(data?.today_live.total_users)}`}
									dotClass="bg-sky-500"
								/>
								<MiniMetric
									label="实时进行中"
									value={formatCount(data?.today_live.ongoing_tasks_total)}
									description="当前还在队列或运行态的任务量"
									dotClass="bg-amber-500"
								/>
							</div>
							<div
								className={cn(SUBPANEL_CLASS, "h-[15rem] sm:h-[16rem]")}
								data-admin-dashboard-chart-active="true"
							>
								<ResponsiveContainer width="100%" height="100%">
									<LineChart data={trendChartData}>
										<CartesianGrid
											stroke="#d6d3d1"
											strokeDasharray="3 3"
											vertical={false}
										/>
										<XAxis
											dataKey="label"
											tickLine={false}
											axisLine={false}
											minTickGap={windowValue === "30d" ? 18 : 8}
											tick={{ fill: "#78716c", fontSize: 12 }}
										/>
										<YAxis
											tickLine={false}
											axisLine={false}
											allowDecimals={false}
											tick={{ fill: "#78716c", fontSize: 12 }}
										/>
										<Tooltip content={<ChartTooltip />} />
										<Line
											type="monotone"
											dataKey="active_users"
											name="活跃用户"
											stroke="#3b82f6"
											strokeWidth={2.25}
											dot={false}
											isAnimationActive={false}
										/>
										<Line
											type="monotone"
											dataKey="total_users"
											name="用户总量"
											stroke="#44403c"
											strokeWidth={2}
											dot={false}
											isAnimationActive={false}
										/>
									</LineChart>
								</ResponsiveContainer>
							</div>
						</CardContent>
					</Card>
				</div>

				<div className="space-y-4">
					<Card>
						<CardHeader>
							<CardTitle>实时任务链路</CardTitle>
							<CardDescription>
								直接查询当前队列，补齐今日统计与历史 rollup 的时效差。
							</CardDescription>
						</CardHeader>
						<CardContent className="space-y-3">
							<div className="grid gap-3">
								<TaskLaneCard
									label="翻译"
									value={data?.summary.ongoing_by_task.translations ?? 0}
									description="发布翻译批任务"
									borderClass="border-l-sky-500"
									dotClass="bg-sky-500"
								/>
								<TaskLaneCard
									label="智能摘要"
									value={data?.summary.ongoing_by_task.summaries ?? 0}
									description="发布摘要批任务"
									borderClass="border-l-indigo-500"
									dotClass="bg-indigo-500"
								/>
								<TaskLaneCard
									label="日报"
									value={data?.summary.ongoing_by_task.briefs ?? 0}
									description="日报生成任务"
									borderClass="border-l-emerald-500"
									dotClass="bg-emerald-500"
								/>
							</div>
							<div className={cn(SUBPANEL_CLASS, "space-y-3")}>
								<div className="flex items-center justify-between gap-3">
									<div>
										<p className="font-medium">
											{highestVolumeTask
												? `${highestVolumeTask.label} 当前任务量最高`
												: "暂无今日任务数据"}
										</p>
										<p className="text-muted-foreground mt-1 text-xs">
											当前进行中{" "}
											{formatCount(data?.summary.ongoing_tasks_total)} 个任务。
										</p>
									</div>
									{highestVolumeTask ? (
										<Badge
											variant="outline"
											className="border-emerald-300 bg-emerald-100/90 text-emerald-900 dark:border-emerald-500/60 dark:bg-emerald-500/20 dark:text-emerald-100"
										>
											{formatPercent(highestVolumeTask.success_rate)} 成功率
										</Badge>
									) : null}
								</div>
								<div className="flex flex-wrap gap-2">
									{STATUS_ORDER.map((status) => (
										<Badge
											key={status}
											variant="outline"
											className={statusBadgeClass(status)}
										>
											{STATUS_LABELS[status]}{" "}
											{formatCount(
												data?.status_breakdown[`${status}_total` as const] ?? 0,
											)}
										</Badge>
									))}
								</div>
							</div>
						</CardContent>
					</Card>

					<Card>
						<CardHeader className="gap-3">
							<div>
								<CardTitle>{WINDOW_LABELS[windowValue]}任务趋势</CardTitle>
								<CardDescription>
									历史序列来自定时 rollup，今日点位用实时数据覆盖。
								</CardDescription>
							</div>
						</CardHeader>
						<CardContent className="space-y-3">
							<TrendSnapshotStrip
								items={[
									{
										label: "翻译",
										value: formatCount(latestTrendPoint?.翻译),
										description: "最新日翻译任务量",
										dotClass: "bg-sky-500",
									},
									{
										label: "智能摘要",
										value: formatCount(latestTrendPoint?.智能摘要),
										description: "最新日摘要任务量",
										dotClass: "bg-indigo-500",
									},
									{
										label: "日报",
										value: formatCount(latestTrendPoint?.日报),
										description: "最新日日报任务量",
										dotClass: "bg-emerald-500",
									},
								]}
							/>
							<div
								className={cn(SUBPANEL_CLASS, "h-[16rem] sm:h-[17rem]")}
								data-admin-dashboard-chart-trend="true"
							>
								<ResponsiveContainer width="100%" height="100%">
									<AreaChart data={trendChartData}>
										<defs>
											<linearGradient
												id="admin-dashboard-translate"
												x1="0"
												y1="0"
												x2="0"
												y2="1"
											>
												<stop
													offset="5%"
													stopColor={TASK_COLORS.翻译}
													stopOpacity={0.24}
												/>
												<stop
													offset="95%"
													stopColor={TASK_COLORS.翻译}
													stopOpacity={0}
												/>
											</linearGradient>
											<linearGradient
												id="admin-dashboard-summary"
												x1="0"
												y1="0"
												x2="0"
												y2="1"
											>
												<stop
													offset="5%"
													stopColor={TASK_COLORS.智能摘要}
													stopOpacity={0.22}
												/>
												<stop
													offset="95%"
													stopColor={TASK_COLORS.智能摘要}
													stopOpacity={0}
												/>
											</linearGradient>
											<linearGradient
												id="admin-dashboard-brief"
												x1="0"
												y1="0"
												x2="0"
												y2="1"
											>
												<stop
													offset="5%"
													stopColor={TASK_COLORS.日报}
													stopOpacity={0.2}
												/>
												<stop
													offset="95%"
													stopColor={TASK_COLORS.日报}
													stopOpacity={0}
												/>
											</linearGradient>
										</defs>
										<CartesianGrid
											stroke="#d6d3d1"
											strokeDasharray="3 3"
											vertical={false}
										/>
										<XAxis
											dataKey="label"
											tickLine={false}
											axisLine={false}
											minTickGap={windowValue === "30d" ? 18 : 8}
											tick={{ fill: "#78716c", fontSize: 12 }}
										/>
										<YAxis
											tickLine={false}
											axisLine={false}
											allowDecimals={false}
											tick={{ fill: "#78716c", fontSize: 12 }}
										/>
										<Tooltip content={<ChartTooltip />} />
										<Area
											type="monotone"
											dataKey="翻译"
											stroke={TASK_COLORS.翻译}
											fill="url(#admin-dashboard-translate)"
											strokeWidth={2}
											isAnimationActive={false}
										/>
										<Area
											type="monotone"
											dataKey="智能摘要"
											stroke={TASK_COLORS.智能摘要}
											fill="url(#admin-dashboard-summary)"
											strokeWidth={2}
											isAnimationActive={false}
										/>
										<Area
											type="monotone"
											dataKey="日报"
											stroke={TASK_COLORS.日报}
											fill="url(#admin-dashboard-brief)"
											strokeWidth={2}
											isAnimationActive={false}
										/>
									</AreaChart>
								</ResponsiveContainer>
							</div>
						</CardContent>
					</Card>

					<Card>
						<CardHeader>
							<CardTitle>今日任务 Stats</CardTitle>
							<CardDescription>
								保留稍高信息密度，方便直接对比各链路吞吐、异常与成功率。
							</CardDescription>
						</CardHeader>
						<CardContent>
							<div className="overflow-x-auto rounded-lg border bg-background/70">
								<table className="w-full min-w-[760px]">
									<thead className="text-muted-foreground text-left text-xs">
										<tr>
											<th className="px-4 py-3 font-medium">任务类型</th>
											<th className="px-4 py-3 font-medium">总量</th>
											<th className="px-4 py-3 font-medium">排队</th>
											<th className="px-4 py-3 font-medium">运行中</th>
											<th className="px-4 py-3 font-medium">成功</th>
											<th className="px-4 py-3 font-medium">失败</th>
											<th className="px-4 py-3 font-medium">成功率</th>
										</tr>
									</thead>
									<tbody>
										{statusItems.map((item) => (
											<tr key={item.task_type} className="border-t">
												<td className="px-4 py-3 align-top">
													<div className="flex items-center gap-3">
														<span
															className="size-2.5 rounded-full"
															style={{
																backgroundColor: chartTaskColor(item.label),
															}}
														/>
														<div>
															<p className="font-medium">{item.label}</p>
															<p className="text-muted-foreground mt-1 font-mono text-[11px]">
																{item.task_type}
															</p>
														</div>
													</div>
												</td>
												<td className="px-4 py-3 font-medium">
													{formatCount(item.total)}
												</td>
												<td className="px-4 py-3">
													{formatCount(item.queued)}
												</td>
												<td className="px-4 py-3">
													{formatCount(item.running)}
												</td>
												<td className="px-4 py-3">
													{formatCount(item.succeeded)}
												</td>
												<td className="px-4 py-3">
													{formatCount(item.failed)}
												</td>
												<td className="px-4 py-3 font-medium">
													{formatPercent(item.success_rate)}
												</td>
											</tr>
										))}
									</tbody>
								</table>
							</div>
						</CardContent>
					</Card>
				</div>
			</div>
		</div>
	);
}
