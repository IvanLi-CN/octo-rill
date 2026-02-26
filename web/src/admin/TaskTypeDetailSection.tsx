import type { AdminRealtimeTaskDetailResponse } from "@/api";

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

function readNumberArray(payload: JsonRecord | null, key: string): number[] {
	if (!payload) return [];
	const value = payload[key];
	if (!Array.isArray(value)) return [];
	return value.filter((item): item is number => typeof item === "number");
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
	for (const event of detail.events) {
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
			const contentLength = readNumber(result, "content_length");
			return {
				pageTitle: "日报生成详情页",
				pageSummary: "展示单用户日报生成任务的输入用户与输出长度。",
				fields: buildFields(
					field("目标用户", userId ? `#${userId}` : null),
					field(
						"生成字符数",
						contentLength !== null ? `${contentLength} chars` : null,
					),
				),
			};
		}
		case "brief.daily_slot": {
			const hourUtc = readNumber(payload, "hour_utc");
			const total = readNumber(result, "total");
			const succeeded = readNumber(result, "succeeded");
			const failed = readNumber(result, "failed");
			const canceled = readString(result, "canceled");
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
						summary.collectedUsers !== null
							? `${summary.collectedUsers}`
							: total !== null
								? `${total}`
								: null,
					),
					field("串行已推进", `${summary.progressedUsers}`),
					field("成功", succeeded !== null ? `${succeeded}` : null),
					field(
						"失败",
						failed !== null ? `${failed}` : `${summary.failedUsers}`,
					),
					field(
						"是否取消",
						canceled === "true" ? "是" : canceled ? "否" : null,
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
		case "translate.release.batch": {
			const releaseIds = readNumberArray(payload, "release_ids");
			const summary = summarizeTranslateBatchResult(result);
			const progress = summarizeTranslateBatchProgress(detail);
			return {
				pageTitle: "批量翻译 Release 详情页",
				pageSummary:
					"展示批量翻译任务的目标 Release 数量、进度与 ready/missing/error 结果分布。",
				fields: buildFields(
					field("目标用户", userId ? `#${userId}` : null),
					field("目标 Release 数", `${releaseIds.length}`),
					field("事件已处理条数", `${progress.processed}`),
					field("最后阶段", progress.lastStage),
					field("总数", summary.total !== null ? `${summary.total}` : null),
					field("ready", summary.ready !== null ? `${summary.ready}` : null),
					field(
						"missing",
						summary.missing !== null ? `${summary.missing}` : null,
					),
					field(
						"disabled",
						summary.disabled !== null ? `${summary.disabled}` : null,
					),
					field("error", summary.error !== null ? `${summary.error}` : null),
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

export function TaskTypeDetailSection(props: {
	detail: AdminRealtimeTaskDetailResponse;
}) {
	const model = buildTaskDetailPageModel(props.detail);
	const payload = parseJsonRecord(props.detail.task.payload_json);
	const result = parseJsonRecord(props.detail.task.result_json);
	const detailCardClass = "rounded-lg border p-3";

	return (
		<section className="space-y-3">
			<div className="grid gap-2 md:grid-cols-2">
				{model.fields.map((item) => (
					<div key={`${item.label}:${item.value}`} className={detailCardClass}>
						<p className="text-muted-foreground text-[11px]">{item.label}</p>
						<p className="mt-1 text-sm font-medium">{item.value}</p>
					</div>
				))}
			</div>
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
