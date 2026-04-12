import type { AdminLlmCallItem, AdminRealtimeTaskDetailResponse } from "@/api";
import { Button } from "@/components/ui/button";
import { formatIsoShortLocal } from "@/lib/datetime";

type JsonRecord = Record<string, unknown>;

type TaskField = {
	label: string;
	value: string;
};

type TaskDetailPageModel = {
	pageTitle: string;
	pageSummary: string;
	fields: TaskField[];
};

function parseJsonRecord(raw: string | null | undefined): JsonRecord | null {
	if (!raw) return null;
	try {
		const parsed = JSON.parse(raw) as unknown;
		if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
			return null;
		}
		return parsed as JsonRecord;
	} catch {
		return null;
	}
}

function readString(payload: JsonRecord | null, key: string): string | null {
	if (!payload) return null;
	const value = payload[key];
	if (typeof value === "string") {
		const trimmed = value.trim();
		return trimmed.length > 0 ? trimmed : null;
	}
	if (typeof value === "number" || typeof value === "boolean") {
		return String(value);
	}
	return null;
}

function readNumber(payload: JsonRecord | null, key: string): number | null {
	if (!payload) return null;
	const value = payload[key];
	return typeof value === "number" ? value : null;
}

function readBoolean(payload: JsonRecord | null, key: string): boolean | null {
	if (!payload) return null;
	const value = payload[key];
	if (typeof value === "boolean") return value;
	if (typeof value === "string") {
		if (value === "true" || value === "1") return true;
		if (value === "false" || value === "0") return false;
	}
	if (typeof value === "number") return value !== 0;
	return null;
}

function readIdArray(payload: JsonRecord | null, key: string): string[] {
	if (!payload) return [];
	const value = payload[key];
	if (!Array.isArray(value)) return [];
	return value.flatMap((item) => {
		if (typeof item === "string") {
			const trimmed = item.trim();
			return trimmed ? [trimmed] : [];
		}
		if (typeof item === "number" || typeof item === "boolean") {
			return [String(item)];
		}
		return [];
	});
}

function readObject(
	payload: JsonRecord | null,
	key: string,
): JsonRecord | null {
	if (!payload) return null;
	const value = payload[key];
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		return null;
	}
	return value as JsonRecord;
}

function buildFields(...items: Array<TaskField | null>): TaskField[] {
	return items.filter((item): item is TaskField => Boolean(item));
}

function field(
	label: string,
	value: string | null | undefined,
): TaskField | null {
	if (!value) return null;
	return { label, value };
}

function summarizeDailySlotEvents(detail: AdminRealtimeTaskDetailResponse) {
	let collectedUsers: number | null = null;
	let progressedUsers = 0;
	let failedUsers = 0;

	for (const event of detail.events) {
		if (event.event_type !== "task.progress") continue;
		const payload = parseJsonRecord(event.payload_json);
		const stage = readString(payload, "stage");
		if (stage === "collect") {
			const totalUsers = readNumber(payload, "total_users");
			if (totalUsers !== null) {
				collectedUsers = totalUsers;
			}
		}
		if (stage === "generate") {
			const index = readNumber(payload, "index");
			if (index !== null) {
				progressedUsers = Math.max(progressedUsers, index);
			}
		}
		if (stage === "user_failed") {
			failedUsers += 1;
		}
	}

	return { collectedUsers, progressedUsers, failedUsers };
}

