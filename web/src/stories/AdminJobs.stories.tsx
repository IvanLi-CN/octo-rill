import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useRef, useState } from "react";

import type {
	AdminLlmCallDetailResponse,
	AdminRealtimeTaskDetailResponse,
	AdminRealtimeTaskItem,
} from "@/api";
import { AdminJobs } from "@/pages/AdminJobs";

const CURRENT_USER_ID = "2f4k7m9p3x6c8v2a";
const RECENT_EVENT_USER_ID = "4h6p9s3t5z8e2x4c";
const STORYBOOK_NOW = Date.now();
const RUNNING_WORKER_UPDATED_AT = new Date(
	STORYBOOK_NOW - 75_000,
).toISOString();
const ERROR_WORKER_UPDATED_AT = new Date(STORYBOOK_NOW - 30_000).toISOString();

const realtimeTasksSeed: AdminRealtimeTaskItem[] = [
	{
		id: "task-sync-releases",
		task_type: "sync.releases",
		status: "running",
		source: "manual",
		requested_by: CURRENT_USER_ID,
		parent_task_id: null,
		cancel_requested: false,
		error_message: null,
		created_at: "2026-02-27T05:58:00Z",
		started_at: "2026-02-27T05:58:05Z",
		finished_at: null,
		updated_at: "2026-02-27T05:58:20Z",
	},
	{
		id: "task-sync-starred",
		task_type: "sync.starred",
		status: "succeeded",
		source: "manual",
		requested_by: CURRENT_USER_ID,
		parent_task_id: null,
		cancel_requested: false,
		error_message: null,
		created_at: "2026-02-27T05:54:00Z",
		started_at: "2026-02-27T05:54:02Z",
		finished_at: "2026-02-27T05:54:13Z",
		updated_at: "2026-02-27T05:54:13Z",
	},
	{
		id: "task-translate-batch-story",
		task_type: "translate.release.batch",
		status: "succeeded",
		source: "api.translate_releases_batch_stream",
		requested_by: CURRENT_USER_ID,
		parent_task_id: null,
		cancel_requested: false,
		error_message: null,
		created_at: "2026-02-27T05:50:00Z",
		started_at: "2026-02-27T05:50:01Z",
		finished_at: "2026-02-27T05:50:30Z",
		updated_at: "2026-02-27T05:50:30Z",
	},
];

const scheduledRunsSeed: AdminRealtimeTaskItem[] = [
	{
		id: "task-brief-slot-07",
		task_type: "brief.daily_slot",
		status: "running",
		source: "scheduler",
		requested_by: null,
		parent_task_id: null,
		cancel_requested: false,
		error_message: null,
		created_at: "2026-02-27T07:00:00Z",
		started_at: "2026-02-27T07:00:02Z",
		finished_at: null,
		updated_at: "2026-02-27T07:00:10Z",
	},
	{
		id: "task-brief-slot-06",
		task_type: "brief.daily_slot",
		status: "succeeded",
		source: "scheduler",
		requested_by: null,
		parent_task_id: null,
		cancel_requested: false,
		error_message: null,
		created_at: "2026-02-27T06:00:00Z",
		started_at: "2026-02-27T06:00:01Z",
		finished_at: "2026-02-27T06:03:24Z",
		updated_at: "2026-02-27T06:03:24Z",
	},
	{
		id: "task-subscription-1430",
		task_type: "sync.subscriptions",
		status: "succeeded",
		source: "scheduler",
		requested_by: null,
		parent_task_id: null,
		cancel_requested: false,
		error_message: null,
		created_at: "2026-02-27T14:30:00Z",
		started_at: "2026-02-27T14:30:02Z",
		finished_at: "2026-02-27T14:37:18Z",
		updated_at: "2026-02-27T14:37:18Z",
	},
];

