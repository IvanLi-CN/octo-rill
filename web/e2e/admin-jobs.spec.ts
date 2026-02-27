import { type Page, type Route, expect, test } from "@playwright/test";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

async function installAdminJobsMocks(page: Page) {
	const tasks = [
		{
			id: "task-failed-1",
			task_type: "brief.daily_slot",
			status: "failed",
			source: "scheduler",
			requested_by: null,
			parent_task_id: null,
			cancel_requested: false,
			error_message: "mock failed",
			created_at: "2026-02-26T00:00:00Z",
			started_at: "2026-02-26T00:00:10Z",
			finished_at: "2026-02-26T00:01:00Z",
			updated_at: "2026-02-26T00:01:00Z",
		},
		{
			id: "task-running-1",
			task_type: "sync.releases",
			status: "running",
			source: "api.sync_releases",
			requested_by: 1,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-02-26T01:00:00Z",
			started_at: "2026-02-26T01:00:05Z",
			finished_at: null,
			updated_at: "2026-02-26T01:00:20Z",
		},
	];

	const llmCalls = [
		{
			id: "llm-call-1",
			status: "failed",
			source: "job.api.translate_release",
			model: "gpt-4o-mini",
			requested_by: 1,
			parent_task_id: "task-running-1",
			parent_task_type: "sync.releases",
			max_tokens: 900,
			attempt_count: 3,
			scheduler_wait_ms: 1200,
			first_token_wait_ms: 860,
			duration_ms: 2200,
			input_tokens: 1230,
			output_tokens: 0,
			cached_input_tokens: 640,
			total_tokens: 1230,
			input_messages_json: JSON.stringify([
				{ role: "system", content: "You are a release translator." },
				{ role: "user", content: "translate notes to Chinese" },
				{ role: "assistant", content: "收到，我将输出三条重点。" },
				{ role: "user", content: "请强调排障价值" },
			]),
			output_messages_json: null,
			prompt_text: "prompt 1",
			response_text: null,
			error_text: "mock llm failed",
			created_at: "2026-02-26T02:00:00Z",
			started_at: "2026-02-26T02:00:01Z",
			finished_at: "2026-02-26T02:00:03Z",
			updated_at: "2026-02-26T02:00:03Z",
		},
		{
			id: "llm-call-2",
			status: "running",
			source: "api.translate_releases_batch",
			model: "gpt-4o-mini",
			requested_by: 1,
			parent_task_id: null,
			parent_task_type: null,
			max_tokens: 900,
			attempt_count: 1,
			scheduler_wait_ms: 80,
			first_token_wait_ms: null,
			duration_ms: null,
			input_tokens: 780,
			output_tokens: null,
			cached_input_tokens: 320,
			total_tokens: null,
			input_messages_json: JSON.stringify([
				{ role: "system", content: "You are a summary assistant." },
				{ role: "user", content: "summarize changes" },
				{ role: "assistant", content: "收到，我将输出三条重点。" },
				{ role: "user", content: "请强调排障价值" },
			]),
			output_messages_json: null,
			prompt_text: "prompt 2",
			response_text: null,
			error_text: null,
			created_at: "2026-02-26T03:00:00Z",
			started_at: "2026-02-26T03:00:00Z",
			finished_at: null,
			updated_at: "2026-02-26T03:00:00Z",
		},
	];

	const slots = Array.from({ length: 24 }, (_, hour) => ({
		hour_utc: hour,
		enabled: hour % 2 === 0,
		last_dispatch_at: "2026-02-26T00:00:00Z",
		updated_at: "2026-02-26T00:00:00Z",
	}));

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(route, {
				user: {
					id: 1,
					github_user_id: 10,
					login: "octo-admin",
					name: "Octo Admin",
					avatar_url: null,
					email: "admin@example.com",
					is_admin: true,
				},
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/overview") {
			return json(route, {
				queued: 2,
				running: 1,
				failed_24h: 1,
				succeeded_24h: 3,
				enabled_scheduled_slots: slots.filter((slot) => slot.enabled).length,
				total_scheduled_slots: slots.length,
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/realtime") {
			const status = url.searchParams.get("status") ?? "all";
			const taskType = url.searchParams.get("task_type") ?? "";
			const excludeTaskType = url.searchParams.get("exclude_task_type") ?? "";
			const filtered = tasks.filter((task) => {
				if (status !== "all" && task.status !== status) return false;
				if (taskType && task.task_type !== taskType) return false;
				if (excludeTaskType && task.task_type === excludeTaskType) return false;
				return true;
			});
			return json(route, {
				items: filtered,
				page: 1,
				page_size: 20,
				total: filtered.length,
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/events") {
			const call = llmCalls.find((item) => item.id === "llm-call-2");
			if (call && call.status === "running") {
				call.status = "succeeded";
				call.first_token_wait_ms = 140;
				call.duration_ms = 400;
				call.output_tokens = 160;
				call.total_tokens = 940;
				call.output_messages_json = JSON.stringify([
					{
						role: "assistant",
						content: "- added scheduler status endpoint\n- added call logging",
					},
				]);
				call.response_text = "ok";
				call.finished_at = "2026-02-26T03:00:01Z";
				call.updated_at = "2026-02-26T03:00:01Z";
			}
			return route.fulfill({
				status: 200,
				contentType: "text/event-stream",
				body: [
					"event: job.event",
					`data: ${JSON.stringify({
						event_id: 9001,
						task_id: "task-running-1",
						task_type: "sync.releases",
						status: "running",
						event_type: "task.running",
						created_at: "2026-02-26T01:00:05Z",
					})}`,
					"",
					"event: llm.call",
					`data: ${JSON.stringify({
						event_id: 9101,
						call_id: "llm-call-2",
						status: "succeeded",
						source: "api.translate_releases_batch",
						requested_by: 1,
						parent_task_id: null,
						event_type: "llm.succeeded",
						created_at: "2026-02-26T03:00:01Z",
					})}`,
					"",
					"",
				].join("\n"),
			});
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/jobs/realtime/")
		) {
			const taskId = pathname.split("/").at(-1) ?? "";
			const task = tasks.find((item) => item.id === taskId);
			if (!task) {
				return json(
					route,
					{
						ok: false,
						error: { code: "not_found", message: "task not found" },
					},
					404,
				);
			}
			return json(route, {
				task,
				events: [
					{
						id: 1,
						event_type: "task.created",
						payload_json: JSON.stringify({
							task_id: task.id,
							status: task.status,
						}),
						created_at: "2026-02-26T00:00:00Z",
					},
				],
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/llm/status") {
			return json(route, {
				scheduler_enabled: true,
				request_interval_ms: 1000,
				waiting_calls: 1,
				in_flight_calls: 1,
				next_slot_in_ms: 420,
				calls_24h: llmCalls.length,
				failed_24h: llmCalls.filter((item) => item.status === "failed").length,
				avg_wait_ms_24h: 640,
				avg_duration_ms_24h: 1300,
				last_success_at: "2026-02-26T03:00:01Z",
				last_failure_at: "2026-02-26T02:00:03Z",
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/llm/calls") {
			const status = url.searchParams.get("status") ?? "all";
			const source = url.searchParams.get("source") ?? "";
			const requestedBy = url.searchParams.get("requested_by");
			const filtered = llmCalls.filter((item) => {
				if (status !== "all" && item.status !== status) return false;
				if (source && item.source !== source) return false;
				if (
					requestedBy &&
					String(item.requested_by ?? "") !== String(requestedBy)
				) {
					return false;
				}
				return true;
			});
			return json(route, {
				items: filtered.map(
					({
						prompt_text: _p,
						response_text: _r,
						error_text: _e,
						input_messages_json: _im,
						output_messages_json: _om,
						...rest
					}) => rest,
				),
				page: 1,
				page_size: 20,
				total: filtered.length,
			});
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/jobs/llm/calls/")
		) {
			const callId = pathname.split("/").at(-1) ?? "";
			const item = llmCalls.find((call) => call.id === callId);
			if (!item) {
				return json(
					route,
					{
						ok: false,
						error: { code: "not_found", message: "llm call not found" },
					},
					404,
				);
			}
			return json(route, item);
		}

		if (
			req.method() === "POST" &&
			pathname.startsWith("/api/admin/jobs/realtime/") &&
			pathname.endsWith("/retry")
		) {
			return json(route, { task_id: "retry-1", status: "queued" });
		}

		if (
			req.method() === "POST" &&
			pathname.startsWith("/api/admin/jobs/realtime/") &&
			pathname.endsWith("/cancel")
		) {
			return json(route, {
				task_id: pathname.split("/").at(-2),
				status: "running",
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/scheduled") {
			return json(route, { items: slots });
		}

		if (
			req.method() === "PATCH" &&
			pathname.startsWith("/api/admin/jobs/scheduled/")
		) {
			const hour = Number(pathname.split("/").at(-1));
			const body = req.postDataJSON() as { enabled: boolean };
			const target = slots.find((slot) => slot.hour_utc === hour);
			if (!target) {
				return json(
					route,
					{
						ok: false,
						error: { code: "not_found", message: "slot not found" },
					},
					404,
				);
			}
			target.enabled = body.enabled;
			return json(route, target);
		}

		return json(
			route,
			{ error: { message: `unhandled ${req.method()} ${pathname}` } },
			404,
		);
	});
}

test("admin can manage jobs center", async ({ page }) => {
	await installAdminJobsMocks(page);
	await page.goto("/admin/jobs");

	await expect(page).toHaveURL(/\/admin\/jobs$/);
	await expect(page.getByRole("heading", { name: "管理后台" })).toBeVisible();
	await expect(page.getByRole("heading", { name: "任务总览" })).toBeVisible();

	await expect(page.getByText("sync.releases")).toBeVisible();
	await expect(page.getByText("brief.daily_slot")).toHaveCount(0);
	await page.getByRole("button", { name: "详情" }).first().click();
	await expect(page.getByRole("heading", { name: "任务详情" })).toBeVisible();
	await page.getByRole("button", { name: "关闭", exact: true }).click();

	await page.getByRole("button", { name: "定时任务" }).click();
	await expect(page.getByText("运行记录")).toBeVisible();
	await expect(page.getByText("定时执行任务")).toBeVisible();
	await expect(page.getByText("执行时间配置（24小时槽）")).toHaveCount(0);

	await page.getByRole("button", { name: "LLM调度" }).click();
	await expect(page.getByRole("heading", { name: "LLM 调度" })).toBeVisible();
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible();
	await expect(page.getByText("Token 输入/输出/缓存").first()).toBeVisible();
	await page.getByRole("button", { name: "详情" }).first().click();
	await expect(
		page.getByRole("heading", { name: "LLM 调用详情" }),
	).toBeVisible();
	await expect(page.getByText("Conversation Timeline")).toBeVisible();
	await expect(page.getByText("Input Messages")).toHaveCount(0);
	await expect(page.getByText("耗时 / 重试")).toBeVisible();
	await expect(page.getByText("等待 / 首字 / 耗时 / 重试")).toHaveCount(0);
	await expect(
		page.getByText("等待 1.20s · 首字 860ms", { exact: true }),
	).toBeVisible();
	await expect(page.getByText("Token（输入 / 输出 / 缓存）")).toBeVisible();
	await page.getByRole("button", { name: "关闭", exact: true }).click();
});
