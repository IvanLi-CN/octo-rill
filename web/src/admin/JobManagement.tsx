import { ChevronDown } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import {
	type AdminJobsOverviewResponse,
	type AdminRealtimeTaskDetailResponse,
	type AdminRealtimeTaskItem,
	type AdminScheduledSlotItem,
	ApiError,
	apiCancelAdminRealtimeTask,
	apiGetAdminJobsOverview,
	apiGetAdminRealtimeTaskDetail,
	apiGetAdminRealtimeTasks,
	apiGetAdminScheduledSlots,
	apiPatchAdminScheduledSlot,
	apiRetryAdminRealtimeTask,
} from "@/api";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

const HM_FORMATTER = new Intl.DateTimeFormat(undefined, {
	hour: "2-digit",
	minute: "2-digit",
	hour12: false,
});

function formatLocalHm(value: string | null | undefined) {
	if (!value) return "-";
	const parsed = new Date(value);
	if (Number.isNaN(parsed.getTime())) {
		return "-";
	}
	return HM_FORMATTER.format(parsed);
}

function formatUtcHourToLocal(hourUtc: number) {
	const parsed = new Date(Date.UTC(1970, 0, 1, hourUtc, 0, 0));
	if (Number.isNaN(parsed.getTime())) {
		return "-";
	}
	return HM_FORMATTER.format(parsed);
}

function normalizeErrorMessage(err: unknown) {
	if (err instanceof ApiError) {
		switch (err.code) {
			case "forbidden_admin_only":
				return "当前账号没有管理员权限。";
			case "not_found":
				return "目标任务不存在。";
			case "invalid_task_state":
				return "当前任务状态不允许执行该操作。";
			default:
				return err.message;
		}
	}
	return err instanceof Error ? err.message : String(err);
}

type RealtimeStatusFilter =
	| "all"
	| "queued"
	| "running"
	| "failed"
	| "succeeded"
	| "canceled";

const TASK_PAGE_SIZE = 20;

type JobManagementProps = {
	currentUserId: number;
};

