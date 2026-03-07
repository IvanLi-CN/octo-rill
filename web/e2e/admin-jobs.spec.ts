import { type Page, type Route, expect, test } from "@playwright/test";

type MockRequestRule = {
	pathname: string;
	search?: string;
	afterCount?: number;
	times?: number;
};

type MockDelayRule = MockRequestRule & {
	delayMs: number;
};

type MockFailureRule = MockRequestRule & {
	status?: number;
	message?: string;
};

type AdminJobsMockOptions = {
	responseDelayMs?: number;
	delayedPaths?: string[];
	delayRules?: MockDelayRule[];
	failureRules?: MockFailureRule[];
	emitStreamEvents?: boolean;
};

function sleep(ms: number) {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

async function installAdminJobsMocks(
	page: Page,
	options: AdminJobsMockOptions = {},
) {
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
		{
			id: "task-translate-batch-1",
			task_type: "translate.release.batch",
			status: "succeeded",
			source: "api.translate_releases_batch_stream",
			requested_by: 1,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-02-26T01:10:00Z",
			started_at: "2026-02-26T01:10:02Z",
			finished_at: "2026-02-26T01:10:40Z",
			updated_at: "2026-02-26T01:10:40Z",
		},
		{
			id: "task-subscriptions-1",
			task_type: "sync.subscriptions",
			status: "succeeded",
			source: "scheduler",
			requested_by: null,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-02-26T14:30:00Z",
			started_at: "2026-02-26T14:30:04Z",
			finished_at: "2026-02-26T14:38:10Z",
			updated_at: "2026-02-26T14:38:10Z",
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
			parent_task_id: "task-translate-batch-1",
			parent_task_type: "translate.release.batch",
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

	const translationRequests = [
		{
			id: "req-translation-1",
			status: "completed",
			source: "feed.auto_translate",
			requested_by: 1,
			scope_user_id: 1,
			item_count: 1,
			completed_item_count: 1,
			created_at: "2026-02-26T04:00:00Z",
			started_at: "2026-02-26T04:00:01Z",
			finished_at: "2026-02-26T04:00:03Z",
			updated_at: "2026-02-26T04:00:03Z",
		},
	];

	const translationRequestDetail = {
		request: translationRequests[0],
		items: [
			{
				producer_ref: "290978079",
				entity_id: "290978079",
				kind: "release_summary",
				variant: "feed_card",
				status: "ready",
				title_zh: "发布说明 290978079",
				summary_md: "- 修复了调度窗口\n- 保持整组返回",
				body_md: null,
				error: null,
				work_item_id: "work-translation-1",
				batch_id: "batch-translation-1",
			},
		],
	};

	const translationBatches = [
		{
			id: "batch-translation-1",
			status: "completed",
			trigger_reason: "deadline",
			item_count: 1,
			estimated_input_tokens: 512,
			created_at: "2026-02-26T04:00:01Z",
			started_at: "2026-02-26T04:00:01Z",
			finished_at: "2026-02-26T04:00:03Z",
			updated_at: "2026-02-26T04:00:03Z",
		},
	];

	const translationBatchDetail = {
		batch: translationBatches[0],
		items: translationRequestDetail.items,
		llm_calls: [
			{
				id: "llm-translation-1",
				status: "succeeded",
				source: "translation.scheduler.deadline",
				model: "gpt-4o-mini",
				scheduler_wait_ms: 240,
				duration_ms: 820,
				created_at: "2026-02-26T04:00:01Z",
			},
		],
	};

	const slots = Array.from({ length: 24 }, (_, hour) => ({
		hour_utc: hour,
		enabled: hour % 2 === 0,
		last_dispatch_at: "2026-02-26T00:00:00Z",
		updated_at: "2026-02-26T00:00:00Z",
	}));

	const delayedPathSet = new Set(options.delayedPaths ?? []);
	const requestCounts = new Map<string, number>();
	const delayRuleCounts = new Map<number, number>();
	const failureRuleCounts = new Map<number, number>();
	const emitStreamEvents = options.emitStreamEvents ?? true;

	function matchesRule(rule: MockRequestRule, url: URL) {
		if (rule.pathname !== url.pathname) {
			return false;
		}
		if (!rule.search) {
			return true;
		}
		return url.search.includes(rule.search);
	}

	function ruleApplies(
		rule: MockRequestRule,
		url: URL,
		ruleCounts: Map<number, number>,
		index: number,
	) {
		if (!matchesRule(rule, url)) {
			return false;
		}
		const nextCount = (ruleCounts.get(index) ?? 0) + 1;
		ruleCounts.set(index, nextCount);
		const afterCount = rule.afterCount ?? 0;
		const times = rule.times ?? Number.POSITIVE_INFINITY;
		return nextCount > afterCount && nextCount <= afterCount + times;
	}

	async function maybeDelay(url: URL) {
		const pathname = url.pathname;
		const nextCount = (requestCounts.get(pathname) ?? 0) + 1;
		requestCounts.set(pathname, nextCount);
		if (
			options.responseDelayMs &&
			options.responseDelayMs > 0 &&
			delayedPathSet.has(pathname) &&
			nextCount > 1
		) {
			await sleep(options.responseDelayMs);
		}

		for (const [index, rule] of (options.delayRules ?? []).entries()) {
			if (ruleApplies(rule, url, delayRuleCounts, index)) {
				await sleep(rule.delayMs);
			}
		}
	}

	function nextFailure(url: URL) {
		for (const [index, rule] of (options.failureRules ?? []).entries()) {
			if (ruleApplies(rule, url, failureRuleCounts, index)) {
				return {
					status: rule.status ?? 500,
					message: rule.message ?? `mock failure for ${url.pathname}`,
				};
			}
		}
		return null;
	}

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		await maybeDelay(url);

		const failure = nextFailure(url);
		if (failure) {
			return json(
				route,
				{
					error: {
						message: failure.message,
					},
				},
				failure.status,
			);
		}

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
			const taskGroup = url.searchParams.get("task_group") ?? "all";
			const filtered = tasks.filter((task) => {
				if (status !== "all" && task.status !== status) return false;
				if (taskType && task.task_type !== taskType) return false;
				if (excludeTaskType && task.task_type === excludeTaskType) return false;
				if (taskGroup === "scheduled") {
					return ["brief.daily_slot", "sync.subscriptions"].includes(
						task.task_type,
					);
				}
				if (taskGroup === "realtime") {
					return !["brief.daily_slot", "sync.subscriptions"].includes(
						task.task_type,
					);
				}
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
			if (!emitStreamEvents) {
				return route.fulfill({
					status: 200,
					contentType: "text/event-stream",
					body: "",
				});
			}

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
			pathname.startsWith("/api/admin/jobs/realtime/") &&
			pathname.endsWith("/log")
		) {
			return route.fulfill({
				status: 200,
				contentType: "application/x-ndjson",
				body: '{"line":1}\n{"line":2}\n',
			});
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/jobs/realtime/") &&
			!pathname.endsWith("/log")
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
			if (taskId === "task-translate-batch-1") {
				return json(route, {
					task: {
						...task,
						payload_json: JSON.stringify({
							user_id: 1,
							release_ids: [290978079, 290980132],
						}),
						result_json: JSON.stringify({
							total: 2,
							ready: 0,
							missing: 0,
							disabled: 0,
							error: 2,
						}),
					},
					event_meta: {
						returned: 2,
						total: 4,
						limit: 2,
						truncated: true,
					},
					diagnostics: {
						business_outcome: {
							code: "failed",
							label: "业务失败",
							message: "任务运行完成，但全部翻译项失败。",
						},
						translate_release_batch: {
							target_user_id: 1,
							release_total: 2,
							summary: {
								total: 2,
								ready: 0,
								missing: 0,
								disabled: 0,
								error: 2,
							},
							progress: {
								processed: 2,
								last_stage: "release",
							},
							items: [
								{
									release_id: "290978079",
									item_status: "error",
									item_error: "translation failed",
									last_event_at: "2026-02-26T01:10:30Z",
								},
								{
									release_id: "290980132",
									item_status: "error",
									item_error: "translation failed",
									last_event_at: "2026-02-26T01:10:32Z",
								},
							],
						},
					},
					events: [
						{
							id: 20,
							event_type: "task.progress",
							payload_json: JSON.stringify({
								stage: "release",
								release_id: "290978079",
								item_status: "error",
								item_error: "translation failed",
							}),
							created_at: "2026-02-26T01:10:30Z",
						},
						{
							id: 21,
							event_type: "task.completed",
							payload_json: JSON.stringify({
								status: "succeeded",
							}),
							created_at: "2026-02-26T01:10:40Z",
						},
					],
				});
			}

			if (taskId === "task-subscriptions-1") {
				return json(route, {
					task: {
						...task,
						payload_json: JSON.stringify({
							trigger: "schedule",
							schedule_key: "2026-02-26T14:30",
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
					event_meta: {
						returned: 4,
						total: 4,
						limit: 200,
						truncated: false,
					},
					diagnostics: {
						business_outcome: {
							code: "partial",
							label: "部分成功",
							message: "任务已完成，但存在失败或关键告警，请查看最近关键事件。",
						},
						sync_subscriptions: {
							trigger: "schedule",
							schedule_key: "2026-02-26T14:30",
							skipped: false,
							skip_reason: null,
							log_available: true,
							log_download_path:
								"/api/admin/jobs/realtime/task-subscriptions-1/log",
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
									id: 42,
									stage: "release",
									event_type: "repo_inaccessible",
									severity: "error",
									recoverable: false,
									attempt: 1,
									user_id: 23,
									repo_id: 9001,
									repo_full_name: "octo/private-repo",
									message:
										"release sync candidate failed for octo/private-repo with user #23",
									created_at: "2026-02-26T14:31:40Z",
								},
							],
						},
					},
					events: [
						{
							id: 31,
							event_type: "task.progress",
							payload_json: JSON.stringify({
								stage: "collect",
								total_users: 12,
							}),
							created_at: "2026-02-26T14:30:05Z",
						},
						{
							id: 32,
							event_type: "task.progress",
							payload_json: JSON.stringify({
								stage: "star_summary",
								total_users: 12,
								succeeded_users: 11,
								failed_users: 1,
							}),
							created_at: "2026-02-26T14:31:02Z",
						},
						{
							id: 33,
							event_type: "task.progress",
							payload_json: JSON.stringify({
								stage: "release_summary",
								total_repos: 128,
								succeeded_repos: 123,
								failed_repos: 5,
								releases_written: 1840,
							}),
							created_at: "2026-02-26T14:37:59Z",
						},
						{
							id: 34,
							event_type: "task.completed",
							payload_json: JSON.stringify({ status: "succeeded" }),
							created_at: "2026-02-26T14:38:10Z",
						},
					],
				});
			}

			return json(route, {
				task: {
					...task,
					payload_json: JSON.stringify({
						task_id: task.id,
						status: task.status,
					}),
					result_json:
						task.status === "failed" ? null : JSON.stringify({ ok: true }),
				},
				event_meta: {
					returned: 1,
					total: 1,
					limit: 200,
					truncated: false,
				},
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
			const parentTaskId = url.searchParams.get("parent_task_id") ?? "";
			const filtered = llmCalls.filter((item) => {
				if (status !== "all" && item.status !== status) return false;
				if (source && item.source !== source) return false;
				if (
					requestedBy &&
					String(item.requested_by ?? "") !== String(requestedBy)
				) {
					return false;
				}
				if (parentTaskId && item.parent_task_id !== parentTaskId) {
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
			if (callId === "llm-translation-1") {
				return json(route, {
					id: "llm-translation-1",
					status: "succeeded",
					source: "translation.scheduler.deadline",
					model: "gpt-4o-mini",
					requested_by: null,
					parent_task_id: null,
					parent_task_type: null,
					max_tokens: 900,
					attempt_count: 1,
					scheduler_wait_ms: 240,
					first_token_wait_ms: 120,
					duration_ms: 820,
					input_tokens: 420,
					output_tokens: 110,
					cached_input_tokens: 0,
					total_tokens: 530,
					input_messages_json: JSON.stringify([
						{ role: "user", content: "translate grouped items" },
					]),
					output_messages_json: JSON.stringify([
						{ role: "assistant", content: "- grouped result" },
					]),
					prompt_text: "translate grouped items",
					response_text: "- grouped result",
					error_text: null,
					created_at: "2026-02-26T04:00:01Z",
					started_at: "2026-02-26T04:00:01Z",
					finished_at: "2026-02-26T04:00:02Z",
					updated_at: "2026-02-26T04:00:02Z",
				});
			}
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

		if (
			req.method() === "GET" &&
			pathname === "/api/admin/jobs/translations/status"
		) {
			return json(route, {
				scheduler_enabled: true,
				llm_enabled: true,
				scan_interval_ms: 250,
				batch_token_threshold: 1800,
				queued_requests: 0,
				queued_work_items: 0,
				running_batches: 0,
				requests_24h: 1,
				completed_batches_24h: 1,
				failed_batches_24h: 0,
				avg_wait_ms_24h: 320,
				last_batch_finished_at: "2026-02-26T04:00:03Z",
			});
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/admin/jobs/translations/requests"
		) {
			return json(route, {
				items: translationRequests,
				page: 1,
				page_size: 20,
				total: translationRequests.length,
			});
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/jobs/translations/requests/")
		) {
			return json(route, translationRequestDetail);
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/admin/jobs/translations/batches"
		) {
			return json(route, {
				items: translationBatches,
				page: 1,
				page_size: 20,
				total: translationBatches.length,
			});
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/jobs/translations/batches/")
		) {
			return json(route, translationBatchDetail);
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
	test.slow();
	await installAdminJobsMocks(page);
	await page.goto("/admin/jobs");

	const realtimeTab = page.getByRole("tab", { name: "实时异步任务" });
	const scheduledTab = page.getByRole("tab", { name: "定时任务" });
	const llmTab = page.getByRole("tab", { name: "LLM调度" });

	await expect(page).toHaveURL(/\/admin\/jobs$/);
	await expect(page.getByRole("heading", { name: "管理后台" })).toBeVisible();
	await expect(page.getByRole("heading", { name: "任务总览" })).toBeVisible();
	await expect(realtimeTab).toHaveAttribute("aria-selected", "true");
	await expect(
		page.getByRole("combobox", { name: "实时异步任务状态筛选" }),
	).toBeVisible();

	const realtimeHelp = page.getByRole("button", { name: "实时异步任务说明" });
	await realtimeHelp.hover();
	await expect(page.getByRole("tooltip")).toContainText(
		"监控系统内部任务，并支持重试与取消。",
	);

	await expect(page.getByText("sync.releases")).toBeVisible();
	await expect(page.getByText("brief.daily_slot")).toHaveCount(0);
	await expect(page.getByText("sync.subscriptions")).toHaveCount(0);
	await page.getByRole("button", { name: "详情" }).first().click();
	const taskSheet = page.getByRole("dialog", { name: "任务详情" });
	await expect(taskSheet).toBeVisible();
	await expect(page).toHaveURL(/\/admin\/jobs\/tasks\/task-running-1$/);
	await page.getByRole("button", { name: "关闭", exact: true }).click();
	await expect(taskSheet).toHaveCount(0);
	await expect(page).toHaveURL(/\/admin\/jobs$/);

	const translateTaskCard = page
		.getByText("ID: task-translate-batch-1")
		.locator("xpath=ancestor::div[.//button[normalize-space()='详情']][1]");
	await translateTaskCard.getByRole("button", { name: "详情" }).click();
	await expect(taskSheet).toBeVisible();
	await expect(
		page.getByText("task-translate-batch-1", { exact: true }),
	).toBeVisible();
	await expect(page.getByText("业务结果：业务失败")).toBeVisible();
	await expect(
		page.getByText("仅展示最近 2 条事件（已加载 2/4）。"),
	).toBeVisible();
	await expect(
		page.getByText("Release #290978079 · error · translation failed"),
	).toBeVisible();
	const taskLlmDetailButton = page.getByRole("button", {
		name: "查看 LLM 详情",
	});
	await taskLlmDetailButton.scrollIntoViewIfNeeded();
	await taskLlmDetailButton.click();
	const taskLlmSheet = page.getByRole("dialog", {
		name: "任务详情 · LLM 调用详情",
	});
	await expect(taskLlmSheet).toBeVisible();
	await expect(page).toHaveURL(
		/\/admin\/jobs\/tasks\/task-translate-batch-1\/llm\/llm-call-2$/,
	);
	await expect(
		page.getByText("来源：api.translate_releases_batch"),
	).toBeVisible();
	await page.getByRole("button", { name: "返回任务详情" }).click();
	await expect(taskSheet).toBeVisible();
	await expect(page).toHaveURL(/\/admin\/jobs\/tasks\/task-translate-batch-1$/);
	await page.getByRole("button", { name: "关闭", exact: true }).click();
	await expect(taskSheet).toHaveCount(0);
	await expect(page).toHaveURL(/\/admin\/jobs$/);

	await scheduledTab.click();
	await expect(scheduledTab).toHaveAttribute("aria-selected", "true");
	await expect(
		page.getByRole("combobox", { name: "定时任务状态筛选" }),
	).toBeVisible();
	await expect(page.getByText("运行记录")).toBeVisible();
	await expect(page.getByText("定时日报")).toBeVisible();
	await expect(page.getByText("订阅同步")).toBeVisible();
	await expect(page.getByText("sync.subscriptions")).toBeVisible();
	await expect(
		page.getByText("brief.daily_slot", { exact: true }),
	).toBeVisible();
	const subscriptionTaskCard = page
		.getByText("ID: task-subscriptions-1")
		.locator("xpath=ancestor::div[.//button[normalize-space()='详情']][1]");
	await subscriptionTaskCard.getByRole("button", { name: "详情" }).click();
	await expect(
		page.getByText("task-subscriptions-1", { exact: true }),
	).toBeVisible();
	await expect(page.getByText("最近关键事件", { exact: true })).toBeVisible();
	await expect(page.getByRole("link", { name: "下载日志" })).toBeVisible();
	await page.getByRole("button", { name: "关闭", exact: true }).click();
	await expect(page).toHaveURL(/\/admin\/jobs$/);
	await expect(page.getByText("执行时间配置（24小时槽）")).toHaveCount(0);

	await llmTab.click();
	await expect(llmTab).toHaveAttribute("aria-selected", "true");
	await expect(page.getByRole("heading", { name: "LLM 调度" })).toBeVisible();
	await expect(
		page.getByRole("combobox", { name: "LLM 调用状态筛选" }),
	).toBeVisible();
	await expect(
		page.getByRole("textbox", { name: "LLM 调用来源筛选" }),
	).toBeVisible();
	await expect(page.getByLabel("LLM 开始时间下限")).toBeVisible();
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible();
	await page
		.getByRole("textbox", { name: "LLM 调用来源筛选" })
		.fill("job.api.translate_release");
	await expect(page.getByText("job.api.translate_release")).toBeVisible();
	await expect(page.getByText("api.translate_releases_batch")).toHaveCount(0);

	const llmCallCard = page
		.getByText("ID: llm-call-1")
		.locator("xpath=ancestor::div[.//button[normalize-space()='详情']][1]");
	await llmCallCard.getByRole("button", { name: "详情" }).click();
	const llmSheet = page.getByRole("dialog", { name: "LLM 调用详情" });
	await expect(llmSheet).toBeVisible();
	await expect(page.getByText("Conversation Timeline")).toBeVisible();
	await expect(page.getByText("Input Messages")).toHaveCount(0);
	await expect(page.getByText("耗时 / 重试")).toBeVisible();
	await expect(page.getByText("等待 / 首字 / 耗时 / 重试")).toHaveCount(0);
	await expect(
		page.getByText("等待 1.20s · 首字 860ms", { exact: true }),
	).toBeVisible();
	await expect(page.getByText("Token（输入 / 输出 / 缓存）")).toBeVisible();
	await page.getByRole("button", { name: "关闭", exact: true }).click();
	await expect(llmSheet).toHaveCount(0);
});

test("admin keeps llm calls visible during sse refresh", async ({ page }) => {
	test.slow();
	await installAdminJobsMocks(page, {
		responseDelayMs: 1200,
		delayedPaths: ["/api/admin/jobs/llm/calls"],
		emitStreamEvents: true,
	});
	await page.goto("/admin/jobs");

	await page.getByRole("tab", { name: "LLM调度" }).click();
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible();
	await expect(page.getByText("LLM 调度更新中...")).toBeVisible();
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible();
	await expect(page.getByText("正在加载调用记录...")).toHaveCount(0);
});

test("admin keeps newest llm filter results after overlapping refreshes", async ({
	page,
}) => {
	test.slow();
	await installAdminJobsMocks(page, {
		delayRules: [
			{
				pathname: "/api/admin/jobs/llm/calls",
				search: "status=all",
				afterCount: 2,
				times: 2,
				delayMs: 1200,
			},
			{
				pathname: "/api/admin/jobs/llm/calls",
				search: "status=failed",
				times: 1,
				delayMs: 300,
			},
		],
		emitStreamEvents: false,
	});
	await page.goto("/admin/jobs");

	await page.getByRole("tab", { name: "LLM调度" }).click();
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible();
	const refreshButton = page.getByRole("button", { name: "刷新" });
	await refreshButton.click();
	await page.getByRole("combobox", { name: "LLM 调用状态筛选" }).click();
	await page.getByRole("option", { name: "状态：失败" }).click();

	await expect(page.getByText("正在加载调用记录...")).toBeVisible();
	await expect(page.getByText("api.translate_releases_batch")).toHaveCount(0);
	await expect(page.getByText("job.api.translate_release")).toBeVisible();
	await expect(refreshButton).toBeEnabled();

	await page.waitForTimeout(1300);
	await expect(page.getByText("job.api.translate_release")).toBeVisible();
	await expect(page.getByText("api.translate_releases_batch")).toHaveCount(0);
});

test("admin keeps blocking loader before first realtime load completes", async ({
	page,
}) => {
	test.slow();
	await installAdminJobsMocks(page, {
		delayRules: [
			{
				pathname: "/api/admin/jobs/realtime",
				search: "task_group=realtime",
				times: 1,
				delayMs: 1200,
			},
		],
		failureRules: [
			{
				pathname: "/api/admin/jobs/realtime",
				search: "task_group=realtime",
				afterCount: 1,
				times: 1,
				message: "ignored background refresh failure",
			},
		],
		emitStreamEvents: true,
	});
	await page.goto("/admin/jobs", { waitUntil: "domcontentloaded" });

	await expect(page.getByText("正在加载任务...")).toBeVisible();
	await expect(page.getByText(/^SSE (已连接|重连中\.\.\.)$/)).toBeVisible();
	await expect(page.getByText("任务列表更新中...")).toHaveCount(0);
	await expect(page.getByText("暂无任务。")).toHaveCount(0);
	await expect(page.getByText("sync.releases")).toBeVisible();
	await expect(
		page.getByText("ignored background refresh failure"),
	).toHaveCount(0);
});

test("admin ignores stale llm refresh errors after filter change", async ({
	page,
}) => {
	test.slow();
	await installAdminJobsMocks(page, {
		delayRules: [
			{
				pathname: "/api/admin/jobs/llm/calls",
				search: "status=all",
				afterCount: 1,
				times: 1,
				delayMs: 600,
			},
			{
				pathname: "/api/admin/jobs/llm/calls",
				search: "status=failed",
				times: 1,
				delayMs: 300,
			},
		],
		failureRules: [
			{
				pathname: "/api/admin/jobs/llm/calls",
				search: "status=all",
				afterCount: 1,
				times: 1,
				message: "stale llm refresh failed",
			},
		],
		emitStreamEvents: false,
	});
	await page.goto("/admin/jobs");

	await page.getByRole("tab", { name: "LLM调度" }).click();
	const refreshButton = page.getByRole("button", { name: "刷新" });
	await refreshButton.click();
	await page.getByRole("combobox", { name: "LLM 调用状态筛选" }).click();
	await page.getByRole("option", { name: "状态：失败" }).click();

	await expect(page.getByText("正在加载调用记录...")).toBeVisible();
	await expect(page.getByText("job.api.translate_release")).toBeVisible();
	await expect(page.getByText("stale llm refresh failed")).toHaveCount(0);

	await page.waitForTimeout(700);
	await expect(page.getByText("job.api.translate_release")).toBeVisible();
	await expect(page.getByText("stale llm refresh failed")).toHaveCount(0);
});

test("admin refresh keeps scheduled runs visible", async ({ page }) => {
	test.slow();
	await installAdminJobsMocks(page, {
		responseDelayMs: 1200,
		delayedPaths: ["/api/admin/jobs/realtime"],
		emitStreamEvents: false,
	});
	await page.goto("/admin/jobs");

	await page.getByRole("tab", { name: "定时任务" }).click();
	await expect(page.getByText("定时日报")).toBeVisible();
	const refreshButton = page.getByRole("button", { name: "刷新" });
	await refreshButton.click();

	await expect(refreshButton).toBeDisabled();
	await expect(page.getByText("定时日报")).toBeVisible();
	await expect(page.getByText("运行记录更新中...")).toBeVisible();
	await expect(page.getByText("正在加载运行记录...")).toHaveCount(0);
	await expect(refreshButton).toBeEnabled();
	await expect(page.getByText("运行记录更新中...")).toHaveCount(0);
});

test("admin refresh keeps existing jobs and llm calls visible", async ({
	page,
}) => {
	test.slow();
	await installAdminJobsMocks(page, {
		responseDelayMs: 1200,
		delayedPaths: ["/api/admin/jobs/realtime", "/api/admin/jobs/llm/calls"],
		emitStreamEvents: false,
	});
	await page.goto("/admin/jobs");

	await expect(page.getByText("sync.releases")).toBeVisible();
	const refreshButton = page.getByRole("button", { name: "刷新" });
	await refreshButton.click();

	await expect(refreshButton).toBeDisabled();
	await expect(page.getByText("sync.releases")).toBeVisible();
	await expect(page.getByText("任务列表更新中...")).toBeVisible();
	await expect(page.getByText("正在加载任务...")).toHaveCount(0);

	await page.getByRole("tab", { name: "LLM调度" }).click();
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible();
	await expect(page.getByText("LLM 调度更新中...")).toBeVisible();
	await expect(page.getByText("正在加载调用记录...")).toHaveCount(0);

	await expect(refreshButton).toBeEnabled();
	await expect(page.getByText("任务列表更新中...")).toHaveCount(0);
	await expect(page.getByText("LLM 调度更新中...")).toHaveCount(0);
});

test("admin can inspect translation scheduler", async ({ page }) => {
	await installAdminJobsMocks(page);

	await page.goto("/admin/jobs");
	await page.getByRole("tab", { name: "翻译调度" }).click();
	await expect(page.getByRole("heading", { name: "翻译调度" })).toBeVisible();
	await expect(page.getByText("feed.auto_translate")).toBeVisible();
	await page.getByRole("button", { name: "详情" }).first().click();
	await expect(
		page.getByRole("heading", { name: "翻译请求详情" }),
	).toBeVisible();
	await expect(page.getByText("release_summary · feed_card")).toBeVisible();
	await page.getByRole("button", { name: "查看批次" }).click();
	await expect(
		page.getByRole("heading", { name: "翻译批次详情" }),
	).toBeVisible();
	await expect(page.getByText("translation.scheduler.deadline")).toBeVisible();
	await page.getByRole("button", { name: "打开 LLM 详情" }).click();
	const llmDialog = page.getByRole("dialog", { name: "LLM 调用详情" });
	await expect(
		llmDialog.getByRole("heading", { name: "LLM 调用详情" }),
	).toBeVisible();
	await expect(llmDialog.getByText("llm-translation-1")).toBeVisible();
});