function summarizeTranslateBatchResult(result: JsonRecord | null) {
	if (!result) {
		return {
			total: null as number | null,
			ready: null as number | null,
			missing: null as number | null,
			disabled: null as number | null,
			error: null as number | null,
		};
	}

	const total = readNumber(result, "total");
	const ready = readNumber(result, "ready");
	const missing = readNumber(result, "missing");
	const disabled = readNumber(result, "disabled");
	const error = readNumber(result, "error");

	if (
		total !== null ||
		ready !== null ||
		missing !== null ||
		disabled !== null ||
		error !== null
	) {
		return { total, ready, missing, disabled, error };
	}

	const items = result.items;
	if (!Array.isArray(items)) {
		return {
			total: null as number | null,
			ready: null as number | null,
			missing: null as number | null,
			disabled: null as number | null,
			error: null as number | null,
		};
	}

	let readyCount = 0;
	let missingCount = 0;
	let disabledCount = 0;
	let errorCount = 0;
	for (const item of items) {
		if (!item || typeof item !== "object" || Array.isArray(item)) continue;
		const status = readString(item as JsonRecord, "status");
		if (status === "ready") readyCount += 1;
		if (status === "missing") missingCount += 1;
		if (status === "disabled") disabledCount += 1;
		if (status === "error") errorCount += 1;
	}

	return {
		total: items.length,
		ready: readyCount,
		missing: missingCount,
		disabled: disabledCount,
		error: errorCount,
	};
}

function summarizeTranslateBatchProgress(
	detail: AdminRealtimeTaskDetailResponse,
) {
	let processed = 0;
	let lastStage: string | null = null;
	const orderedEvents = [...detail.events].sort(
		(a, b) =>
			a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id),
	);
	for (const event of orderedEvents) {
		if (event.event_type !== "task.progress") continue;
		const payload = parseJsonRecord(event.payload_json);
		const stage = readString(payload, "stage");
		if (!stage) continue;
		lastStage = stage;
		if (stage === "release") {
			processed += 1;
		}
	}
	return { processed, lastStage };
}