const llmCallsSeed: AdminLlmCallDetailResponse[] = [
	{
		id: "llm-call-1",
		status: "failed",
		source: "job.api.translate_release",
		model: "gpt-4o-mini",
		requested_by: CURRENT_USER_ID,
		parent_task_id: "task-sync-releases",
		parent_task_type: "sync.releases",
		max_tokens: 900,
		attempt_count: 3,
		scheduler_wait_ms: 1500,
		first_token_wait_ms: 980,
		duration_ms: 2600,
		input_tokens: 1320,
		output_tokens: 0,
		cached_input_tokens: 640,
		total_tokens: 1320,
		input_messages_json: JSON.stringify([
			{
				role: "system",
				content:
					"You are an assistant that translates GitHub release notes into concise Chinese.",
			},
			{
				role: "user",
				content: [
					"Repo: octo-rill",
					"Release: v0.1.0",
					"Notes:",
					"- feat(admin): add LLM scheduler observability",
					"- fix(api): normalize RFC3339 timestamps",
					"- docs: update admin guide",
				].join("\n"),
			},
			{
				role: "assistant",
				content: "收到。我会先给出 4 条摘要，再附 1 条排障提示。",
			},
			{
				role: "user",
				content: "请保留术语 scheduler / retry，并用中文输出。",
			},
		]),
		output_messages_json: null,
		prompt_text: `system:
You are an assistant that translates GitHub release notes into concise Chinese.
Preserve markdown structure and links.

user:
Repo: octo-rill
Release: v0.1.0
Notes:
- feat(admin): add LLM scheduler observability
- fix(api): normalize RFC3339 timestamps
- docs: update admin guide`,
		response_text: null,
		error_text:
			"OpenAI API timeout after 30000ms; retry budget exhausted (attempt=3).",
		created_at: "2026-02-27T06:10:00Z",
		started_at: "2026-02-27T06:10:01Z",
		finished_at: "2026-02-27T06:10:04Z",
		updated_at: "2026-02-27T06:10:04Z",
	},
	{
		id: "llm-call-2",
		status: "running",
		source: "api.translate_releases_batch",
		model: "gpt-4o-mini",
		requested_by: CURRENT_USER_ID,
		parent_task_id: "task-translate-batch-story",
		parent_task_type: "translate.release.batch",
		max_tokens: 900,
		attempt_count: 1,
		scheduler_wait_ms: 120,
		first_token_wait_ms: null,
		duration_ms: null,
		input_tokens: 880,
		output_tokens: null,
		cached_input_tokens: 420,
		total_tokens: null,
		input_messages_json: JSON.stringify([
			{
				role: "system",
				content:
					"You are an assistant that produces a Chinese summary for release changes.",
			},
			{
				role: "user",
				content: [
					"Summarize the following bullet points:",
					"1) added admin LLM scheduler status endpoint",
					"2) added per-call prompt/response/error observability",
					"3) added filters: status/source/requested_by/started_from/started_to",
				].join("\n"),
			},
			{
				role: "assistant",
				content: "收到，我会整理为简明中文摘要并保留重点结构。",
			},
			{
				role: "user",
				content: "请控制在 3-4 条要点内，并突出排障价值。",
			},
		]),
		output_messages_json: null,
		prompt_text: `system:
You are an assistant that produces a Chinese summary for release changes.
Return markdown only.

user:
Summarize the following bullet points:
1) added admin LLM scheduler status endpoint
2) added per-call prompt/response/error observability
3) added filters: status/source/requested_by/started_from/started_to`,
		response_text: null,
		error_text: null,
		created_at: "2026-02-27T06:12:00Z",
		started_at: "2026-02-27T06:12:00Z",
		finished_at: null,
		updated_at: "2026-02-27T06:12:00Z",
	},
];

const translationCompletedWorkersSeed = [
	{
		worker_id: "translation-worker-1",
		worker_slot: 1,
		worker_kind: "general",
		status: "idle",
		current_batch_id: null,
		request_count: 0,
		work_item_count: 0,
		trigger_reason: null,
		updated_at: "2026-02-26T04:00:03Z",
		error_text: null,
	},
	{
		worker_id: "translation-worker-2",
		worker_slot: 2,
		worker_kind: "general",
		status: "idle",
		current_batch_id: null,
		request_count: 0,
		work_item_count: 0,
		trigger_reason: null,
		updated_at: "2026-02-26T04:00:03Z",
		error_text: null,
	},
	{
		worker_id: "translation-worker-3",
		worker_slot: 3,
		worker_kind: "general",
		status: "idle",
		current_batch_id: null,
		request_count: 0,
		work_item_count: 0,
		trigger_reason: null,
		updated_at: "2026-02-26T04:00:03Z",
		error_text: null,
	},
	{
		worker_id: "translation-worker-4",
		worker_slot: 4,
		worker_kind: "user_dedicated",
		status: "idle",
		current_batch_id: null,
		request_count: 0,
		work_item_count: 0,
		trigger_reason: null,
		updated_at: "2026-02-26T04:00:03Z",
		error_text: null,
	},
];

const translationBusyWorkersSeed = [
	{
		worker_id: "translation-worker-1",
		worker_slot: 1,
		worker_kind: "general",
		status: "running",
		current_batch_id: "batch-translation-story",
		request_count: 2,
		work_item_count: 3,
		trigger_reason: "token_threshold",
		updated_at: RUNNING_WORKER_UPDATED_AT,
		error_text: null,
	},
	{
		worker_id: "translation-worker-2",
		worker_slot: 2,
		worker_kind: "general",
		status: "idle",
		current_batch_id: null,
		request_count: 0,
		work_item_count: 0,
		trigger_reason: null,
		updated_at: "2026-02-26T04:00:02Z",
		error_text: null,
	},
	{
		worker_id: "translation-worker-3",
		worker_slot: 3,
		worker_kind: "general",
		status: "error",
		current_batch_id: null,
		request_count: 0,
		work_item_count: 0,
		trigger_reason: null,
		updated_at: ERROR_WORKER_UPDATED_AT,
		error_text: "claim retry",
	},
	{
		worker_id: "translation-worker-4",
		worker_slot: 4,
		worker_kind: "user_dedicated",
		status: "running",
		current_batch_id: "batch-translation-user-story",
		request_count: 1,
		work_item_count: 1,
		trigger_reason: "deadline",
		updated_at: RUNNING_WORKER_UPDATED_AT,
		error_text: null,
	},
];

