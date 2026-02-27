import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useState } from "react";

import type {
	AdminRealtimeTaskDetailResponse,
	AdminRealtimeTaskItem,
} from "@/api";
import { AdminJobs } from "@/pages/AdminJobs";

const realtimeTasksSeed: AdminRealtimeTaskItem[] = [
	{
		id: "task-sync-releases",
		task_type: "sync.releases",
		status: "running",
		source: "manual",
		requested_by: 1,
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
		requested_by: 1,
		parent_task_id: null,
		cancel_requested: false,
		error_message: null,
		created_at: "2026-02-27T05:54:00Z",
		started_at: "2026-02-27T05:54:02Z",
		finished_at: "2026-02-27T05:54:13Z",
		updated_at: "2026-02-27T05:54:13Z",
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
];

const llmCallsSeed = [
	{
		id: "llm-call-1",
		status: "failed",
		source: "job.api.translate_release",
		model: "gpt-4o-mini",
		requested_by: 1,
		parent_task_id: "task-sync-releases",
		parent_task_type: "sync.releases",
		max_tokens: 900,
		attempt_count: 3,
		scheduler_wait_ms: 1500,
		duration_ms: 2600,
		prompt_text: "prompt 1",
		response_text: null,
		error_text: "mock llm failed",
		created_at: "2026-02-27T06:10:00Z",
		started_at: "2026-02-27T06:10:01Z",
		finished_at: "2026-02-27T06:10:04Z",
		updated_at: "2026-02-27T06:10:04Z",
	},
	{
		id: "llm-call-2",
		status: "succeeded",
		source: "api.translate_releases_batch",
		model: "gpt-4o-mini",
		requested_by: 1,
		parent_task_id: null,
		parent_task_type: null,
		max_tokens: 900,
		attempt_count: 1,
		scheduler_wait_ms: 120,
		duration_ms: 540,
		prompt_text: "prompt 2",
		response_text: "ok",
		error_text: null,
		created_at: "2026-02-27T06:12:00Z",
		started_at: "2026-02-27T06:12:00Z",
		finished_at: "2026-02-27T06:12:01Z",
		updated_at: "2026-02-27T06:12:01Z",
	},
];

function buildTaskDetail(
	task: AdminRealtimeTaskItem,
): AdminRealtimeTaskDetailResponse {
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
				id: 1001,
				event_type: "task.created",
				payload_json: JSON.stringify({ source: task.source }),
				created_at: task.created_at,
			},
			{
				id: 1002,
				event_type: "task.running",
				payload_json: JSON.stringify({}),
				created_at: task.started_at ?? task.created_at,
			},
			{
				id: 1003,
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

function AdminJobsPreview() {
	const [ready, setReady] = useState(false);

	useEffect(() => {
		const originalFetch = window.fetch.bind(window);
		const originalEventSource = window.EventSource;
		const realtimeTasks = realtimeTasksSeed.map((item) => ({ ...item }));
		const scheduledRuns = scheduledRunsSeed.map((item) => ({ ...item }));
		const llmCalls = llmCallsSeed.map((item) => ({ ...item }));

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
				const status = url.searchParams.get("status");
				const page = url.searchParams.get("page");
				const pageSize = url.searchParams.get("page_size");

				let rows =
					taskType === "brief.daily_slot"
						? [...scheduledRuns]
						: [...realtimeTasks, ...scheduledRuns];
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
				url.pathname.startsWith("/api/admin/jobs/realtime/") &&
				req.method === "GET"
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
						request_interval_ms: 1000,
						waiting_calls: 1,
						in_flight_calls: 1,
						next_slot_in_ms: 450,
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

			constructor(_url: string, _init?: EventSourceInit) {
				window.setTimeout(() => {
					this.onopen?.call(this as unknown as EventSource, new Event("open"));
				}, 10);
			}

			close() {
				this.readyState = 2;
			}

			addEventListener() {}
			removeEventListener() {}
			dispatchEvent() {
				return true;
			}
		}

		window.EventSource = MockEventSource as unknown as typeof EventSource;
		setReady(true);

		return () => {
			window.fetch = originalFetch;
			window.EventSource = originalEventSource;
		};
	}, []);

	if (!ready) return null;

	return (
		<AdminJobs
			me={{
				user: {
					id: 1,
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
	title: "Pages/AdminJobs",
	component: AdminJobsPreview,
	parameters: {
		layout: "fullscreen",
	},
} satisfies Meta<typeof AdminJobsPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