function buildTaskDetailPageModel(
	detail: AdminRealtimeTaskDetailResponse,
): TaskDetailPageModel {
	const task = detail.task;
	const payload = parseJsonRecord(task.payload_json);
	const result = parseJsonRecord(task.result_json);
	const userId = readString(payload, "user_id");

	switch (task.task_type) {
		case "sync.starred": {
			const syncedRepos = readNumber(result, "repos");
			return {
				pageTitle: "同步 Star 详情页",
				pageSummary: "展示单次 Star 同步任务的目标用户与仓库写入结果。",
				fields: buildFields(
					field("目标用户", userId ? `#${userId}` : null),
					field("同步仓库数", syncedRepos !== null ? `${syncedRepos}` : null),
				),
			};
		}
		case "sync.releases": {
			const syncedRepos = readNumber(result, "repos");
			const syncedReleases = readNumber(result, "releases");
			return {
				pageTitle: "同步 Release 详情页",
				pageSummary: "展示 Release 同步任务覆盖仓库与写入 Release 数量。",
				fields: buildFields(
					field("目标用户", userId ? `#${userId}` : null),
					field("覆盖仓库数", syncedRepos !== null ? `${syncedRepos}` : null),
					field(
						"写入 Release 数",
						syncedReleases !== null ? `${syncedReleases}` : null,
					),
				),
			};
		}
		case "sync.access_refresh": {
			const starred = readObject(result, "starred");
			const release = readObject(result, "release");
			return {
				pageTitle: "访问增量同步详情页",
				pageSummary:
					"展示用户访问触发的 staged refresh，包括 Star 快照刷新与共享 Release 队列结果。",
				fields: buildFields(
					field("目标用户", userId ? `#${userId}` : null),
					field(
						"Star 仓库数",
						readNumber(starred, "repos") !== null
							? `${readNumber(starred, "repos")}`
							: null,
					),
					field(
						"Release 仓库数",
						readNumber(release, "repos") !== null
							? `${readNumber(release, "repos")}`
							: null,
					),
					field(
						"共享 Release 数",
						readNumber(release, "releases") !== null
							? `${readNumber(release, "releases")}`
							: null,
					),
				),
			};
		}
		case "sync.notifications": {
			const notifications = readNumber(result, "notifications");
			const since = readString(result, "since");
			return {
				pageTitle: "同步通知详情页",
				pageSummary: "展示通知同步任务拉取条数与增量窗口。",
				fields: buildFields(
					field("目标用户", userId ? `#${userId}` : null),
					field(
						"同步通知数",
						notifications !== null ? `${notifications}` : null,
					),
					field("增量起点", since),
				),
			};
		}
		case "sync.all": {
			const starred = readObject(result, "starred");
			const releases = readObject(result, "releases");
			const notifications = readObject(result, "notifications");
			return {
				pageTitle: "全量同步详情页",
				pageSummary: "展示全量同步任务中 Star/Release/通知三段执行结果。",
				fields: buildFields(
					field("目标用户", userId ? `#${userId}` : null),
					field(
						"Star 同步仓库",
						readNumber(starred, "repos") !== null
							? `${readNumber(starred, "repos")}`
							: null,
					),
					field(
						"Release 同步数",
						readNumber(releases, "releases") !== null
							? `${readNumber(releases, "releases")}`
							: null,
					),
					field(
						"通知同步数",
						readNumber(notifications, "notifications") !== null
							? `${readNumber(notifications, "notifications")}`
							: null,
					),
				),
			};
		}
		case "brief.generate": {
			const diagnostics = detail.diagnostics?.brief_generate ?? null;
			const contentLength =
				diagnostics?.content_length ?? readNumber(result, "content_length");
			const keyDate = diagnostics?.key_date ?? readString(payload, "key_date");
			const targetUser =
				diagnostics?.target_user_id !== null &&
				diagnostics?.target_user_id !== undefined
					? `#${diagnostics.target_user_id}`
					: userId
						? `#${userId}`
						: null;
			return {
				pageTitle: "日报生成详情页",
				pageSummary: "展示单用户日报生成任务的输入用户与输出长度。",
				fields: buildFields(
					field("目标用户", targetUser),
					field(
						"生成字符数",
						contentLength !== null ? `${contentLength} chars` : null,
					),
					field("key_date", keyDate),
				),
			};
		}
		case "brief.daily_slot": {
			const diagnostics = detail.diagnostics?.brief_daily_slot ?? null;
			const hourUtc = diagnostics?.hour_utc ?? readNumber(payload, "hour_utc");
			const total =
				diagnostics?.summary.total_users ?? readNumber(result, "total");
			const succeeded =
				diagnostics?.summary.succeeded_users ?? readNumber(result, "succeeded");
			const failed =
				diagnostics?.summary.failed_users ?? readNumber(result, "failed");
			const canceled =
				diagnostics?.summary.canceled ?? readBoolean(result, "canceled");
			const summary = summarizeDailySlotEvents(detail);
			return {
				pageTitle: "日报定时槽详情页",
				pageSummary:
					"展示日报定时任务在指定 UTC 小时槽的串行执行进度与成功/失败统计。",
				fields: buildFields(
					field(
						"UTC 小时槽",
						hourUtc !== null
							? `${hourUtc.toString().padStart(2, "0")}:00`
							: null,
					),
					field(
						"收集用户数",
						total !== null
							? `${total}`
							: summary.collectedUsers !== null
								? `${summary.collectedUsers}`
								: null,
					),
					field(
						"串行已推进",
						`${
							diagnostics?.summary.progressed_users !== undefined
								? diagnostics.summary.progressed_users
								: summary.progressedUsers
						}`,
					),
					field("成功", succeeded !== null ? `${succeeded}` : null),
					field(
						"失败",
						failed !== null ? `${failed}` : `${summary.failedUsers}`,
					),
					field(
						"是否取消",
						canceled === true ? "是" : canceled === false ? "否" : null,
					),
				),
			};
		}
		case "sync.subscriptions": {
			const diagnostics = detail.diagnostics?.sync_subscriptions ?? null;
			const skipped = diagnostics?.skipped ?? readBoolean(result, "skipped");
			const skipReason =
				diagnostics?.skip_reason ?? readString(result, "skip_reason");
			const social = readObject(result, "social");
			const notifications = readObject(result, "notifications");
			return {
				pageTitle: "订阅同步详情页",
				pageSummary:
					"展示半小时全用户订阅同步的 Star、Release、social 与 Inbox 四阶段结果、关键事件与日志入口。",
				fields: buildFields(
					field(
						"触发方式",
						diagnostics?.trigger ?? readString(payload, "trigger"),
					),
					field(
						"调度键",
						diagnostics?.schedule_key ?? readString(payload, "schedule_key"),
					),
					field("本轮状态", skipped === true ? "已跳过" : "已执行"),
					field("跳过原因", skipReason),
					field(
						"Star 成功/总计",
						diagnostics
							? `${diagnostics.star.succeeded_users}/${diagnostics.star.total_users}`
							: null,
					),
					field(
						"Release 成功/总计",
						diagnostics
							? `${diagnostics.release.succeeded_repos}/${diagnostics.release.total_repos}`
							: null,
					),
					field(
						"Social 成功/总计",
						diagnostics
							? `${diagnostics.social.succeeded_users}/${diagnostics.social.total_users}`
							: readNumber(social, "succeeded_users") !== null &&
									readNumber(social, "total_users") !== null
								? `${readNumber(social, "succeeded_users")}/${readNumber(
										social,
										"total_users",
									)}`
								: null,
					),
					field(
						"Social 事件写入",
						diagnostics
							? `${diagnostics.social.events}`
							: readNumber(social, "events") !== null
								? `${readNumber(social, "events")}`
								: null,
					),
					field(
						"Inbox 成功/总计",
						diagnostics
							? `${diagnostics.notifications.succeeded_users}/${diagnostics.notifications.total_users}`
							: readNumber(notifications, "succeeded_users") !== null &&
									readNumber(notifications, "total_users") !== null
								? `${readNumber(notifications, "succeeded_users")}/${readNumber(
										notifications,
										"total_users",
									)}`
								: null,
					),
					field(
						"同步通知数",
						diagnostics
							? `${diagnostics.notifications.notifications}`
							: readNumber(notifications, "notifications") !== null
								? `${readNumber(notifications, "notifications")}`
								: null,
					),
					field(
						"写入 Release 数",
						diagnostics ? `${diagnostics.releases_written}` : null,
					),
					field(
						"关键事件数",
						diagnostics ? `${diagnostics.critical_events}` : null,
					),
				),
			};
		}
		case "translate.release": {
			const releaseId = readString(payload, "release_id");
			const status = readString(result, "status");
			return {
				pageTitle: "Release 翻译详情页",
				pageSummary: "展示单条 Release 翻译任务的目标对象与翻译状态。",
				fields: buildFields(
					field("目标用户", userId ? `#${userId}` : null),
					field("Release ID", releaseId),
					field("翻译结果", status),
				),
			};
		}
		case "translate.release.batch":
		case "summarize.release.smart.batch": {
			const diagnostics = detail.diagnostics?.translate_release_batch ?? null;
			const releaseIds = readIdArray(payload, "release_ids");
			const summary = summarizeTranslateBatchResult(result);
			const progress = summarizeTranslateBatchProgress(detail);
			const totalCount = diagnostics?.summary.total ?? summary.total;
			const readyCount = diagnostics?.summary.ready ?? summary.ready;
			const missingCount = diagnostics?.summary.missing ?? summary.missing;
			const disabledCount = diagnostics?.summary.disabled ?? summary.disabled;
			const errorCount = diagnostics?.summary.error ?? summary.error;
			const targetUser =
				diagnostics?.target_user_id !== null &&
				diagnostics?.target_user_id !== undefined
					? `#${diagnostics.target_user_id}`
					: userId
						? `#${userId}`
						: null;
			return {
				pageTitle:
					task.task_type === "summarize.release.smart.batch"
						? "批量智能整理 Release 详情页"
						: "批量翻译 Release 详情页",
				pageSummary:
					task.task_type === "summarize.release.smart.batch"
						? "展示批量智能整理任务的目标 Release 数量、进度与 ready/missing/error 结果分布。"
						: "展示批量翻译任务的目标 Release 数量、进度与 ready/missing/error 结果分布。",
				fields: buildFields(
					field("目标用户", targetUser),
					field(
						"目标 Release 数",
						`${
							diagnostics?.release_total !== undefined
								? diagnostics.release_total
								: releaseIds.length
						}`,
					),
					field(
						"事件已处理条数",
						`${
							diagnostics?.progress.processed !== undefined
								? diagnostics.progress.processed
								: progress.processed
						}`,
					),
					field(
						"最后阶段",
						diagnostics?.progress.last_stage ?? progress.lastStage,
					),
					field("总数", totalCount !== null ? `${totalCount}` : null),
					field("ready", readyCount !== null ? `${readyCount}` : null),
					field("missing", missingCount !== null ? `${missingCount}` : null),
					field("disabled", disabledCount !== null ? `${disabledCount}` : null),
					field("error", errorCount !== null ? `${errorCount}` : null),
				),
			};
		}
		case "translate.release_detail": {
			const releaseId = readString(payload, "release_id");
			const status = readString(result, "status");
			return {
				pageTitle: "Release 详情翻译页",
				pageSummary: "展示 Release 详情正文翻译任务的执行对象与状态。",
				fields: buildFields(
					field("目标用户", userId ? `#${userId}` : null),
					field("Release ID", releaseId),
					field("翻译结果", status),
				),
			};
		}
		case "translate.notification": {
			const threadId = readString(payload, "thread_id");
			const status = readString(result, "status");
			return {
				pageTitle: "通知翻译详情页",
				pageSummary: "展示单条通知翻译任务的线程目标与翻译状态。",
				fields: buildFields(
					field("目标用户", userId ? `#${userId}` : null),
					field("Thread ID", threadId),
					field("翻译结果", status),
				),
			};
		}
		default:
			return {
				pageTitle: `通用任务详情页（${task.task_type}）`,
				pageSummary:
					"该任务类型暂未定义专属布局，展示原始 payload/result 以便排查。",
				fields: buildFields(
					field("目标用户", userId ? `#${userId}` : null),
					field("任务类型", task.task_type),
				),
			};
	}
}