const translationRequestSeed = {
	id: "req-translation-story",
	status: "completed",
	source: "feed.auto_translate",
	request_origin: "user",
	requested_by: CURRENT_USER_ID,
	scope_user_id: CURRENT_USER_ID,
	item_count: 1,
	completed_item_count: 1,
	created_at: "2026-02-26T04:00:00Z",
	started_at: "2026-02-26T04:00:01Z",
	finished_at: "2026-02-26T04:00:03Z",
	updated_at: "2026-02-26T04:00:03Z",
};

const translationRequestItemSeed = {
	producer_ref: "290978079",
	entity_id: "290978079",
	kind: "release_summary",
	variant: "feed_card",
	status: "ready",
	title_zh: "发布说明 290978079",
	summary_md: "- 修复了调度窗口\n- 保持整组返回",
	body_md: null,
	error: null,
	work_item_id: "work-translation-story",
	batch_id: "batch-translation-story",
};

const translationBatchSeed = {
	id: "batch-translation-story",
	status: "completed",
	trigger_reason: "deadline",
	worker_slot: 4,
	request_count: 1,
	item_count: 1,
	estimated_input_tokens: 512,
	created_at: "2026-02-26T04:00:01Z",
	started_at: "2026-02-26T04:00:01Z",
	finished_at: "2026-02-26T04:00:03Z",
	updated_at: "2026-02-26T04:00:03Z",
};

const translationBatchDetailSeed = {
	batch: translationBatchSeed,
	items: [translationRequestItemSeed],
	llm_calls: [
		{
			id: "llm-translation-story",
			status: "succeeded",
			source: "translation.scheduler.deadline",
			model: "gpt-4o-mini",
			scheduler_wait_ms: 240,
			duration_ms: 820,
			created_at: "2026-02-26T04:00:01Z",
		},
	],
};

function buildTaskDetail(
	task: AdminRealtimeTaskItem,
): AdminRealtimeTaskDetailResponse {
	if (task.task_type === "sync.subscriptions") {
		return {
			task: {
				...task,
				payload_json: JSON.stringify({
					trigger: "schedule",
					schedule_key: "2026-02-27T14:30",
				}),
				result_json: JSON.stringify({
					skipped: false,
					skip_reason: null,
					star: {
						total_users: 12,
						succeeded_users: 11,
						failed_users: 1,
						total_repos: 340,
					},
					release: {
						total_repos: 128,
						succeeded_repos: 123,
						failed_repos: 5,
						candidate_failures: 7,
					},
					releases_written: 1840,
					critical_events: 6,
				}),
			},
			events: [
				{
					id: "evt-task-1001",
					event_type: "task.created",
					payload_json: JSON.stringify({ source: task.source }),
					created_at: task.created_at,
				},
				{
					id: "evt-task-1002",
					event_type: "task.running",
					payload_json: JSON.stringify({}),
					created_at: task.started_at ?? task.created_at,
				},
				{
					id: "evt-task-1003",
					event_type: "task.progress",
					payload_json: JSON.stringify({ stage: "collect", total_users: 12 }),
					created_at: task.started_at ?? task.created_at,
				},
				{
					id: "evt-task-1004",
					event_type: "task.progress",
					payload_json: JSON.stringify({
						stage: "star_summary",
						total_users: 12,
						succeeded_users: 11,
						failed_users: 1,
					}),
					created_at: task.started_at ?? task.created_at,
				},
				{
					id: "evt-task-1005",
					event_type: "task.progress",
					payload_json: JSON.stringify({
						stage: "repo_collect",
						total_repos: 128,
					}),
					created_at: task.updated_at,
				},
				{
					id: "evt-task-1006",
					event_type: "task.progress",
					payload_json: JSON.stringify({
						stage: "release_summary",
						total_repos: 128,
						succeeded_repos: 123,
						failed_repos: 5,
						releases_written: 1840,
					}),
					created_at: task.updated_at,
				},
				{
					id: "evt-task-1007",
					event_type: "task.completed",
					payload_json: JSON.stringify({
						status: task.status,
						error: task.error_message,
					}),
					created_at: task.finished_at ?? task.updated_at,
				},
			],
			diagnostics: {
				business_outcome: {
					code: "partial",
					label: "部分成功",
					message: "任务已完成，但存在失败或关键告警，请查看最近关键事件。",
				},
				sync_subscriptions: {
					trigger: "schedule",
					schedule_key: "2026-02-27T14:30",
					skipped: false,
					skip_reason: null,
					log_available: true,
					log_download_path:
						"/api/admin/jobs/realtime/task-subscription-1430/log",
					star: {
						total_users: 12,
						succeeded_users: 11,
						failed_users: 1,
						total_repos: 340,
					},
					release: {
						total_repos: 128,
						succeeded_repos: 123,
						failed_repos: 5,
						candidate_failures: 7,
					},
					releases_written: 1840,
					critical_events: 6,
					recent_events: [
						{
							id: "evt-sync-9",
							stage: "release",
							event_type: "repo_inaccessible",
							severity: "error",
							recoverable: false,
							attempt: 1,
							user_id: RECENT_EVENT_USER_ID,
							repo_id: 9001,
							repo_full_name: "octo/private-repo",
							message:
								"release sync candidate failed for octo/private-repo with user #4h6p9s3t5z8e2x4c",
							created_at: "2026-02-27T14:31:40Z",
						},
					],
				},
			},
		};
	}

	return {
		task: {
			...task,
			payload_json: JSON.stringify({
				repo_scope: "all",
				requested_by: task.requested_by,
			}),
			result_json:
				task.status === "succeeded"
					? JSON.stringify({ message: "ok" })
					: task.status === "failed"
						? JSON.stringify({ message: task.error_message ?? "error" })
						: null,
		},
		events: [
			{
				id: "evt-task-1001",
				event_type: "task.created",
				payload_json: JSON.stringify({ source: task.source }),
				created_at: task.created_at,
			},
			{
				id: "evt-task-1002",
				event_type: "task.running",
				payload_json: JSON.stringify({}),
				created_at: task.started_at ?? task.created_at,
			},
			{
				id: "evt-task-1003",
				event_type: "task.completed",
				payload_json: JSON.stringify({
					status: task.status,
					error: task.error_message,
				}),
				created_at: task.finished_at ?? task.updated_at,
			},
		],
	};
}

