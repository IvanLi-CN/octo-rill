import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Settings2 } from "lucide-react";

import {
	ADMIN_JOBS_SUBSCRIPTIONS_PATH,
	ADMIN_SUBSCRIPTION_SETTINGS_AUTO_OPEN_SESSION_KEY,
} from "@/admin/jobsRouteState";
import {
	type AdminRepoGovernanceGridCell,
	type AdminRepoGovernanceListItem,
	type AdminRepoGovernanceListResponse,
	type AdminRepoGovernanceOverviewResponse,
	ApiError,
	apiGetAdminRepoGovernanceList,
	apiGetAdminRepoGovernanceOverview,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { InternalLink } from "@/lib/internalNavigation";
import { useOptionalTheme } from "@/theme/ThemeProvider";

const DATETIME_FORMATTER = new Intl.DateTimeFormat(undefined, {
	month: "2-digit",
	day: "2-digit",
	hour: "2-digit",
	minute: "2-digit",
	hour12: false,
});
const PAGE_SIZE = 60;
const GRID_BASE_GAP = 2;
const GRID_DENSE_GAP = 1;
const GRID_ULTRA_DENSE_GAP = 0;
const GRID_IDEAL_CELL = 8;
const GRID_MIN_CELL = 1;
const GRID_DENSE_CELL = 3;
const GRID_ULTRA_DENSE_CELL = 3;
const GRID_COMPACT_BLOCK_THRESHOLD = 8000;
const GRID_COMPACT_TARGET_ASPECT_RATIO = 3.2;
const GRID_COMPACT_MIN_WIDTH = 300;
const GRID_COMPACT_MAX_WIDTH = 420;
const GRID_HEIGHT_TARGET_RATIO = 0.52;
const GRID_ULTRA_DENSE_HEIGHT_TARGET_RATIO = 0.28;
const GRID_LEGEND_SKELETON_IDS = [
	"fresh",
	"warm",
	"aging",
	"stale",
	"missing",
] as const;
const REPO_ROW_SKELETON_IDS = ["one", "two", "three", "four"] as const;

const AGE_BUCKET_LABELS = {
	fresh: "4 小时内",
	warm: "4-12 小时",
	aging: "12-24 小时",
	stale: "超过 24 小时",
	missing: "暂无成功记录",
} as const;

type AgeBucketKey = keyof typeof AGE_BUCKET_LABELS;

function formatDateTime(value: string | null | undefined) {
	if (!value) return "-";
	const parsed = new Date(value);
	if (Number.isNaN(parsed.getTime())) return "-";
	return DATETIME_FORMATTER.format(parsed);
}

function formatPressure(value: number) {
	return Number.isFinite(value) ? value.toFixed(2) : "-";
}

function normalizeError(err: unknown) {
	if (err instanceof ApiError) return err.message;
	return err instanceof Error ? err.message : String(err);
}

function urgencyTone(bucket: string) {
	switch (bucket) {
		case "critical":
			return "border-rose-300 bg-rose-50 text-rose-700 dark:border-rose-400/40 dark:bg-rose-500/15 dark:text-rose-100";
		case "due":
			return "border-amber-300 bg-amber-50 text-amber-700 dark:border-amber-400/40 dark:bg-amber-500/15 dark:text-amber-100";
		default:
			return "border-emerald-300 bg-emerald-50 text-emerald-700 dark:border-emerald-400/40 dark:bg-emerald-500/15 dark:text-emerald-100";
	}
}

function urgencyLabel(bucket: string) {
	switch (bucket) {
		case "critical":
			return "紧急";
		case "due":
			return "临期";
		default:
			return "健康";
	}
}

function sourceLabel(source: string | null | undefined) {
	switch (source) {
		case "system":
			return "系统调度";
		case "interactive":
			return "交互刷新";
		case "manual":
			return "手动刷新";
		default:
			return source ?? "-";
	}
}

function ageBucketColor(bucket: string, isDark: boolean) {
	switch (bucket) {
		case "fresh":
			return isDark ? "#34d399" : "#10b981";
		case "warm":
			return isDark ? "#fbbf24" : "#f59e0b";
		case "aging":
			return isDark ? "#fb923c" : "#f97316";
		case "stale":
			return isDark ? "#fb7185" : "#e11d48";
		default:
			return isDark ? "#52525b" : "#d4d4d8";
	}
}

function fillSquare(
	context: CanvasRenderingContext2D,
	x: number,
	y: number,
	size: number,
	color: string,
) {
	context.fillStyle = color;
	context.fillRect(x, y, size, size);
}

function ErrorPanel(props: {
	title: string;
	message: string;
	actionLabel?: string;
	onRetry?: () => void;
}) {
	const { title, message, actionLabel = "重试", onRetry } = props;

	return (
		<div className="rounded-2xl border border-destructive/25 bg-destructive/[0.06] px-4 py-3 text-sm text-foreground">
			<p className="font-medium">{title}</p>
			<p className="mt-1 text-muted-foreground">{message}</p>
			{onRetry ? (
				<Button
					type="button"
					variant="outline"
					size="sm"
					className="mt-3 border-destructive/20 bg-background/80 hover:bg-background"
					onClick={onRetry}
				>
					{actionLabel}
				</Button>
			) : null}
		</div>
	);
}

function OverviewMetricCard(props: {
	label: string;
	value: string;
	loading?: boolean;
}) {
	const { label, value, loading = false } = props;

	return (
		<div className="rounded-2xl border border-border/70 bg-card/78 p-4">
			<p className="text-muted-foreground text-xs">{label}</p>
			{loading ? (
				<div className="mt-3 h-8 w-24 animate-pulse rounded-md bg-muted/70" />
			) : (
				<p className="mt-2 font-semibold text-2xl text-foreground">{value}</p>
			)}
		</div>
	);
}

function GovernanceGrid(props: {
	cells: AdminRepoGovernanceGridCell[];
	isDark: boolean;
}) {
	const { cells, isDark } = props;
	const hostRef = useRef<HTMLDivElement | null>(null);
	const canvasRef = useRef<HTMLCanvasElement | null>(null);
	const [hostWidth, setHostWidth] = useState(0);

	useEffect(() => {
		const host = hostRef.current;
		if (!host) return;

		const measure = () => {
			const nextWidth = Math.max(0, Math.floor(host.clientWidth));
			setHostWidth((current) => (current === nextWidth ? current : nextWidth));
		};

		measure();

		const observer = new ResizeObserver(() => {
			measure();
		});
		observer.observe(host);

		return () => {
			observer.disconnect();
		};
	}, []);

	useEffect(() => {
		const canvas = canvasRef.current;
		if (!canvas || hostWidth <= 0) return;

		const dpr = window.devicePixelRatio || 1;
		const totalCells = Math.max(1, cells.length);
		const isCompactBlock = totalCells >= GRID_COMPACT_BLOCK_THRESHOLD;
		const densityRatio = Math.min(1, totalCells / 4000);
		const isUltraDense = !isCompactBlock && totalCells >= 8000;
		const isDense = !isUltraDense && densityRatio > 0.35;
		let gap: number;
		let columns: number;
		let cell: number;
		let width: number;
		let height: number;

		if (isCompactBlock) {
			gap = GRID_ULTRA_DENSE_GAP;
			columns = Math.max(
				1,
				Math.ceil(Math.sqrt(totalCells * GRID_COMPACT_TARGET_ASPECT_RATIO)),
			);
			const targetWidth = Math.min(
				hostWidth,
				Math.max(
					GRID_COMPACT_MIN_WIDTH,
					Math.min(GRID_COMPACT_MAX_WIDTH, hostWidth * 0.48),
				),
			);
			cell = Math.max(GRID_MIN_CELL, targetWidth / columns);
			width = columns * cell;
			const rows = Math.max(1, Math.ceil(cells.length / columns));
			height = rows * cell;
		} else {
			gap = isUltraDense
				? GRID_ULTRA_DENSE_GAP
				: isDense
					? GRID_DENSE_GAP
					: GRID_BASE_GAP;
			const compactCell = isUltraDense
				? GRID_ULTRA_DENSE_CELL
				: isDense
					? GRID_DENSE_CELL
					: GRID_DENSE_CELL;
			const targetHeightRatio = isUltraDense
				? GRID_ULTRA_DENSE_HEIGHT_TARGET_RATIO
				: GRID_HEIGHT_TARGET_RATIO;
			const minimumColumns = Math.max(
				1,
				Math.floor((hostWidth + gap) / (compactCell + gap)),
			);
			const targetColumnsFromArea = Math.ceil(
				Math.sqrt(
					totalCells * (hostWidth / Math.max(1, hostWidth * targetHeightRatio)),
				),
			);
			const preferredColumns = Math.max(
				1,
				Math.floor((hostWidth + gap) / (GRID_IDEAL_CELL + gap)),
			);
			columns = Math.max(
				Math.min(totalCells, minimumColumns),
				Math.min(totalCells, Math.max(preferredColumns, targetColumnsFromArea)),
			);
			const rawCell =
				columns > 1 ? (hostWidth - (columns - 1) * gap) / columns : hostWidth;
			cell = Math.max(GRID_MIN_CELL, rawCell);
			const rows = Math.max(1, Math.ceil(cells.length / columns));
			width = hostWidth;
			height = rows * cell + (rows - 1) * gap;
		}
		canvas.width = Math.round(width * dpr);
		canvas.height = Math.round(height * dpr);
		canvas.style.width = `${width}px`;
		canvas.style.height = `${height}px`;

		const context = canvas.getContext("2d");
		if (!context) return;
		context.setTransform(dpr, 0, 0, dpr, 0, 0);
		context.clearRect(0, 0, width, height);

		cells.forEach((cellItem, index) => {
			const column = index % columns;
			const row = Math.floor(index / columns);
			const x = column * (cell + gap);
			const y = row * (cell + gap);
			fillSquare(
				context,
				x,
				y,
				cell,
				ageBucketColor(cellItem.age_bucket, isDark),
			);
		});
	}, [cells, hostWidth, isDark]);

	return (
		<div ref={hostRef} className="w-full">
			<canvas
				ref={canvasRef}
				className="block"
				tabIndex={-1}
				title="颜色按实际最后成功刷新时间分桶；顺序按治理优先级。"
			/>
		</div>
	);
}

function RepoRow(props: { item: AdminRepoGovernanceListItem }) {
	const { item } = props;

	return (
		<div className="rounded-2xl border border-border/70 bg-card/70 p-4 transition-colors duration-200 hover:bg-card/90">
			<div className="min-w-0 space-y-2">
				<div className="flex flex-wrap items-center gap-2">
					<p className="truncate font-medium text-sm text-foreground">
						{item.repo_full_name}
					</p>
					<Badge variant="outline" className={urgencyTone(item.urgency_bucket)}>
						{urgencyLabel(item.urgency_bucket)}
					</Badge>
				</div>
				<dl className="flex flex-wrap gap-x-4 gap-y-2 text-sm">
					<div className="inline-flex items-baseline gap-1.5">
						<dt className="text-muted-foreground">排序</dt>
						<dd className="font-semibold text-foreground">
							#{item.priority_rank}
						</dd>
					</div>
					<div className="inline-flex items-baseline gap-1.5">
						<dt className="text-muted-foreground">目标窗口</dt>
						<dd className="font-semibold text-foreground">
							W{item.target_window} · {item.target_interval_minutes} 分钟
						</dd>
					</div>
					<div className="inline-flex items-baseline gap-1.5">
						<dt className="text-muted-foreground">迫切值</dt>
						<dd className="font-semibold text-foreground">
							{item.urgency_score.toFixed(2)}
						</dd>
					</div>
				</dl>
				<dl className="flex flex-wrap gap-x-4 gap-y-2 text-xs">
					<div className="inline-flex items-baseline gap-1.5">
						<dt className="text-muted-foreground">关注人数</dt>
						<dd className="font-medium text-foreground">
							{item.watcher_user_count}
						</dd>
					</div>
					<div className="inline-flex items-baseline gap-1.5">
						<dt className="text-muted-foreground">关注仓库累加</dt>
						<dd className="font-medium text-foreground">
							{item.watcher_repo_total_sum}
						</dd>
					</div>
					<div className="inline-flex items-baseline gap-1.5">
						<dt className="text-muted-foreground">星标缓存</dt>
						<dd className="font-medium text-foreground">
							{item.cached_stargazer_count ?? "-"}
						</dd>
					</div>
				</dl>
				<p className="text-muted-foreground text-xs">
					系统刷新 {formatDateTime(item.system_last_success_at)} · 实际刷新{" "}
					{formatDateTime(item.actual_last_success_at)} · 最近来源{" "}
					{sourceLabel(item.actual_last_success_source)}
				</p>
			</div>
		</div>
	);
}

export function AdminRepoGovernance() {
	const theme = useOptionalTheme();
	const isDark = theme?.resolvedTheme === "dark";
	const [overview, setOverview] =
		useState<AdminRepoGovernanceOverviewResponse | null>(null);
	const [overviewLoading, setOverviewLoading] = useState(true);
	const [overviewError, setOverviewError] = useState<string | null>(null);
	const [list, setList] = useState<AdminRepoGovernanceListResponse | null>(
		null,
	);
	const [listLoading, setListLoading] = useState(true);
	const [listError, setListError] = useState<string | null>(null);
	const [queryInput, setQueryInput] = useState("");
	const [query, setQuery] = useState("");
	const [aging, setAging] = useState<"all" | "stale" | "missing">("all");
	const [page, setPage] = useState(1);

	const loadOverview = useCallback(async () => {
		setOverviewLoading(true);
		setOverviewError(null);
		try {
			const overviewRes = await apiGetAdminRepoGovernanceOverview();
			setOverview(overviewRes);
		} catch (err) {
			setOverviewError(normalizeError(err));
		} finally {
			setOverviewLoading(false);
		}
	}, []);

	const loadList = useCallback(
		async (options?: { background?: boolean }) => {
			if (!options?.background || !list) {
				setListLoading(true);
			}
			setListError(null);
			try {
				const params = new URLSearchParams();
				params.set("page", String(page));
				params.set("page_size", String(PAGE_SIZE));
				if (query.trim()) params.set("query", query.trim());
				if (aging !== "all") params.set("aging", aging);
				const listRes = await apiGetAdminRepoGovernanceList(params);
				setList(listRes);
			} catch (err) {
				setListError(normalizeError(err));
			} finally {
				setListLoading(false);
			}
		},
		[aging, list, page, query],
	);

	useEffect(() => {
		let cancelled = false;
		setOverviewLoading(true);
		setOverviewError(null);

		void (async () => {
			try {
				const overviewRes = await apiGetAdminRepoGovernanceOverview();
				if (cancelled) return;
				setOverview(overviewRes);
			} catch (err) {
				if (cancelled) return;
				setOverviewError(normalizeError(err));
			} finally {
				if (!cancelled) setOverviewLoading(false);
			}
		})();

		return () => {
			cancelled = true;
		};
	}, []);

	useEffect(() => {
		let cancelled = false;
		if (!list) setListLoading(true);
		setListError(null);

		void (async () => {
			try {
				const params = new URLSearchParams();
				params.set("page", String(page));
				params.set("page_size", String(PAGE_SIZE));
				if (query.trim()) params.set("query", query.trim());
				if (aging !== "all") params.set("aging", aging);
				const listRes = await apiGetAdminRepoGovernanceList(params);
				if (cancelled) return;
				setList(listRes);
			} catch (err) {
				if (cancelled) return;
				setListError(normalizeError(err));
			} finally {
				if (!cancelled) setListLoading(false);
			}
		})();

		return () => {
			cancelled = true;
		};
	}, [aging, list, page, query]);

	const totalPages = Math.max(1, Math.ceil((list?.total ?? 0) / PAGE_SIZE));
	const gridSummary = useMemo(() => {
		const counts: Record<AgeBucketKey, number> = {
			fresh: 0,
			warm: 0,
			aging: 0,
			stale: 0,
			missing: 0,
		};

		for (const cell of overview?.grid_cells ?? []) {
			const bucket = (
				cell.age_bucket in counts ? cell.age_bucket : "missing"
			) as AgeBucketKey;
			counts[bucket] += 1;
		}

		const entries = (Object.keys(AGE_BUCKET_LABELS) as AgeBucketKey[]).map(
			(bucket) => ({
				bucket,
				label: AGE_BUCKET_LABELS[bucket],
				count: counts[bucket],
				tone: ageBucketColor(bucket, isDark),
			}),
		);

		return {
			total: (overview?.grid_cells ?? []).length,
			entries,
			sentence:
				entries.length === 0
					? "活动图暂无仓库。"
					: `活动图共 ${entries.reduce((sum, entry) => sum + entry.count, 0)} 个仓库；${entries
							.map((entry) => `${entry.label} ${entry.count} 个`)
							.join("，")}。方块顺序按治理优先级排列。`,
		};
	}, [isDark, overview?.grid_cells]);

	function onApplyFilters() {
		setPage(1);
		setQuery(queryInput.trim());
	}

	function onOpenBudgetSettings() {
		try {
			window.sessionStorage.setItem(
				ADMIN_SUBSCRIPTION_SETTINGS_AUTO_OPEN_SESSION_KEY,
				"1",
			);
		} catch {
			// ignore storage failures; route jump still works
		}
	}

	return (
		<div className="space-y-4">
			<Card
				className="border-border/70 bg-[radial-gradient(circle_at_top_left,color-mix(in_oklch,var(--color-primary)_8%,transparent),transparent_42%),linear-gradient(180deg,color-mix(in_oklch,var(--color-card)_96%,transparent),color-mix(in_oklch,var(--color-card)_82%,var(--color-background)))] shadow-sm"
				aria-busy={overviewLoading}
			>
				<CardHeader>
					<CardTitle>仓库刷新治理</CardTitle>
				</CardHeader>
				<CardContent className="space-y-4">
					<div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
						<p className="max-w-3xl text-muted-foreground text-sm leading-6">
							这里展示有效关注池、系统预算、闭环批次和仓库老化。交互刷新会更新实际新鲜度，但不会提前完成系统目标窗口。
						</p>
						<Button
							type="button"
							variant="outline"
							size="icon"
							className="border-border/70 bg-background/75 self-start"
							asChild
						>
							<InternalLink
								href={ADMIN_JOBS_SUBSCRIPTIONS_PATH}
								to={ADMIN_JOBS_SUBSCRIPTIONS_PATH}
								onClick={onOpenBudgetSettings}
								aria-label={`打开订阅同步设置，当前系统预算 ${
									overview?.settings.repo_refresh_system_budget_per_window ??
									"-"
								} / 10 分钟`}
								title={`系统预算 ${
									overview?.settings.repo_refresh_system_budget_per_window ??
									"-"
								} / 10 分钟`}
							>
								<Settings2 />
								<span className="sr-only">打开订阅同步设置</span>
							</InternalLink>
						</Button>
					</div>

					{overviewError ? (
						<ErrorPanel
							title="治理概览加载失败"
							message={overviewError}
							actionLabel="重试概览"
							onRetry={() => void loadOverview()}
						/>
					) : null}

					<div className="grid gap-3 sm:grid-cols-3">
						<OverviewMetricCard
							label="去重仓库数"
							value={String(overview?.summary.dedup_repo_count ?? "-")}
							loading={overviewLoading && !overview}
						/>
						<OverviewMetricCard
							label="压力值"
							value={
								overview
									? formatPressure(overview.summary.pressure_windows)
									: "-"
							}
							loading={overviewLoading && !overview}
						/>
						<OverviewMetricCard
							label="上次全量闭环"
							value={formatDateTime(
								overview?.summary.last_full_cycle_completed_at,
							)}
							loading={overviewLoading && !overview}
						/>
					</div>

					<div className="rounded-2xl border border-border/70 bg-card/72 p-4">
						<div className="mb-3 flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
							<div>
								<p className="font-medium text-sm text-foreground">活动图</p>
								<p
									id="repo-governance-grid-summary"
									className="text-muted-foreground text-xs leading-5"
								>
									颜色看实际刷新时间；W* 看系统目标窗口；迫切值大于 1
									表示已超出目标窗口。
								</p>
								<p className="sr-only">{gridSummary.sentence}</p>
							</div>
							<div className="rounded-xl border border-border/70 bg-background/75 px-3 py-2 text-xs">
								<p className="text-muted-foreground">
									当前闭环批次：{overview?.cycle.active_cycle_id ?? "-"}
								</p>
								<p className="mt-1 text-muted-foreground">
									进度 {overview?.cycle.active_cycle_completed_count ?? 0}/
									{overview?.cycle.active_cycle_repo_count ?? 0} · 开始于{" "}
									{formatDateTime(overview?.cycle.active_cycle_started_at)}
								</p>
							</div>
						</div>

						{overviewLoading && !overview ? (
							<div className="space-y-3">
								<div className="h-24 w-full animate-pulse bg-muted/60" />
								<div className="flex flex-wrap gap-2">
									{GRID_LEGEND_SKELETON_IDS.map((id) => (
										<div
											key={`grid-legend-skeleton-${id}`}
											className="h-8 w-28 animate-pulse rounded-full bg-muted/60"
										/>
									))}
								</div>
							</div>
						) : (
							<figure
								aria-labelledby="repo-governance-grid-summary"
								className="space-y-3"
							>
								<GovernanceGrid
									cells={overview?.grid_cells ?? []}
									isDark={isDark}
								/>
								<ul
									className="flex flex-wrap gap-2"
									aria-label="活动图颜色图例"
								>
									{gridSummary.entries.map((entry) => (
										<li
											key={entry.bucket}
											className="inline-flex items-center gap-2 rounded-full border border-border/70 bg-background/75 px-3 py-1.5 text-xs text-foreground"
										>
											<span
												className="size-2.5 rounded-full"
												style={{ backgroundColor: entry.tone }}
												aria-hidden="true"
											/>
											<span>
												{entry.label} · {entry.count}
											</span>
										</li>
									))}
									<li className="inline-flex items-center rounded-full border border-dashed border-border/70 bg-muted/20 px-3 py-1.5 text-xs text-muted-foreground">
										总计 {gridSummary.total} 个仓库
									</li>
								</ul>
							</figure>
						)}
					</div>
				</CardContent>
			</Card>

			<Card aria-busy={listLoading}>
				<CardHeader>
					<CardTitle>仓库明细</CardTitle>
				</CardHeader>
				<CardContent className="space-y-4">
					<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
						<Input
							value={queryInput}
							onChange={(event) => setQueryInput(event.target.value)}
							onKeyDown={(event) => {
								if (event.key === "Enter") onApplyFilters();
							}}
							placeholder="搜索仓库全名"
							className="sm:max-w-sm"
						/>
						<div className="flex flex-wrap items-center gap-2">
							<Button
								type="button"
								variant={aging === "all" ? "default" : "outline"}
								size="sm"
								onClick={() => {
									setPage(1);
									setAging("all");
								}}
							>
								全部
							</Button>
							<Button
								type="button"
								variant={aging === "stale" ? "default" : "outline"}
								size="sm"
								onClick={() => {
									setPage(1);
									setAging("stale");
								}}
							>
								仅超 24 小时
							</Button>
							<Button
								type="button"
								variant={aging === "missing" ? "default" : "outline"}
								size="sm"
								onClick={() => {
									setPage(1);
									setAging("missing");
								}}
							>
								仅未成功
							</Button>
							<Button type="button" variant="outline" onClick={onApplyFilters}>
								筛选
							</Button>
						</div>
					</div>
					<div className="flex flex-wrap items-center justify-between gap-2 text-xs">
						<p className="text-muted-foreground">
							先看排序、目标窗口和迫切值，再补充实际刷新时间。
						</p>
						{listLoading && list ? (
							<p className="text-muted-foreground inline-flex items-center gap-2">
								<span className="size-2 rounded-full bg-amber-500/80" />
								明细更新中…
							</p>
						) : null}
					</div>

					{listError ? (
						<ErrorPanel
							title="仓库明细加载失败"
							message={listError}
							actionLabel="重试明细"
							onRetry={() => void loadList()}
						/>
					) : null}

					{listLoading && !list ? (
						<div className="space-y-3">
							{REPO_ROW_SKELETON_IDS.map((id) => (
								<div
									key={`repo-row-skeleton-${id}`}
									className="h-28 animate-pulse rounded-2xl border border-border/70 bg-muted/45"
								/>
							))}
						</div>
					) : (
						<div className="space-y-3">
							{(list?.items ?? []).map((item) => (
								<RepoRow key={item.repo_id} item={item} />
							))}
							{(list?.items ?? []).length === 0 ? (
								<p className="rounded-2xl border border-dashed border-border/70 bg-muted/20 px-4 py-6 text-center text-muted-foreground text-sm">
									没有匹配的仓库。
								</p>
							) : null}
							<div className="flex items-center justify-between gap-3 border-t border-border/70 pt-3 text-sm">
								<p className="text-muted-foreground">
									共 {list?.total ?? 0} 个仓库 · 第 {page}/{totalPages} 页
								</p>
								<div className="flex gap-2">
									<Button
										type="button"
										variant="outline"
										size="sm"
										disabled={page <= 1 || listLoading}
										onClick={() =>
											setPage((current) => Math.max(1, current - 1))
										}
									>
										上一页
									</Button>
									<Button
										type="button"
										variant="outline"
										size="sm"
										disabled={page >= totalPages || listLoading}
										onClick={() =>
											setPage((current) => Math.min(totalPages, current + 1))
										}
									>
										下一页
									</Button>
								</div>
							</div>
						</div>
					)}
				</CardContent>
			</Card>
		</div>
	);
}