function businessOutcomeClass(code: string | undefined) {
	switch (code) {
		case "ok":
			return "border-emerald-500/40 bg-emerald-500/5";
		case "partial":
			return "border-amber-500/40 bg-amber-500/5";
		case "failed":
			return "border-red-500/40 bg-red-500/5";
		case "disabled":
			return "border-slate-500/40 bg-slate-500/5";
		default:
			return "border-border bg-muted/30";
	}
}

function translateItemStatusLabel(status: string) {
	switch (status) {
		case "ready":
			return "已就绪";
		case "missing":
			return "缺失";
		case "disabled":
			return "已禁用";
		case "error":
			return "失败";
		case "processing":
			return "处理中";
		default:
			return status;
	}
}

function dailyUserStateLabel(state: string) {
	switch (state) {
		case "succeeded":
			return "成功";
		case "failed":
			return "失败";
		case "running":
			return "执行中";
		default:
			return state;
	}
}

function syncSubscriptionStageLabel(stage: string) {
	switch (stage) {
		case "scheduler":
			return "调度";
		case "star":
			return "Star";
		case "release":
			return "Release";
		default:
			return stage;
	}
}

function syncSubscriptionSeverityLabel(severity: string) {
	switch (severity) {
		case "error":
			return "错误";
		case "warning":
			return "告警";
		case "info":
			return "信息";
		default:
			return severity;
	}
}