function applyStatusFilter(
	items: AdminRealtimeTaskItem[],
	status: string | null,
) {
	if (!status || status === "all") return items;
	return items.filter((item) => item.status === status);
}

function paginate<T>(
	items: T[],
	pageRaw: string | null,
	pageSizeRaw: string | null,
) {
	const page = Math.max(1, Number(pageRaw ?? "1") || 1);
	const pageSize = Math.max(1, Number(pageSizeRaw ?? "20") || 20);
	const start = (page - 1) * pageSize;
	const pageItems = items.slice(start, start + pageSize);
	return { page, pageSize, pageItems, total: items.length };
}

type AdminJobsPreviewProps = {
	autoOpenConversation?: boolean;
	autoOpenTaskDrawer?: boolean;
	autoOpenTaskDrawerLlmRoute?: boolean;
	initialTab?: "scheduled" | "llm" | "translations";
	llmSourceFilter?: string;
	translationState?: "default" | "busy";
};

function setInputValue(element: HTMLInputElement, value: string) {
	const setter = Object.getOwnPropertyDescriptor(
		window.HTMLInputElement.prototype,
		"value",
	)?.set;
	if (setter) {
		setter.call(element, value);
	} else {
		element.value = value;
	}
	element.dispatchEvent(new Event("input", { bubbles: true }));
	element.dispatchEvent(new Event("change", { bubbles: true }));
}

