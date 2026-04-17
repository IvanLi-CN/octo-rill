import { Activity, RefreshCw, Sparkles, Users } from "lucide-react";
import {
	type ComponentType,
	useCallback,
	useEffect,
	useMemo,
	useState,
} from "react";
import {
	Area,
	AreaChart,
	Bar,
	BarChart,
	CartesianGrid,
	Cell,
	Legend,
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
	ApiError,
	apiGetAdminDashboard,
} from "@/api";
import { readBrowserTimeZone } from "@/briefs/DailyBriefProfileForm";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
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
	翻译: "#6366f1",
	智能摘要: "#06b6d4",
	日报: "#f59e0b",
} as const;

const STATUS_COLORS = {
	queued: "#f59e0b",
	running: "#38bdf8",
	succeeded: "#10b981",
	failed: "#f43f5e",
	canceled: "#94a3b8",
} as const;

const STATUS_LABELS = {
	queued: "排队",
	running: "运行中",
	succeeded: "成功",
	failed: "失败",
	canceled: "取消",
} as const;
const OVERVIEW_SKELETON_HERO_KEYS = [
	"hero-kpi-users",
	"hero-kpi-active",
	"hero-kpi-ongoing",
	"hero-kpi-today",
] as const;
const OVERVIEW_SKELETON_LANE_KEYS = [
	"lane-translate",
	"lane-summary",
	"lane-brief",
] as const;
const OVERVIEW_SKELETON_CHART_KEYS = ["chart-today", "chart-share"] as const;

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
	return TASK_COLORS[label as keyof typeof TASK_COLORS] ?? "#6366f1";
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
			return "border-rose-300 bg-rose-100/90 text-rose-900 dark:border-rose-500/60 dark:bg-rose-500/20 dark:text-rose-100";
		case "canceled":
			return "border-slate-300 bg-slate-100/90 text-slate-900 dark:border-slate-500/60 dark:bg-slate-500/20 dark:text-slate-100";
	}
}