function syncSubscriptionSeverityClass(severity: string) {
	switch (severity) {
		case "error":
			return "border-red-500/35 bg-red-500/5 text-red-700 dark:text-red-200";
		case "warning":
			return "border-amber-500/35 bg-amber-500/5 text-amber-700 dark:text-amber-200";
		default:
			return "border-border bg-muted/30 text-foreground";
	}
}

function llmCallStatusLabel(status: string) {
	switch (status) {
		case "queued":
			return "排队中";
		case "running":
			return "运行中";
		case "succeeded":
			return "成功";
		case "failed":
			return "失败";
		default:
			return status;
	}
}

function shouldShowRelatedLlmCalls(taskType: string) {
	return taskType.startsWith("translate.") || taskType.startsWith("brief.");
}

type TaskTypeDetailSectionProps = {
	detail: AdminRealtimeTaskDetailResponse;
	relatedLlmCalls?: AdminLlmCallItem[];
	relatedLlmCallsLoading?: boolean;
	onOpenLlmCallDetail?: (callId: string) => void;
};

export function TaskTypeDetailSection(props: TaskTypeDetailSectionProps) {
	const relatedLlmCalls = props.relatedLlmCalls ?? [];
	const relatedLlmCallsLoading = props.relatedLlmCallsLoading ?? false;
	const model = buildTaskDetailPageModel(props.detail);
	const payload = parseJsonRecord(props.detail.task.payload_json);
	const result = parseJsonRecord(props.detail.task.result_json);
	const diagnostics = props.detail.diagnostics ?? null;
	const syncDiagnostics = diagnostics?.sync_subscriptions ?? null;
	const eventMeta = props.detail.event_meta ?? null;
	const isEventsTruncated = eventMeta?.truncated === true;
	const detailCardClass = "rounded-lg border p-3";
	const diagnosticsPrimaryError =
		props.detail.task.error_message ??
		diagnostics?.translate_release_batch?.items?.find((item) => item.item_error)
			?.item_error ??
		diagnostics?.brief_daily_slot?.users?.find((item) => item.error)?.error ??
		syncDiagnostics?.recent_events?.find((item) => item.severity === "error")
			?.message ??
		null;
	const showRelatedLlmCalls = shouldShowRelatedLlmCalls(
		props.detail.task.task_type,
	);

	return (
		<section className="space-y-3">
			{diagnostics ? (
				<div
					className={`rounded-lg border p-3 ${businessOutcomeClass(diagnostics.business_outcome.code)}`}
				>
					<p className="text-muted-foreground text-[11px]">业务结果诊断</p>
					<p className="mt-1 text-sm font-semibold">
						{diagnostics.business_outcome.label}
					</p>
					<p className="text-muted-foreground mt-1 text-xs">
						{diagnostics.business_outcome.message}
					</p>
					{diagnosticsPrimaryError ? (
						<div className="mt-2 rounded-md border border-red-500/35 bg-red-500/5 p-2">
							<p className="text-[11px] font-medium text-red-700 dark:text-red-300">
								失败主因
							</p>
							<p className="mt-1 text-sm font-medium text-red-700 dark:text-red-200">
								{diagnosticsPrimaryError}
							</p>
						</div>
					) : null}
				</div>
			) : null}
			{isEventsTruncated ? (
				<div className="rounded-lg border border-amber-500/40 bg-amber-500/5 p-3">
					<p className="text-[11px] font-medium text-amber-800 dark:text-amber-200">
						明细可能非全量
					</p>
					<p className="mt-1 text-xs text-amber-700 dark:text-amber-300">
						当前仅基于最近 {eventMeta?.limit ?? 200} 条事件生成明细诊断。
					</p>
				</div>
			) : null}
			<div className="grid gap-2 md:grid-cols-2">
				{model.fields.map((item) => (
					<div key={`${item.label}:${item.value}`} className={detailCardClass}>
						<p className="text-muted-foreground text-[11px]">{item.label}</p>
						<p className="mt-1 text-sm font-medium">{item.value}</p>
					</div>
				))}
			</div>
			{diagnostics?.translate_release_batch?.items?.length ? (
				<div className={detailCardClass}>
					<p className="text-muted-foreground text-[11px]">Release 翻译明细</p>
					<div className="mt-2 space-y-2">
						{diagnostics.translate_release_batch.items.map((item) => (
							<div key={item.release_id} className="rounded-md border p-2">
								<div className="flex flex-wrap items-center justify-between gap-2">
									<p className="font-mono text-xs">#{item.release_id}</p>
									<span className="text-xs font-medium">
										{translateItemStatusLabel(item.item_status)}
									</span>
								</div>
								{item.item_error ? (
									<p className="mt-1 text-sm font-medium text-red-700 dark:text-red-300">
										错误原因：{item.item_error}
									</p>
								) : null}
								<p className="text-muted-foreground mt-1 text-[11px]">
									最后事件：{item.last_event_at}
								</p>
							</div>
						))}
					</div>
				</div>
			) : null}
			{diagnostics?.brief_daily_slot?.users?.length ? (
				<div className={detailCardClass}>
					<p className="text-muted-foreground text-[11px]">用户执行明细</p>
					<div className="mt-2 space-y-2">
						{diagnostics.brief_daily_slot.users.map((item) => (
							<div key={item.user_id} className="rounded-md border p-2">
								<div className="flex flex-wrap items-center justify-between gap-2">
									<p className="text-xs font-medium">用户 #{item.user_id}</p>
									<span className="text-xs font-medium">
										{dailyUserStateLabel(item.state)}
									</span>
								</div>
								{item.key_date ? (
									<p className="text-muted-foreground mt-1 text-xs">
										key_date: {item.key_date}
									</p>
								) : null}
								{item.error ? (
									<p className="mt-1 text-sm font-medium text-red-700 dark:text-red-300">
										错误原因：{item.error}
									</p>
								) : null}
								<p className="text-muted-foreground mt-1 text-[11px]">
									最后事件：{item.last_event_at}
								</p>
							</div>
						))}
					</div>
				</div>
			) : null}
			{syncDiagnostics ? (
				<div className="grid gap-2 md:grid-cols-3">
					<div className={detailCardClass}>
						<p className="text-muted-foreground text-[11px]">Star 阶段摘要</p>
						<p className="mt-1 text-sm font-semibold">
							{syncDiagnostics.star.succeeded_users}/
							{syncDiagnostics.star.total_users}
							位用户成功
						</p>
						<p className="text-muted-foreground mt-1 text-xs">
							失败 {syncDiagnostics.star.failed_users} · 仓库快照{" "}
							{syncDiagnostics.star.total_repos}
						</p>
					</div>
					<div className={detailCardClass}>
						<p className="text-muted-foreground text-[11px]">
							Release 阶段摘要
						</p>
						<p className="mt-1 text-sm font-semibold">
							{syncDiagnostics.release.succeeded_repos}/
							{syncDiagnostics.release.total_repos}
							个仓库成功
						</p>
						<p className="text-muted-foreground mt-1 text-xs">
							失败 {syncDiagnostics.release.failed_repos} · 候选失败{" "}
							{syncDiagnostics.release.candidate_failures}
						</p>
					</div>
					<div className={detailCardClass}>
						<p className="text-muted-foreground text-[11px]">附加信息</p>
						<p className="mt-1 text-sm font-semibold">
							写入 {syncDiagnostics.releases_written} 条 Release
						</p>
						<p className="text-muted-foreground mt-1 text-xs">
							关键事件 {syncDiagnostics.critical_events} · 本轮
							{syncDiagnostics.skipped ? "已跳过" : "已执行"}
						</p>
						{syncDiagnostics.log_available &&
						syncDiagnostics.log_download_path ? (
							<Button
								asChild
								className="mt-3 w-full"
								size="sm"
								variant="outline"
							>
								<a download href={syncDiagnostics.log_download_path}>
									下载日志
								</a>
							</Button>
						) : (
							<p className="text-muted-foreground mt-3 text-xs">
								暂无可下载日志。
							</p>
						)}
					</div>
				</div>
			) : null}
			{syncDiagnostics?.recent_events?.length ? (
				<div className={detailCardClass}>
					<div className="flex flex-wrap items-center justify-between gap-2">
						<p className="text-muted-foreground text-[11px]">最近关键事件</p>
						<p className="text-muted-foreground text-[11px]">
							仅展示最近 {syncDiagnostics.recent_events.length} 条
						</p>
					</div>
					<div className="mt-2 space-y-2">
						{syncDiagnostics.recent_events.map((item) => (
							<div
								key={item.id}
								className={`rounded-md border p-2 ${syncSubscriptionSeverityClass(item.severity)}`}
							>
								<div className="flex flex-wrap items-center justify-between gap-2">
									<p className="text-xs font-medium">
										{syncSubscriptionStageLabel(item.stage)} · {item.event_type}
									</p>
									<span className="text-[11px] font-medium">
										{syncSubscriptionSeverityLabel(item.severity)}
									</span>
								</div>
								{item.message ? (
									<p className="mt-1 text-sm font-medium">{item.message}</p>
								) : null}
								<p className="text-muted-foreground mt-1 text-[11px]">
									{formatIsoShortLocal(item.created_at)}
									{item.user_id !== null ? ` · 用户 #${item.user_id}` : ""}
									{item.repo_full_name ? ` · ${item.repo_full_name}` : ""}
									{item.attempt > 0 ? ` · attempt ${item.attempt}` : ""}
									{item.recoverable ? " · 可恢复" : ""}
								</p>
							</div>
						))}
					</div>
				</div>
			) : null}
			{showRelatedLlmCalls ? (
				<div className={detailCardClass}>
					<p className="text-muted-foreground text-[11px]">关联 LLM 调用</p>
					{relatedLlmCallsLoading ? (
						<p className="text-muted-foreground mt-2 text-xs">
							正在加载关联调用...
						</p>
					) : relatedLlmCalls.length === 0 ? (
						<p className="text-muted-foreground mt-2 text-xs">暂无关联调用。</p>
					) : (
						<div className="mt-2 space-y-2">
							{relatedLlmCalls.map((call) => (
								<div key={call.id} className="rounded-md border p-2">
									<div className="flex flex-wrap items-center justify-between gap-2">
										<div className="min-w-0">
											<p className="truncate font-mono text-xs">{call.id}</p>
											<p className="text-muted-foreground mt-1 text-[11px]">
												{call.source} · {llmCallStatusLabel(call.status)}
											</p>
										</div>
										<Button
											size="sm"
											variant="outline"
											disabled={!props.onOpenLlmCallDetail}
											onClick={() => props.onOpenLlmCallDetail?.(call.id)}
										>
											查看 LLM 详情
										</Button>
									</div>
								</div>
							))}
						</div>
					)}
				</div>
			) : null}
			<details className={detailCardClass}>
				<summary className="text-muted-foreground cursor-pointer text-xs font-medium">
					查看任务输入/输出原始 JSON
				</summary>
				<div className="mt-2 grid gap-2 md:grid-cols-2">
					<div className={detailCardClass}>
						<p className="text-muted-foreground text-[11px]">payload</p>
						<pre className="mt-1 overflow-x-auto whitespace-pre-wrap break-all font-mono text-[11px]">
							{payload ? JSON.stringify(payload, null, 2) : "(empty)"}
						</pre>
					</div>
					<div className={detailCardClass}>
						<p className="text-muted-foreground text-[11px]">result</p>
						<pre className="mt-1 overflow-x-auto whitespace-pre-wrap break-all font-mono text-[11px]">
							{result ? JSON.stringify(result, null, 2) : "(empty)"}
						</pre>
					</div>
				</div>
			</details>
		</section>
	);
}
