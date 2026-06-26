import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import {
	type Locator,
	type Page,
	type Route,
	expect,
	test,
} from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

const CURRENT_USER_ID = "2f4k7m9p3x6c8v2a";
const ACCESS_TASK_ID = "task-access-click";
const ACCESS_SYNC_EVIDENCE_DIR = process.env.ACCESS_SYNC_EVIDENCE_DIR;

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

async function installEventSourceMock(page: Page) {
	await page.addInitScript(
		({ accessTaskId }) => {
			class MockEventSource {
				url: string;
				readyState = 1;
				withCredentials = false;
				onopen: ((this: EventSource, event: Event) => unknown) | null = null;
				onmessage:
					| ((this: EventSource, event: MessageEvent<string>) => unknown)
					| null = null;
				onerror: ((this: EventSource, event: Event) => unknown) | null = null;
				private listeners = new Map<
					string,
					Set<(event: Event | MessageEvent<string>) => unknown>
				>();
				private timers: number[] = [];

				constructor(url: string | URL) {
					this.url = String(url);
					this.timers.push(
						window.setTimeout(() => {
							this.onopen?.call(
								this as unknown as EventSource,
								new Event("open"),
							);
						}, 0),
					);

					if (this.url.endsWith(`/api/tasks/${accessTaskId}/events`)) {
						this.timers.push(
							window.setTimeout(() => {
								this.dispatch("task.running", {
									task_id: accessTaskId,
									status: "running",
								});
							}, 60),
						);
						this.timers.push(
							window.setTimeout(() => {
								this.dispatch("task.progress", {
									task_id: accessTaskId,
									stage: "star_failed",
									operation: "sync starred graphql",
									error_kind: "timeout",
								});
							}, 120),
						);
					}
				}

				addEventListener(
					type: string,
					listener: (event: Event | MessageEvent<string>) => unknown,
				) {
					const current = this.listeners.get(type) ?? new Set();
					current.add(listener);
					this.listeners.set(type, current);
				}

				removeEventListener(
					type: string,
					listener: (event: Event | MessageEvent<string>) => unknown,
				) {
					this.listeners.get(type)?.delete(listener);
				}

				close() {
					this.readyState = 2;
					for (const timer of this.timers) {
						window.clearTimeout(timer);
					}
					this.timers = [];
				}

				private dispatch(type: string, payload: unknown) {
					if (this.readyState === 2) return;
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
		},
		{ accessTaskId: ACCESS_TASK_ID },
	);
}

async function captureRawEvidence(target: Page | Locator, filename: string) {
	if (!ACCESS_SYNC_EVIDENCE_DIR) return;
	mkdirSync(ACCESS_SYNC_EVIDENCE_DIR, { recursive: true });
	await target.screenshot({
		path: resolve(ACCESS_SYNC_EVIDENCE_DIR, filename),
		animations: "disabled",
	});
}

test("dashboard sync click creates the same access-refresh task visible in admin jobs", async ({
	page,
}) => {
	let accessTaskVisible = false;
	let syncRequestAccepted = false;

	const accessRefreshTask = {
		id: ACCESS_TASK_ID,
		task_type: "sync.access_refresh",
		status: "failed",
		source: "api.sync_all",
		requested_by: CURRENT_USER_ID,
		parent_task_id: null,
		cancel_requested: false,
		error_message: "sync starred graphql: timed out after 30s",
		created_at: "2026-06-26T02:30:00Z",
		started_at: "2026-06-26T02:30:02Z",
		finished_at: "2026-06-26T02:30:32Z",
		updated_at: "2026-06-26T02:30:32Z",
	};

	await installEventSourceMock(page);
	await page.setViewportSize({ width: 1440, height: 1200 });

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname, searchParams } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: CURRENT_USER_ID,
					github_user_id: 10,
					login: "octo-admin",
					name: "Octo Admin",
					avatar_url: null,
					email: "admin@example.com",
					is_admin: true,
				}),
			);
		}

		if (req.method() === "POST" && pathname === "/api/sync/all") {
			syncRequestAccepted = true;
			accessTaskVisible = true;
			expect(searchParams.get("return_mode")).toBe("task_id");
			return json(route, { task_id: ACCESS_TASK_ID });
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			return json(route, { items: [], next_cursor: null });
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/reaction-token/status") {
			return json(route, {
				configured: false,
				masked_token: null,
				check: {
					state: "idle",
					message: null,
					checked_at: null,
				},
			});
		}

		if (req.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "1.2.3" });
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/overview") {
			return json(route, {
				queued: 0,
				running: 0,
				failed_24h: accessTaskVisible ? 1 : 0,
				succeeded_24h: 0,
				enabled_scheduled_slots: 12,
				total_scheduled_slots: 24,
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/realtime") {
			if (searchParams.get("task_group") === "scheduled") {
				return json(route, {
					items: [],
					page: 1,
					page_size: 20,
					total: 0,
				});
			}

			if (searchParams.get("task_type") === "sync.subscriptions") {
				return json(route, {
					items: [],
					page: 1,
					page_size: 20,
					total: 0,
				});
			}

			const items = accessTaskVisible ? [accessRefreshTask] : [];
			return json(route, {
				items,
				page: 1,
				page_size: 20,
				total: items.length,
			});
		}

		if (
			req.method() === "GET" &&
			pathname === `/api/admin/jobs/realtime/${ACCESS_TASK_ID}`
		) {
			return json(route, {
				task: {
					...accessRefreshTask,
					payload_json: JSON.stringify({
						user_id: CURRENT_USER_ID,
					}),
					result_json: JSON.stringify({
						starred: { repos: 5 },
						release: { repos: 5, releases: 8 },
						social: { repo_stars: 3, followers: 2, events: 1 },
						notifications: { notifications: 7 },
					}),
				},
				event_meta: {
					returned: 3,
					total: 3,
					limit: 200,
					truncated: false,
				},
				diagnostics: {
					business_outcome: {
						code: "failed",
						label: "Star 阶段失败",
						message:
							"Star 阶段在 timeout 后失败，任务未进入 release/social/notifications 完整收敛。",
					},
					sync_access_refresh: {
						log_available: true,
						log_download_path: `/api/admin/jobs/realtime/${ACCESS_TASK_ID}/log`,
						star_repos: 5,
						release_repos: 5,
						releases: 8,
						social_repo_stars: 3,
						social_followers: 2,
						social_events: 1,
						notifications: 7,
						social_error: null,
						notifications_error: null,
						failure: {
							operation: "sync starred graphql",
							error_kind: "timeout",
							error_stage: "timeout",
							retryable: true,
							http_status: 200,
							timeout_ms: 30000,
							elapsed_ms: 30004,
							attempts: 3,
							retry_limit: 3,
							error_chain: "sync starred graphql: timed out after 30s",
						},
					},
				},
				events: [
					{
						id: "evt-access-1",
						event_type: "task.running",
						payload_json: JSON.stringify({
							status: "running",
						}),
						created_at: "2026-06-26T02:30:02Z",
					},
					{
						id: "evt-access-2",
						event_type: "task.progress",
						payload_json: JSON.stringify({
							stage: "star_failed",
							operation: "sync starred graphql",
							error_kind: "timeout",
							error_stage: "timeout",
						}),
						created_at: "2026-06-26T02:30:32Z",
					},
					{
						id: "evt-access-3",
						event_type: "task.completed",
						payload_json: JSON.stringify({
							status: "failed",
						}),
						created_at: "2026-06-26T02:30:32Z",
					},
				],
			});
		}

		if (
			req.method() === "GET" &&
			pathname === `/api/admin/jobs/realtime/${ACCESS_TASK_ID}/log`
		) {
			return route.fulfill({
				status: 200,
				contentType: "application/x-ndjson",
				body: '{"line":1}\n{"line":2}\n',
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/llm/status") {
			return json(route, {
				scheduler_enabled: true,
				max_concurrency: 2,
				ai_model_context_limit: null,
				effective_model_input_limit: 32768,
				effective_model_input_limit_source: "builtin_catalog",
				available_slots: 2,
				waiting_calls: 0,
				in_flight_calls: 0,
				calls_24h: 0,
				failed_24h: 0,
				avg_wait_ms_24h: null,
				avg_duration_ms_24h: null,
				last_success_at: null,
				last_failure_at: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/llm/calls") {
			return json(route, {
				items: [],
				page: 1,
				page_size: 20,
				total: 0,
			});
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/admin/jobs/sync/runtime-config"
		) {
			return json(route, {
				sync_auto_fetch_interval_minutes: 10,
				retry_recent_failures_interval_minutes: 10,
				repo_release_worker_concurrency: 5,
				recent_sync_tasks: [],
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
				ai_model_context_limit: null,
				effective_model_input_limit: 32768,
				effective_model_input_limit_source: "builtin_catalog",
				general_worker_concurrency: 1,
				dedicated_worker_concurrency: 1,
				worker_concurrency: 2,
				target_general_worker_concurrency: 1,
				target_dedicated_worker_concurrency: 1,
				target_worker_concurrency: 2,
				idle_workers: 2,
				busy_workers: 0,
				workers: [],
				queued_requests: 0,
				queued_work_items: 0,
				running_batches: 0,
				requests_24h: 0,
				completed_batches_24h: 0,
				clean_completed_batches_24h: 0,
				completed_with_issues_batches_24h: 0,
				failed_batches_24h: 0,
				error_work_items_24h: 0,
				missing_work_items_24h: 0,
				avg_wait_ms_24h: null,
				last_batch_finished_at: null,
			});
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/admin/jobs/translations/requests"
		) {
			return json(route, {
				items: [],
				page: 1,
				page_size: 20,
				total: 0,
			});
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/admin/jobs/translations/batches"
		) {
			return json(route, {
				items: [],
				page: 1,
				page_size: 20,
				total: 0,
			});
		}

		return json(
			route,
			{
				error: {
					code: "not_found",
					message: `unhandled ${req.method()} ${pathname}`,
				},
			},
			404,
		);
	});

	await page.goto("/admin/jobs", { waitUntil: "domcontentloaded" });
	await expect(page.getByRole("tab", { name: "实时异步任务" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect(page.getByText("暂无任务。")).toBeVisible();

	await page.goto("/");
	const syncButton = page.getByRole("banner").getByRole("button", {
		name: "同步",
	});
	await expect(syncButton).toBeEnabled();
	await syncButton.click();

	const tooltip = page.locator('[data-slot="tooltip-content"]').first();
	await expect(tooltip).toBeVisible();
	await expect(tooltip).toContainText("后台任务已启动");
	await captureRawEvidence(page, "access-sync-dashboard-started-raw.png");
	expect(syncRequestAccepted).toBe(true);

	await page.goto("/admin/jobs", { waitUntil: "domcontentloaded" });

	const accessTaskCard = page
		.getByText(`ID: ${ACCESS_TASK_ID}`)
		.locator("xpath=ancestor::div[.//button[normalize-space()='详情']][1]");
	await expect(accessTaskCard.getByText("访问增量同步")).toBeVisible();
	await expect(accessTaskCard.getByText("sync.access_refresh")).toBeVisible();
	await expect(
		accessTaskCard.getByText(
			"失败原因：sync starred graphql: timed out after 30s",
		),
	).toBeVisible();
	await captureRawEvidence(
		accessTaskCard,
		"access-sync-admin-task-card-raw.png",
	);

	await accessTaskCard.getByRole("button", { name: "详情" }).click();

	const taskSheet = page.getByRole("dialog", { name: "任务详情" });
	await expect(taskSheet).toBeVisible();
	await expect(page).toHaveURL(
		/\/admin\/jobs\/tasks\/task-access-click\?from=realtime$/,
	);
	await expect(page.getByText("Star 失败定类")).toBeVisible();
	await expect(
		page
			.getByText("错误链")
			.locator("xpath=ancestor::div[contains(@class,'rounded-lg')][1]")
			.getByText("sync starred graphql: timed out after 30s", { exact: true }),
	).toBeVisible();
	await expect(
		page.getByText("timeout 30000ms · elapsed 30004ms"),
	).toBeVisible();
	await expect(page.getByRole("link", { name: "下载日志" })).toBeVisible();
	await captureRawEvidence(taskSheet, "access-sync-admin-task-detail-raw.png");
});