export function JobManagement({ currentUserId }: JobManagementProps) {
	const [tab, setTab] = useState<"realtime" | "scheduled">("realtime");
	const [overview, setOverview] = useState<AdminJobsOverviewResponse | null>(
		null,
	);

	const [statusFilter, setStatusFilter] = useState<RealtimeStatusFilter>("all");
	const [tasks, setTasks] = useState<AdminRealtimeTaskItem[]>([]);
	const [taskTotal, setTaskTotal] = useState(0);
	const [taskPage, setTaskPage] = useState(1);
	const [tasksLoading, setTasksLoading] = useState(false);
	const [taskActionBusyId, setTaskActionBusyId] = useState<string | null>(null);

	const [scheduledSlots, setScheduledSlots] = useState<
		AdminScheduledSlotItem[]
	>([]);
	const [scheduledLoading, setScheduledLoading] = useState(false);
	const [scheduledBusyHour, setScheduledBusyHour] = useState<number | null>(
		null,
	);

	const [detailTask, setDetailTask] =
		useState<AdminRealtimeTaskDetailResponse | null>(null);
	const [detailLoading, setDetailLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const taskTotalPages = useMemo(
		() => Math.max(1, Math.ceil(taskTotal / TASK_PAGE_SIZE)),
		[taskTotal],
	);

	const loadOverview = useCallback(async () => {
		const res = await apiGetAdminJobsOverview();
		setOverview(res);
	}, []);

	const loadRealtimeTasks = useCallback(async () => {
		setTasksLoading(true);
		try {
			const params = new URLSearchParams();
			params.set("status", statusFilter);
			params.set("page", String(taskPage));
			params.set("page_size", String(TASK_PAGE_SIZE));
			const res = await apiGetAdminRealtimeTasks(params);
			setTasks(res.items);
			setTaskTotal(res.total);
		} finally {
			setTasksLoading(false);
		}
	}, [statusFilter, taskPage]);

	const loadScheduled = useCallback(async () => {
		setScheduledLoading(true);
		try {
			const res = await apiGetAdminScheduledSlots();
			setScheduledSlots(res.items);
		} finally {
			setScheduledLoading(false);
		}
	}, []);

	const loadAll = useCallback(async () => {
		setError(null);
		try {
			await Promise.all([loadOverview(), loadRealtimeTasks(), loadScheduled()]);
		} catch (err) {
			setError(normalizeErrorMessage(err));
		}
	}, [loadOverview, loadRealtimeTasks, loadScheduled]);

	useEffect(() => {
		void loadAll();
	}, [loadAll]);

	useEffect(() => {
		setError(null);
		void loadRealtimeTasks().catch((err) => {
			setError(normalizeErrorMessage(err));
		});
	}, [loadRealtimeTasks]);

	const onOpenTaskDetail = useCallback(async (taskId: string) => {
		setDetailLoading(true);
		setError(null);
		try {
			const detail = await apiGetAdminRealtimeTaskDetail(taskId);
			setDetailTask(detail);
		} catch (err) {
			setError(normalizeErrorMessage(err));
		} finally {
			setDetailLoading(false);
		}
	}, []);

	const onRetryTask = useCallback(
		async (taskId: string) => {
			setTaskActionBusyId(taskId);
			setError(null);
			try {
				await apiRetryAdminRealtimeTask(taskId);
				await Promise.all([loadOverview(), loadRealtimeTasks()]);
			} catch (err) {
				setError(normalizeErrorMessage(err));
			} finally {
				setTaskActionBusyId(null);
			}
		},
		[loadOverview, loadRealtimeTasks],
	);

	const onCancelTask = useCallback(
		async (taskId: string) => {
			setTaskActionBusyId(taskId);
			setError(null);
			try {
				await apiCancelAdminRealtimeTask(taskId);
				await Promise.all([loadOverview(), loadRealtimeTasks()]);
			} catch (err) {
				setError(normalizeErrorMessage(err));
			} finally {
				setTaskActionBusyId(null);
			}
		},
		[loadOverview, loadRealtimeTasks],
	);

	const onToggleScheduledSlot = useCallback(
		async (slot: AdminScheduledSlotItem) => {
			setScheduledBusyHour(slot.hour_utc);
			setError(null);
			try {
				await apiPatchAdminScheduledSlot(slot.hour_utc, !slot.enabled);
				await Promise.all([loadOverview(), loadScheduled()]);
			} catch (err) {
				setError(normalizeErrorMessage(err));
			} finally {
				setScheduledBusyHour(null);
			}
		},
		[loadOverview, loadScheduled],
	);

	return (
		<div className="space-y-4">
			<Card>
				<CardHeader>
					<CardTitle>任务总览</CardTitle>
					<CardDescription>
						展示实时异步任务与定时日报槽位状态。
					</CardDescription>
				</CardHeader>
				<CardContent className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
					<div className="bg-card/70 rounded-lg border p-3">
						<p className="text-muted-foreground text-xs">队列中</p>
						<p className="mt-1 text-xl font-semibold">
							{overview?.queued ?? "-"}
						</p>
					</div>
					<div className="bg-card/70 rounded-lg border p-3">
						<p className="text-muted-foreground text-xs">运行中</p>
						<p className="mt-1 text-xl font-semibold">
							{overview?.running ?? "-"}
						</p>
					</div>
					<div className="bg-card/70 rounded-lg border p-3">
						<p className="text-muted-foreground text-xs">近24h 成功 / 失败</p>
						<p className="mt-1 text-xl font-semibold">
							{overview?.succeeded_24h ?? "-"} / {overview?.failed_24h ?? "-"}
						</p>
					</div>
					<div className="bg-card/70 rounded-lg border p-3 sm:col-span-2 lg:col-span-3">
						<p className="text-muted-foreground text-xs">
							定时日报槽位启用情况
						</p>
						<p className="mt-1 text-xl font-semibold">
							{overview?.enabled_scheduled_slots ?? "-"} /{" "}
							{overview?.total_scheduled_slots ?? 24}
						</p>
					</div>
				</CardContent>
			</Card>

			<div className="flex flex-wrap items-center gap-2">
				<Button
					variant={tab === "realtime" ? "default" : "outline"}
					size="sm"
					className="font-mono text-xs"
					onClick={() => setTab("realtime")}
				>
					实时异步任务
				</Button>
				<Button
					variant={tab === "scheduled" ? "default" : "outline"}
					size="sm"
					className="font-mono text-xs"
					onClick={() => setTab("scheduled")}
				>
					定时任务
				</Button>
				<Button
					variant="secondary"
					size="sm"
					disabled={tasksLoading || scheduledLoading}
					onClick={() => void loadAll()}
				>
					刷新
				</Button>
			</div>

			{error ? <p className="text-destructive text-sm">{error}</p> : null}

			{tab === "realtime" ? (
				<Card>
					<CardHeader>
						<CardTitle>实时异步任务</CardTitle>
						<CardDescription>
							监控系统内部任务，并支持重试与取消。
						</CardDescription>
					</CardHeader>
					<CardContent className="space-y-4">
						<div className="flex flex-wrap items-center gap-2">
							<div className="relative w-full max-w-xs">
								<select
									value={statusFilter}
									onChange={(e) => {
										setTaskPage(1);
										setStatusFilter(e.target.value as RealtimeStatusFilter);
									}}
									className="bg-background h-9 w-full appearance-none rounded-md border pl-3 pr-10 text-sm outline-none"
								>
									<option value="all">状态：全部</option>
									<option value="queued">状态：排队</option>
									<option value="running">状态：运行中</option>
									<option value="failed">状态：失败</option>
									<option value="succeeded">状态：成功</option>
									<option value="canceled">状态：取消</option>
								</select>
								<ChevronDown className="text-muted-foreground pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2" />
							</div>
							<span className="text-muted-foreground text-xs">
								共 {taskTotal} 个任务 · 当前用户 #{currentUserId}
							</span>
						</div>

						<div className="space-y-2">
							{tasksLoading ? (
								<p className="text-muted-foreground text-sm">正在加载任务...</p>
							) : tasks.length === 0 ? (
								<p className="text-muted-foreground text-sm">暂无任务。</p>
							) : (
								tasks.map((task) => {
									const busy = taskActionBusyId === task.id;
									return (
										<div
											key={task.id}
											className="bg-card/70 flex flex-col gap-3 rounded-lg border p-3 lg:flex-row lg:items-center lg:justify-between"
										>
											<div className="min-w-0">
												<p className="font-medium text-sm">{task.task_type}</p>
												<p className="text-muted-foreground mt-1 truncate font-mono text-[11px]">
													ID: {task.id}
												</p>
												<p className="text-muted-foreground text-xs">
													状态 {task.status}
													{task.cancel_requested ? " · 已请求取消" : ""}· 创建{" "}
													{formatLocalHm(task.created_at)}· 完成{" "}
													{formatLocalHm(task.finished_at)}
												</p>
												{task.error_message ? (
													<p className="text-destructive mt-1 text-xs">
														{task.error_message}
													</p>
												) : null}
											</div>
											<div className="flex flex-wrap gap-2">
												<Button
													variant="outline"
													disabled={detailLoading}
													onClick={() => void onOpenTaskDetail(task.id)}
												>
													详情
												</Button>
												<Button
													variant="outline"
													disabled={
														busy ||
														task.status === "queued" ||
														task.status === "running"
													}
													onClick={() => void onRetryTask(task.id)}
												>
													重试
												</Button>
												<Button
													variant="destructive"
													disabled={
														busy ||
														task.status === "succeeded" ||
														task.status === "failed" ||
														task.status === "canceled"
													}
													onClick={() => void onCancelTask(task.id)}
												>
													取消
												</Button>
											</div>
										</div>
									);
								})
							)}
						</div>

						<div className="flex items-center justify-between">
							<p className="text-muted-foreground text-xs">
								第 {taskPage}/{taskTotalPages} 页
							</p>
							<div className="flex gap-2">
								<Button
									variant="outline"
									size="sm"
									disabled={taskPage <= 1 || tasksLoading}
									onClick={() => setTaskPage((prev) => Math.max(1, prev - 1))}
								>
									上一页
								</Button>
								<Button
									variant="outline"
									size="sm"
									disabled={taskPage >= taskTotalPages || tasksLoading}
									onClick={() =>
										setTaskPage((prev) => Math.min(taskTotalPages, prev + 1))
									}
								>
									下一页
								</Button>
							</div>
						</div>
					</CardContent>
				</Card>
			) : null}

			{tab === "scheduled" ? (
				<Card>
					<CardHeader>
						<CardTitle>定时任务（24小时槽）</CardTitle>
						<CardDescription>
							固定 24 个 UTC 小时槽，命中槽位后收集用户并串行生成日报。
						</CardDescription>
					</CardHeader>
					<CardContent className="space-y-2">
						{scheduledLoading ? (
							<p className="text-muted-foreground text-sm">
								正在加载定时槽位...
							</p>
						) : (
							scheduledSlots.map((slot) => (
								<div
									key={slot.hour_utc}
									className="bg-card/70 flex flex-col gap-2 rounded-lg border p-3 md:flex-row md:items-center md:justify-between"
								>
									<div>
										<p className="font-medium text-sm">
											UTC {slot.hour_utc.toString().padStart(2, "0")}:00 · 本地{" "}
											{formatUtcHourToLocal(slot.hour_utc)}
										</p>
										<p className="text-muted-foreground text-xs">
											最近调度 {formatLocalHm(slot.last_dispatch_at)}· 更新时间{" "}
											{formatLocalHm(slot.updated_at)}
										</p>
									</div>
									<Button
										variant={slot.enabled ? "destructive" : "secondary"}
										disabled={scheduledBusyHour === slot.hour_utc}
										onClick={() => void onToggleScheduledSlot(slot)}
									>
										{slot.enabled ? "停用" : "启用"}
									</Button>
								</div>
							))
						)}
					</CardContent>
				</Card>
			) : null}

			{detailTask ? (
				<div className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4">
					<div className="bg-card w-full max-w-2xl rounded-xl border p-5 shadow-2xl">
						<div className="flex items-center justify-between gap-3">
							<div>
								<h3 className="text-lg font-semibold tracking-tight">
									任务详情
								</h3>
								<p className="text-muted-foreground font-mono text-xs">
									{detailTask.task.id}
								</p>
							</div>
							<Button variant="outline" onClick={() => setDetailTask(null)}>
								关闭
							</Button>
						</div>
						<p className="text-muted-foreground mt-2 text-sm">
							类型 {detailTask.task.task_type} · 状态 {detailTask.task.status}
						</p>
						<div className="mt-4 max-h-[50vh] space-y-2 overflow-auto pr-1">
							{detailTask.events.length === 0 ? (
								<p className="text-muted-foreground text-sm">暂无事件日志。</p>
							) : (
								detailTask.events.map((event) => (
									<div key={event.id} className="bg-card/70 rounded border p-2">
										<p className="font-mono text-[11px]">
											{event.event_type} · {formatLocalHm(event.created_at)}
										</p>
										<pre className="text-muted-foreground mt-1 overflow-x-auto whitespace-pre-wrap break-all text-[11px]">
											{event.payload_json}
										</pre>
									</div>
								))
							)}
						</div>
					</div>
				</div>
			) : null}
		</div>
	);
}