function AdminJobsPreview({
	autoOpenConversation = false,
	autoOpenTaskDrawer = false,
	autoOpenTaskDrawerLlmRoute = false,
	initialTab,
	llmSourceFilter = "",
	translationState = "default",
}: AdminJobsPreviewProps) {
	const [ready, setReady] = useState(false);
	const autoOpenedRef = useRef(false);

	useEffect(() => {
		const originalFetch = window.fetch.bind(window);
		const originalEventSource = window.EventSource;
		const realtimeTasks = realtimeTasksSeed.map((item) => ({ ...item }));
		const scheduledRuns = scheduledRunsSeed.map((item) => ({ ...item }));
		const llmCalls = llmCallsSeed.map((item) => ({ ...item }));
		const translationWorkers =
			translationState === "busy"
				? translationBusyWorkersSeed.map((item) => ({ ...item }))
				: translationCompletedWorkersSeed.map((item) => ({ ...item }));
		const translationRequests = [
			{
				...translationRequestSeed,
				status: translationState === "busy" ? "running" : "completed",
				completed_item_count: translationState === "busy" ? 0 : 1,
				started_at:
					translationState === "busy"
						? "2026-02-26T04:00:01Z"
						: translationRequestSeed.started_at,
				finished_at:
					translationState === "busy"
						? null
						: translationRequestSeed.finished_at,
				updated_at:
					translationState === "busy"
						? "2026-02-26T04:00:02Z"
						: translationRequestSeed.updated_at,
			},
		];
		const translationBatches =
			translationState === "busy"
				? [
						{
							...translationBatchSeed,
							id: "batch-translation-user-story",
							status: "running",
							trigger_reason: "deadline",
							updated_at: "2026-02-26T04:00:02Z",
							finished_at: null,
						},
					]
				: [translationBatchSeed];

		window.fetch = async (input, init) => {
			const req =
				typeof input === "string" || input instanceof URL
					? new Request(input, init)
					: input;
			const url = new URL(req.url, window.location.origin);

			if (url.pathname === "/api/admin/jobs/overview" && req.method === "GET") {
				const queued = [...realtimeTasks, ...scheduledRuns].filter(
					(item) => item.status === "queued",
				).length;
				const running = [...realtimeTasks, ...scheduledRuns].filter(
					(item) => item.status === "running",
				).length;
				const succeeded24h = [...realtimeTasks, ...scheduledRuns].filter(
					(item) => item.status === "succeeded",
				).length;
				const failed24h = [...realtimeTasks, ...scheduledRuns].filter(
					(item) => item.status === "failed",
				).length;
				return new Response(
					JSON.stringify({
						queued,
						running,
						succeeded_24h: succeeded24h,
						failed_24h: failed24h,
						enabled_scheduled_slots: 24,
						total_scheduled_slots: 24,
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}

			if (url.pathname === "/api/admin/jobs/realtime" && req.method === "GET") {
				const taskType = url.searchParams.get("task_type");
				const excludeTaskType = url.searchParams.get("exclude_task_type");
				const taskGroup = url.searchParams.get("task_group");
				const status = url.searchParams.get("status");
				const page = url.searchParams.get("page");
				const pageSize = url.searchParams.get("page_size");

				let rows = [...realtimeTasks, ...scheduledRuns];
				if (taskGroup === "scheduled") {
					rows = rows.filter((item) =>
						["brief.daily_slot", "sync.subscriptions"].includes(item.task_type),
					);
				} else if (taskGroup === "realtime") {
					rows = rows.filter(
						(item) =>
							!["brief.daily_slot", "sync.subscriptions"].includes(
								item.task_type,
							),
					);
				} else if (taskType === "brief.daily_slot") {
					rows = [...scheduledRuns];
				}
				if (excludeTaskType) {
					rows = rows.filter((item) => item.task_type !== excludeTaskType);
				}
				rows = applyStatusFilter(rows, status);
				const { pageItems, total } = paginate(rows, page, pageSize);
				return new Response(
					JSON.stringify({
						items: pageItems,
						page: Number(page ?? "1") || 1,
						page_size: Number(pageSize ?? "20") || 20,
						total,
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}

			if (
				url.pathname.endsWith("/log") &&
				url.pathname.startsWith("/api/admin/jobs/realtime/") &&
				req.method === "GET"
			) {
				return new Response('{"line":1}\n{"line":2}\n', {
					status: 200,
					headers: { "content-type": "application/x-ndjson" },
				});
			}

			if (
				url.pathname.startsWith("/api/admin/jobs/realtime/") &&
				req.method === "GET" &&
				!url.pathname.endsWith("/log")
			) {
				const taskId = decodeURIComponent(url.pathname.split("/").at(-1) ?? "");
				const task = [...realtimeTasks, ...scheduledRuns].find(
					(item) => item.id === taskId,
				);
				if (!task) {
					return new Response(
						JSON.stringify({
							ok: false,
							error: { code: "not_found", message: "task not found" },
						}),
						{
							status: 404,
							headers: { "content-type": "application/json" },
						},
					);
				}
				return new Response(JSON.stringify(buildTaskDetail(task)), {
					status: 200,
					headers: { "content-type": "application/json" },
				});
			}

			if (
				url.pathname.endsWith("/retry") &&
				url.pathname.startsWith("/api/admin/jobs/realtime/") &&
				req.method === "POST"
			) {
				const taskId = decodeURIComponent(
					url.pathname.replace("/retry", "").split("/").at(-1) ?? "",
				);
				return new Response(
					JSON.stringify({
						task_id: taskId,
						status: "queued",
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}

			if (
				url.pathname.endsWith("/cancel") &&
				url.pathname.startsWith("/api/admin/jobs/realtime/") &&
				req.method === "POST"
			) {
				const taskId = decodeURIComponent(
					url.pathname.replace("/cancel", "").split("/").at(-1) ?? "",
				);
				return new Response(
					JSON.stringify({
						task_id: taskId,
						status: "canceled",
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}

			if (
				url.pathname === "/api/admin/jobs/llm/status" &&
				req.method === "GET"
			) {
				return new Response(
					JSON.stringify({
						scheduler_enabled: true,
						max_concurrency: 2,
						waiting_calls: 1,
						in_flight_calls: 1,
						available_slots: 1,
						calls_24h: llmCalls.length,
						failed_24h: llmCalls.filter((item) => item.status === "failed")
							.length,
						avg_wait_ms_24h: 810,
						avg_duration_ms_24h: 1570,
						last_success_at: llmCalls.at(-1)?.finished_at ?? null,
						last_failure_at: llmCalls[0]?.finished_at ?? null,
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}

			if (
				url.pathname === "/api/admin/jobs/llm/calls" &&
				req.method === "GET"
			) {
				const status = url.searchParams.get("status");
				const source = url.searchParams.get("source");
				const requestedBy = url.searchParams.get("requested_by");
				const parentTaskId = url.searchParams.get("parent_task_id");
				let rows = [...llmCalls];
				if (status && status !== "all") {
					rows = rows.filter((item) => item.status === status);
				}
				if (source) {
					rows = rows.filter((item) => item.source === source);
				}
				if (requestedBy) {
					rows = rows.filter(
						(item) => String(item.requested_by ?? "") === String(requestedBy),
					);
				}
				if (parentTaskId) {
					rows = rows.filter((item) => item.parent_task_id === parentTaskId);
				}
				const page = url.searchParams.get("page");
				const pageSize = url.searchParams.get("page_size");
				const { pageItems, total } = paginate(rows, page, pageSize);
				return new Response(
					JSON.stringify({
						items: pageItems.map(
							({
								prompt_text: _p,
								response_text: _r,
								error_text: _e,
								input_messages_json: _im,
								output_messages_json: _om,
								...rest
							}) => rest,
						),
						page: Number(page ?? "1") || 1,
						page_size: Number(pageSize ?? "20") || 20,
						total,
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}

			if (
				url.pathname.startsWith("/api/admin/jobs/llm/calls/") &&
				req.method === "GET"
			) {
				const callId = decodeURIComponent(url.pathname.split("/").at(-1) ?? "");
				const item = llmCalls.find((call) => call.id === callId);
				if (!item) {
					return new Response(
						JSON.stringify({
							ok: false,
							error: { code: "not_found", message: "llm call not found" },
						}),
						{
							status: 404,
							headers: { "content-type": "application/json" },
						},
					);
				}
				return new Response(JSON.stringify(item), {
					status: 200,
					headers: { "content-type": "application/json" },
				});
			}

			if (
				url.pathname === "/api/admin/jobs/translations/status" &&
				req.method === "GET"
			) {
				const busyWorkers = translationWorkers.filter(
					(worker) => worker.status === "running",
				).length;
				const idleWorkers = translationWorkers.filter(
					(worker) => worker.status === "idle",
				).length;
				return new Response(
					JSON.stringify({
						scheduler_enabled: true,
						llm_enabled: true,
						scan_interval_ms: 250,
						batch_token_threshold: 1800,
						worker_concurrency: 4,
						idle_workers: idleWorkers,
						busy_workers: busyWorkers,
						workers: translationWorkers,
						queued_requests: translationState === "busy" ? 1 : 0,
						queued_work_items: translationState === "busy" ? 1 : 0,
						running_batches: translationState === "busy" ? 1 : 0,
						requests_24h: 1,
						completed_batches_24h: translationState === "busy" ? 0 : 1,
						failed_batches_24h: 0,
						avg_wait_ms_24h: translationState === "busy" ? null : 320,
						last_batch_finished_at:
							translationState === "busy" ? null : "2026-02-26T04:00:03Z",
					}),
					{ status: 200, headers: { "content-type": "application/json" } },
				);
			}

			if (
				url.pathname === "/api/admin/jobs/translations/requests" &&
				req.method === "GET"
			) {
				return new Response(
					JSON.stringify({
						items: translationRequests,
						page: 1,
						page_size: 20,
						total: translationRequests.length,
					}),
					{ status: 200, headers: { "content-type": "application/json" } },
				);
			}

			if (
				url.pathname.startsWith("/api/admin/jobs/translations/requests/") &&
				req.method === "GET"
			) {
				return new Response(
					JSON.stringify({
						request: translationRequests[0],
						items: [translationRequestItemSeed],
					}),
					{ status: 200, headers: { "content-type": "application/json" } },
				);
			}

			if (
				url.pathname === "/api/admin/jobs/translations/batches" &&
				req.method === "GET"
			) {
				return new Response(
					JSON.stringify({
						items: translationBatches,
						page: 1,
						page_size: 20,
						total: translationBatches.length,
					}),
					{ status: 200, headers: { "content-type": "application/json" } },
				);
			}

			if (
				url.pathname.startsWith("/api/admin/jobs/translations/batches/") &&
				req.method === "GET"
			) {
				return new Response(JSON.stringify(translationBatchDetailSeed), {
					status: 200,
					headers: { "content-type": "application/json" },
				});
			}

			return originalFetch(input, init);
		};

		class MockEventSource {
			onopen: ((this: EventSource, event: Event) => unknown) | null = null;
			onmessage: ((this: EventSource, event: MessageEvent) => unknown) | null =
				null;
			onerror: ((this: EventSource, event: Event) => unknown) | null = null;
			readonly withCredentials = true;
			readonly CONNECTING = 0;
			readonly OPEN = 1;
			readonly CLOSED = 2;
			readyState = 1;
			private readonly listeners = new Map<
				string,
				Set<(event: Event) => unknown>
			>();
			private readonly timers: number[] = [];

			constructor(_url: string, _init?: EventSourceInit) {
				const openTimer = window.setTimeout(() => {
					this.onopen?.call(this as unknown as EventSource, new Event("open"));
				}, 10);
				this.timers.push(openTimer);
				const llmUpdateTimer = window.setTimeout(() => {
					const target = llmCalls.find((item) => item.id === "llm-call-2");
					if (!target) return;
					target.status = "succeeded";
					target.first_token_wait_ms = 160;
					target.duration_ms = 540;
					target.output_tokens = 198;
					target.total_tokens = 1078;
					target.output_messages_json = JSON.stringify([
						{
							role: "assistant",
							content: [
								"### 本次更新摘要",
								"",
								"- 新增管理员可见的 LLM 调度状态接口，便于观察等待与并发情况。",
								"- 新增逐次调用日志，记录 prompt、response、error、等待耗时与重试次数。",
								"- 新增筛选能力（状态 / 来源 / 用户 / 起止时间），便于快速定位异常调用。",
							].join("\n"),
						},
					]);
					target.response_text = `### 本次更新摘要

- 新增管理员可见的 LLM 调度状态接口，便于观察等待与并发情况。
- 新增逐次调用日志，记录 prompt、response、error、等待耗时与重试次数。
- 新增筛选能力（状态 / 来源 / 用户 / 起止时间），便于快速定位异常调用。`;
					target.finished_at = "2026-02-27T06:12:01Z";
					target.updated_at = "2026-02-27T06:12:01Z";
					this.emit("llm.call", {
						event_id: "evt-stream-2001",
						call_id: "llm-call-2",
						status: target.status,
						source: target.source,
						requested_by: target.requested_by,
						parent_task_id: target.parent_task_id,
						event_type: "llm.succeeded",
						created_at: target.updated_at,
					});
				}, 1600);
				this.timers.push(llmUpdateTimer);
			}

			close() {
				this.readyState = 2;
				for (const timer of this.timers) {
					window.clearTimeout(timer);
				}
				this.timers.length = 0;
			}

			addEventListener(type: string, listener: (event: Event) => unknown) {
				if (!this.listeners.has(type)) {
					this.listeners.set(type, new Set());
				}
				this.listeners.get(type)?.add(listener);
			}
			removeEventListener(type: string, listener: (event: Event) => unknown) {
				this.listeners.get(type)?.delete(listener);
			}
			dispatchEvent() {
				return true;
			}

			private emit(type: string, payload: unknown) {
				if (this.readyState === this.CLOSED) return;
				const event = new MessageEvent(type, {
					data: JSON.stringify(payload),
				});
				for (const listener of this.listeners.get(type) ?? []) {
					listener.call(this as unknown as EventSource, event);
				}
				this.onmessage?.call(this as unknown as EventSource, event);
			}
		}

		window.EventSource = MockEventSource as unknown as typeof EventSource;
		setReady(true);

		return () => {
			window.fetch = originalFetch;
			window.EventSource = originalEventSource;
		};
	}, [translationState]);

	useEffect(() => {
		const needsTabPrep = Boolean(initialTab) || Boolean(llmSourceFilter);
		if (
			!ready ||
			autoOpenedRef.current ||
			(!autoOpenConversation && !needsTabPrep)
		) {
			return;
		}
		autoOpenedRef.current = true;
		const openTimer = window.setTimeout(() => {
			const targetTabLabel =
				initialTab === "scheduled"
					? "定时任务"
					: initialTab === "translations"
						? "翻译调度"
						: initialTab === "llm" || autoOpenConversation || llmSourceFilter
							? "LLM调度"
							: null;
			if (!targetTabLabel) {
				return;
			}
			const targetTab = Array.from(document.querySelectorAll("button")).find(
				(node) => node.textContent?.trim() === targetTabLabel,
			) as HTMLButtonElement | undefined;
			targetTab?.click();
			if (targetTabLabel !== "LLM调度") {
				return;
			}
			window.setTimeout(() => {
				const sourceInput = document.querySelector(
					'input[placeholder="来源（source）"]',
				) as HTMLInputElement | null;
				if (sourceInput && (autoOpenConversation || llmSourceFilter)) {
					setInputValue(
						sourceInput,
						llmSourceFilter || "job.api.translate_release",
					);
				}
				if (!autoOpenConversation) {
					return;
				}
				window.setTimeout(() => {
					const detailButton = Array.from(
						document.querySelectorAll("button"),
					).find((node) => node.textContent?.trim() === "详情") as
						| HTMLButtonElement
						| undefined;
					detailButton?.click();
				}, 80);
			}, 80);
		}, 80);
		return () => {
			window.clearTimeout(openTimer);
		};
	}, [ready, autoOpenConversation, initialTab, llmSourceFilter]);

	useEffect(() => {
		if (
			!ready ||
			(!autoOpenTaskDrawer && !autoOpenTaskDrawerLlmRoute) ||
			autoOpenedRef.current
		) {
			return;
		}
		autoOpenedRef.current = true;
		let llmDetailPollTimer: number | null = null;
		const openTimer = window.setTimeout(() => {
			const targetTask = Array.from(document.querySelectorAll("p")).find(
				(node) => node.textContent?.includes("ID: task-translate-batch-story"),
			);
			let taskCard: HTMLElement | null =
				targetTask instanceof HTMLElement ? targetTask : null;
			while (taskCard) {
				const hasDetailButton = Array.from(
					taskCard.querySelectorAll("button"),
				).some((node) => node.textContent?.trim() === "详情");
				if (hasDetailButton) {
					break;
				}
				taskCard = taskCard.parentElement;
			}
			const detailButton = Array.from(
				taskCard?.querySelectorAll("button") ?? [],
			).find((node) => node.textContent?.trim() === "详情");
			detailButton?.click();
			if (!autoOpenTaskDrawerLlmRoute) {
				return;
			}
			let attempts = 0;
			llmDetailPollTimer = window.setInterval(() => {
				const llmDetailButton = Array.from(
					document.querySelectorAll("button"),
				).find((node) => node.textContent?.trim() === "查看 LLM 详情") as
					| HTMLButtonElement
					| undefined;
				if (llmDetailButton) {
					llmDetailButton.click();
					if (llmDetailPollTimer !== null) {
						window.clearInterval(llmDetailPollTimer);
						llmDetailPollTimer = null;
					}
					return;
				}
				attempts += 1;
				if (attempts >= 30 && llmDetailPollTimer !== null) {
					window.clearInterval(llmDetailPollTimer);
					llmDetailPollTimer = null;
				}
			}, 120);
		}, 120);
		return () => {
			window.clearTimeout(openTimer);
			if (llmDetailPollTimer !== null) {
				window.clearInterval(llmDetailPollTimer);
			}
		};
	}, [ready, autoOpenTaskDrawer, autoOpenTaskDrawerLlmRoute]);

	if (!ready) return null;

	return (
		<AdminJobs
			me={{
				user: {
					id: CURRENT_USER_ID,
					github_user_id: 10,
					login: "storybook-admin",
					name: "Storybook Admin",
					avatar_url: null,
					email: "admin@example.com",
					is_admin: true,
				},
			}}
		/>
	);
}

const meta = {
	title: "Admin/Admin Jobs",
	component: AdminJobsPreview,
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"后台任务中心，覆盖实时任务、计划任务、LLM 调用、任务抽屉与会话详情。适合验证可观测性页面在复杂状态下的切换、筛选与细节路由。\n\n相关公开文档：[产品说明](../product.html) · [配置参考](../config.html)",
			},
		},
	},
} satisfies Meta<typeof AdminJobsPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	parameters: {
		docs: {
			description: {
				story: "默认实时任务视图，包含运行中、成功与批量翻译任务的基线状态。",
			},
		},
	},
};

export const ScheduledTab: Story = {
	render: () => <AdminJobsPreview initialTab="scheduled" />,
	parameters: {
		docs: {
			description: {
				story: "切到计划任务页签，检查日报与订阅同步等调度任务的呈现。",
			},
		},
	},
};

export const LlmFilters: Story = {
	render: () => (
		<AdminJobsPreview
			initialTab="llm"
			llmSourceFilter="job.api.translate_release"
		/>
	),
	parameters: {
		docs: {
			description: {
				story: "聚焦 LLM 调用筛选器生效后的列表视图。",
			},
		},
	},
};

export const LlmConversationDetail: Story = {
	render: () => <AdminJobsPreview autoOpenConversation />,
	parameters: {
		docs: {
			description: {
				story: "直接展示 LLM 会话详情，检查 prompt/response 链路是否可读。",
			},
		},
	},
};

export const TaskDrawerDetail: Story = {
	render: () => <AdminJobsPreview autoOpenTaskDrawer />,
	parameters: {
		docs: {
			description: {
				story: "打开任务详情抽屉，验证任务元信息、参数与结果摘要布局。",
			},
		},
	},
};

export const TaskDrawerLlmRoute: Story = {
	render: () => <AdminJobsPreview autoOpenTaskDrawerLlmRoute />,
	parameters: {
		docs: {
			description: {
				story: "验证任务抽屉内继续钻取到 LLM 详情路由时的组合状态。",
			},
		},
	},
};

export const TranslationWorkerBoard: Story = {
	render: () => <AdminJobsPreview initialTab="translations" />,
};

export const TranslationWorkerBoardBusy: Story = {
	render: () => (
		<AdminJobsPreview initialTab="translations" translationState="busy" />
	),
};

export const TranslationWorkerBoardMobile: Story = {
	render: () => (
		<div className="mx-auto max-w-sm">
			<AdminJobsPreview initialTab="translations" translationState="busy" />
		</div>
	),
};