function CustomChartTooltip(props: {
	active?: boolean;
	label?: string;
	payload?: Array<{ name: string; value: number; color?: string }>;
}) {
	if (!props.active || !props.payload || props.payload.length === 0) {
		return null;
	}

	return (
		<div className="min-w-44 rounded-xl border bg-background/95 p-3 shadow-lg backdrop-blur">
			<p className="text-sm font-medium">{props.label}</p>
			<div className="mt-2 space-y-1.5">
				{props.payload.map((item) => (
					<div
						key={`${props.label}-${item.name}`}
						className="flex items-center justify-between gap-3 text-xs"
					>
						<div className="flex items-center gap-2 text-muted-foreground">
							<span
								className="size-2 rounded-full"
								style={{ backgroundColor: item.color ?? "#64748b" }}
							/>
							<span>{item.name}</span>
						</div>
						<span className="font-medium text-foreground">
							{formatCount(item.value)}
						</span>
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
	accentClass: string;
	icon: ComponentType<{ className?: string }>;
}) {
	const { label, value, description, accentClass, icon: Icon } = props;
	return (
		<div
			className={cn(
				"rounded-2xl border border-border/70 bg-card/78 p-4 shadow-sm",
				accentClass,
			)}
		>
			<div className="flex items-start justify-between gap-3">
				<div className="space-y-1">
					<p className="text-muted-foreground text-xs">{label}</p>
					<p className="text-2xl font-semibold tracking-tight">{value}</p>
				</div>
				<div className="rounded-xl border border-white/20 bg-white/10 p-2 dark:bg-white/5">
					<Icon className="size-4" />
				</div>
			</div>
			<p className="text-muted-foreground mt-3 text-xs leading-5">
				{description}
			</p>
		</div>
	);
}

function OverviewSkeleton() {
	return (
		<div className="space-y-4" data-admin-dashboard-skeleton="true">
			<div className="grid gap-4 xl:grid-cols-[1.55fr_1fr]">
				<div className="rounded-[28px] border border-border/70 bg-card/72 p-6">
					<div className="animate-pulse space-y-4">
						<div className="h-4 w-28 rounded-full bg-muted" />
						<div className="h-10 w-3/5 rounded-2xl bg-muted" />
						<div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
							{OVERVIEW_SKELETON_HERO_KEYS.map((key) => (
								<div key={key} className="h-32 rounded-2xl bg-muted/70" />
							))}
						</div>
					</div>
				</div>
				<div className="rounded-[28px] border border-border/70 bg-card/72 p-6">
					<div className="animate-pulse space-y-3">
						<div className="h-4 w-24 rounded-full bg-muted" />
						<div className="grid gap-3 sm:grid-cols-3 xl:grid-cols-1">
							{OVERVIEW_SKELETON_LANE_KEYS.map((key) => (
								<div key={key} className="h-24 rounded-2xl bg-muted/70" />
							))}
						</div>
						<div className="h-24 rounded-2xl bg-muted/60" />
					</div>
				</div>
			</div>
			<div className="grid gap-4 2xl:grid-cols-2">
				{OVERVIEW_SKELETON_CHART_KEYS.map((key) => (
					<div
						key={key}
						className="h-[24rem] rounded-[28px] border border-border/70 bg-card/72 p-6"
					>
						<div className="h-full animate-pulse rounded-2xl bg-muted/60" />
					</div>
				))}
			</div>
		</div>
	);
}

export function AdminDashboard() {
	const [data, setData] = useState<AdminDashboardResponse | null>(null);
	const [loading, setLoading] = useState(true);
	const [refreshing, setRefreshing] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const timeZone = useMemo(() => readBrowserTimeZone(), []);

	const loadDashboard = useCallback(
		async (mode: "initial" | "refresh" = "initial") => {
			if (mode === "initial") {
				setLoading(true);
			} else {
				setRefreshing(true);
			}
			setError(null);
			try {
				const next = await apiGetAdminDashboard(timeZone);
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
		[timeZone],
	);

	useEffect(() => {
		void loadDashboard("initial");
	}, [loadDashboard]);

	const todayChartData = useMemo(
		() =>
			(data?.today.task_status ?? []).map((item) => ({
				label: item.label,
				queued: item.queued,
				running: item.running,
				succeeded: item.succeeded,
				failed: item.failed,
				canceled: item.canceled,
			})),
		[data],
	);

	const trendChartData = useMemo(
		() =>
			(data?.trends ?? []).map((item) => ({
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
			(data?.today.task_status ?? [])
				.filter((item) => item.total > 0)
				.map((item) => ({
					name: item.label,
					value: item.total,
					fill: chartTaskColor(item.label),
				})),
		[data],
	);

	const highestVolumeTask = useMemo(() => {
		const items = data?.today.task_status ?? [];
		return items.reduce<AdminDashboardTaskStatusItem | null>((best, item) => {
			if (!best || item.total > best.total) return item;
			return best;
		}, null);
	}, [data]);

	if (loading && !data) {
		return <OverviewSkeleton />;
	}

	return (
		<div className="space-y-4" data-admin-dashboard-shell="true">
			{error ? (
				<div className="rounded-2xl border border-rose-300 bg-rose-100/80 px-4 py-3 text-sm text-rose-900 dark:border-rose-500/60 dark:bg-rose-500/15 dark:text-rose-100">
					<div className="flex flex-wrap items-center justify-between gap-3">
						<span>仪表盘数据加载失败：{error}</span>
						<Button
							size="sm"
							variant="outline"
							onClick={() => void loadDashboard("refresh")}
						>
							重试
						</Button>
					</div>
				</div>
			) : null}

			<div className="grid gap-4 xl:grid-cols-[1.55fr_1fr]">
				<Card
					className="relative overflow-hidden border-border/80 bg-card/88 shadow-sm"
					data-admin-dashboard-hero="true"
				>
					<div className="pointer-events-none absolute inset-x-0 top-0 h-40 bg-[radial-gradient(circle_at_top_left,rgba(99,102,241,0.18),transparent_48%),radial-gradient(circle_at_top_right,rgba(6,182,212,0.18),transparent_42%),linear-gradient(180deg,rgba(255,255,255,0.18),transparent)] dark:bg-[radial-gradient(circle_at_top_left,rgba(99,102,241,0.22),transparent_48%),radial-gradient(circle_at_top_right,rgba(6,182,212,0.18),transparent_42%),linear-gradient(180deg,rgba(255,255,255,0.04),transparent)]" />
					<CardHeader className="relative gap-3">
						<div className="flex flex-wrap items-start justify-between gap-3">
							<div className="space-y-2">
								<div className="flex flex-wrap items-center gap-2">
									<Badge
										variant="outline"
										className="border-sky-300 bg-sky-100/90 text-sky-900 dark:border-sky-500/60 dark:bg-sky-500/20 dark:text-sky-100"
									>
										管理仪表盘
									</Badge>
									<Badge variant="outline" className="bg-background/70">
										Rollup 已记录
									</Badge>
								</div>
								<CardTitle className="text-2xl tracking-tight md:text-[1.9rem]">
									运营总览与任务态势一屏收口
								</CardTitle>
								<CardDescription className="max-w-3xl text-sm leading-6">
									聚焦用户规模、今日活跃、翻译 / 智能摘要 /
									日报三条任务链路的执行态势， 并使用按天 rollup
									的趋势记录来支撑统计分析。
								</CardDescription>
							</div>
							<div className="flex flex-wrap items-center gap-2">
								<Badge variant="outline" className="bg-background/70">
									时区 {data?.time_zone ?? timeZone}
								</Badge>
								<Button
									size="sm"
									variant="secondary"
									disabled={refreshing}
									onClick={() => void loadDashboard("refresh")}
								>
									<RefreshCw
										className={cn(
											"mr-1.5 size-4",
											refreshing && "animate-spin",
										)}
									/>
									刷新
								</Button>
							</div>
						</div>
					</CardHeader>
					<CardContent className="relative space-y-4">
						<div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
							<StatCard
								label="用户总数"
								value={formatCount(data?.kpis.total_users)}
								description="累计注册并纳入后台可管理范围的账号总量。"
								accentClass="bg-gradient-to-br from-indigo-500/14 via-card to-card"
								icon={Users}
							/>
							<StatCard
								label="今日活跃用户"
								value={formatCount(data?.kpis.active_users_today)}
								description="按当前查看时区统计，当日发生过站内活动的用户数。"
								accentClass="bg-gradient-to-br from-cyan-500/12 via-card to-card"
								icon={Sparkles}
							/>
							<StatCard
								label="进行中任务"
								value={formatCount(data?.kpis.ongoing_tasks_total)}
								description={`排队 ${formatCount(data?.kpis.queued_tasks)} · 运行中 ${formatCount(data?.kpis.running_tasks)}`}
								accentClass="bg-gradient-to-br from-amber-500/14 via-card to-card"
								icon={Activity}
							/>
							<StatCard
								label="今日任务总量"
								value={formatCount(data?.today.total)}
								description={`成功 ${formatCount(data?.today.succeeded_total)} · 失败 ${formatCount(data?.today.failed_total)}`}
								accentClass="bg-gradient-to-br from-emerald-500/14 via-card to-card"
								icon={RefreshCw}
							/>
						</div>
						<div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
							<span>
								统计窗口 {data?.window_start ?? "-"} → {data?.window_end ?? "-"}
							</span>
							<span>·</span>
							<span>最近更新 {formatGeneratedAt(data?.generated_at)}</span>
						</div>
					</CardContent>
				</Card>

				<Card className="border-border/80 bg-card/88 shadow-sm">
					<CardHeader>
						<CardTitle>进行中链路</CardTitle>
						<CardDescription>
							以当前队列实时状态补充今日 rollup 视图，优先暴露运营阻塞点。
						</CardDescription>
					</CardHeader>
					<CardContent className="space-y-3">
						<div className="grid gap-3 sm:grid-cols-3 xl:grid-cols-1">
							{[
								{
									label: "翻译",
									value: data?.kpis.ongoing_by_task.translations ?? 0,
									description: "发布翻译批任务",
								},
								{
									label: "智能摘要",
									value: data?.kpis.ongoing_by_task.summaries ?? 0,
									description: "发布摘要批任务",
								},
								{
									label: "日报",
									value: data?.kpis.ongoing_by_task.briefs ?? 0,
									description: "日报生成任务",
								},
							].map((item) => (
								<div
									key={item.label}
									className="rounded-2xl border border-border/70 bg-muted/30 p-4"
								>
									<div className="flex items-center justify-between gap-3">
										<div>
											<p className="text-muted-foreground text-xs">
												{item.label}
											</p>
											<p
												className="mt-1 text-2xl font-semibold"
												style={{ color: chartTaskColor(item.label) }}
											>
												{formatCount(item.value)}
											</p>
										</div>
										<span
											className="size-3 rounded-full shadow-[0_0_24px_currentColor]"
											style={{
												color: chartTaskColor(item.label),
												backgroundColor: chartTaskColor(item.label),
											}}
										/>
									</div>
									<p className="text-muted-foreground mt-2 text-xs">
										{item.description}
									</p>
								</div>
							))}
						</div>
						<div className="rounded-2xl border border-border/70 bg-background/70 p-4">
							<div className="flex items-center justify-between gap-3">
								<div>
									<p className="text-muted-foreground text-xs">
										今日主视图焦点
									</p>
									<p className="mt-1 text-base font-semibold">
										{highestVolumeTask
											? `${highestVolumeTask.label} 任务量最高`
											: "暂无今日任务数据"}
									</p>
								</div>
								{highestVolumeTask ? (
									<Badge
										variant="outline"
										className="border-primary/20 bg-primary/8"
									>
										{formatPercent(highestVolumeTask.success_rate)} 成功率
									</Badge>
								) : null}
							</div>
							<div className="mt-3 flex flex-wrap gap-2">
								{(
									[
										"queued",
										"running",
										"succeeded",
										"failed",
										"canceled",
									] as const
								).map((status) => (
									<Badge
										key={status}
										variant="outline"
										className={statusBadgeClass(status)}
									>
										{STATUS_LABELS[status]}{" "}
										{formatCount(data?.today[`${status}_total` as const] ?? 0)}
									</Badge>
								))}
							</div>
						</div>
					</CardContent>
				</Card>
			</div>

			<div className="grid gap-4 2xl:grid-cols-[1.45fr_0.95fr]">
				<Card className="border-border/80 bg-card/88 shadow-sm">
					<CardHeader>
						<CardTitle>今日执行状态分布</CardTitle>
						<CardDescription>
							按任务类型拆分今日队列 / 运行 / 成功 / 失败 / 取消情况。
						</CardDescription>
					</CardHeader>
					<CardContent>
						<div className="h-[22rem]" data-admin-dashboard-chart-today="true">
							<ResponsiveContainer width="100%" height="100%">
								<BarChart data={todayChartData} barGap={10}>
									<CartesianGrid strokeDasharray="3 3" strokeOpacity={0.24} />
									<XAxis dataKey="label" tickLine={false} axisLine={false} />
									<YAxis
										tickLine={false}
										axisLine={false}
										allowDecimals={false}
									/>
									<Tooltip content={<CustomChartTooltip />} />
									<Legend />
									<Bar
										dataKey="queued"
										stackId="status"
										fill={STATUS_COLORS.queued}
										radius={[8, 8, 0, 0]}
										isAnimationActive={false}
									/>
									<Bar
										dataKey="running"
										stackId="status"
										fill={STATUS_COLORS.running}
										isAnimationActive={false}
									/>
									<Bar
										dataKey="succeeded"
										stackId="status"
										fill={STATUS_COLORS.succeeded}
										isAnimationActive={false}
									/>
									<Bar
										dataKey="failed"
										stackId="status"
										fill={STATUS_COLORS.failed}
										isAnimationActive={false}
									/>
									<Bar
										dataKey="canceled"
										stackId="status"
										fill={STATUS_COLORS.canceled}
										radius={[8, 8, 0, 0]}
										isAnimationActive={false}
									/>
								</BarChart>
							</ResponsiveContainer>
						</div>
					</CardContent>
				</Card>

				<Card className="border-border/80 bg-card/88 shadow-sm">
					<CardHeader>
						<CardTitle>今日任务占比</CardTitle>
						<CardDescription>
							快速判断今天压力主要落在哪条链路上。
						</CardDescription>
					</CardHeader>
					<CardContent className="space-y-4">
						<div className="h-[20rem]" data-admin-dashboard-chart-share="true">
							<ResponsiveContainer width="100%" height="100%">
								<PieChart>
									<Tooltip content={<CustomChartTooltip />} />
									<Pie
										data={shareChartData}
										dataKey="value"
										nameKey="name"
										cx="50%"
										cy="50%"
										innerRadius={62}
										outerRadius={98}
										paddingAngle={4}
										isAnimationActive={false}
									>
										{shareChartData.map((entry) => (
											<Cell key={`share-${entry.name}`} fill={entry.fill} />
										))}
									</Pie>
								</PieChart>
							</ResponsiveContainer>
						</div>
						<div className="grid gap-2">
							{shareChartData.map((item) => (
								<div
									key={item.name}
									className="flex items-center justify-between rounded-xl border border-border/70 bg-background/70 px-3 py-2 text-sm"
								>
									<div className="flex items-center gap-2">
										<span
											className="size-2.5 rounded-full"
											style={{ backgroundColor: item.fill }}
										/>
										<span>{item.name}</span>
									</div>
									<span className="font-medium">{formatCount(item.value)}</span>
								</div>
							))}
						</div>
					</CardContent>
				</Card>
			</div>

			<div className="grid gap-4 2xl:grid-cols-[1.45fr_0.95fr]">
				<Card className="border-border/80 bg-card/88 shadow-sm">
					<CardHeader>
						<CardTitle>近 7 日任务趋势</CardTitle>
						<CardDescription>
							趋势图基于每日 rollup，适合快速观察波峰与链路偏移。
						</CardDescription>
					</CardHeader>
					<CardContent>
						<div className="h-[22rem]" data-admin-dashboard-chart-trend="true">
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
												stopOpacity={0.3}
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
												stopOpacity={0.28}
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
												stopOpacity={0.3}
											/>
											<stop
												offset="95%"
												stopColor={TASK_COLORS.日报}
												stopOpacity={0}
											/>
										</linearGradient>
									</defs>
									<CartesianGrid strokeDasharray="3 3" strokeOpacity={0.24} />
									<XAxis dataKey="label" tickLine={false} axisLine={false} />
									<YAxis
										tickLine={false}
										axisLine={false}
										allowDecimals={false}
									/>
									<Tooltip content={<CustomChartTooltip />} />
									<Legend />
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

				<Card className="border-border/80 bg-card/88 shadow-sm">
					<CardHeader>
						<CardTitle>活跃用户与规模</CardTitle>
						<CardDescription>
							辅助判断活跃波动是否与任务量变化同步。
						</CardDescription>
					</CardHeader>
					<CardContent className="space-y-4">
						<div className="h-[16rem]" data-admin-dashboard-chart-active="true">
							<ResponsiveContainer width="100%" height="100%">
								<BarChart data={trendChartData}>
									<CartesianGrid strokeDasharray="3 3" strokeOpacity={0.24} />
									<XAxis dataKey="label" tickLine={false} axisLine={false} />
									<YAxis
										tickLine={false}
										axisLine={false}
										allowDecimals={false}
									/>
									<Tooltip content={<CustomChartTooltip />} />
									<Legend />
									<Bar
										dataKey="active_users"
										name="活跃用户"
										fill="#22c55e"
										radius={[8, 8, 0, 0]}
										isAnimationActive={false}
									/>
									<Bar
										dataKey="total_users"
										name="用户总量"
										fill="#a855f7"
										radius={[8, 8, 0, 0]}
										isAnimationActive={false}
									/>
								</BarChart>
							</ResponsiveContainer>
						</div>
						<div className="rounded-2xl border border-border/70 bg-background/70 p-4">
							<p className="text-muted-foreground text-xs">今日摘要</p>
							<div className="mt-3 grid gap-3 sm:grid-cols-2">
								<div>
									<p className="text-muted-foreground text-xs">活跃 / 总用户</p>
									<p className="mt-1 text-lg font-semibold">
										{formatCount(data?.kpis.active_users_today)} /{" "}
										{formatCount(data?.kpis.total_users)}
									</p>
								</div>
								<div>
									<p className="text-muted-foreground text-xs">实时进行中</p>
									<p className="mt-1 text-lg font-semibold">
										{formatCount(data?.kpis.ongoing_tasks_total)}
									</p>
								</div>
							</div>
						</div>
					</CardContent>
				</Card>
			</div>

			<Card className="border-border/80 bg-card/88 shadow-sm">
				<CardHeader>
					<CardTitle>今日任务 Stats</CardTitle>
					<CardDescription>
						保留稍高信息密度，方便直接对比各链路吞吐、异常与成功率。
					</CardDescription>
				</CardHeader>
				<CardContent>
					<div className="overflow-x-auto">
						<table className="w-full min-w-[760px] border-separate border-spacing-y-2">
							<thead>
								<tr className="text-left text-xs text-muted-foreground">
									<th className="px-3 py-2 font-medium">任务类型</th>
									<th className="px-3 py-2 font-medium">总量</th>
									<th className="px-3 py-2 font-medium">排队</th>
									<th className="px-3 py-2 font-medium">运行中</th>
									<th className="px-3 py-2 font-medium">成功</th>
									<th className="px-3 py-2 font-medium">失败</th>
									<th className="px-3 py-2 font-medium">成功率</th>
								</tr>
							</thead>
							<tbody>
								{(data?.today.task_status ?? []).map((item) => (
									<tr key={item.task_type} className="rounded-2xl bg-muted/25">
										<td className="rounded-l-2xl border-y border-l border-border/70 px-3 py-3">
											<div className="flex items-center gap-3">
												<span
													className="size-2.5 rounded-full"
													style={{
														backgroundColor: chartTaskColor(item.label),
													}}
												/>
												<div>
													<p className="font-medium">{item.label}</p>
													<p className="text-muted-foreground text-xs">
														{item.task_type}
													</p>
												</div>
											</div>
										</td>
										<td className="border-y border-border/70 px-3 py-3 font-medium">
											{formatCount(item.total)}
										</td>
										<td className="border-y border-border/70 px-3 py-3">
											{formatCount(item.queued)}
										</td>
										<td className="border-y border-border/70 px-3 py-3">
											{formatCount(item.running)}
										</td>
										<td className="border-y border-border/70 px-3 py-3">
											{formatCount(item.succeeded)}
										</td>
										<td className="border-y border-border/70 px-3 py-3">
											{formatCount(item.failed)}
										</td>
										<td className="rounded-r-2xl border-y border-r border-border/70 px-3 py-3 font-medium">
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
	);
}
