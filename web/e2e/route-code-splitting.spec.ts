import { expect, test, type Page, type Route } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

async function installAppMocks(
	page: Page,
	options: {
		meStatus?: 200 | 401;
		isAdmin?: boolean;
	},
) {
	const meStatus = options.meStatus ?? 200;
	const isAdmin = options.isAdmin ?? false;

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			if (meStatus === 401) {
				return json(
					route,
					{
						error: {
							code: "unauthorized",
							message: "unauthorized",
						},
					},
					401,
				);
			}

			return json(
				route,
				buildMockMeResponse({
					id: "2f4k7m9p3x6c8v2a",
					github_user_id: 10,
					login: isAdmin ? "octo-admin" : "octo-user",
					name: isAdmin ? "Octo Admin" : "Octo User",
					avatar_url: null,
					email: isAdmin ? "admin@example.com" : "user@example.com",
					is_admin: isAdmin,
				}),
			);
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

		if (req.method() === "GET" && pathname === "/api/me/profile") {
			return json(route, {
				user_id: "2f4k7m9p3x6c8v2a",
				daily_brief_local_time: "08:00",
				daily_brief_time_zone: "Asia/Shanghai",
				last_active_at: "2026-04-20T08:00:00+08:00",
				include_own_releases: false,
			});
		}

		if (req.method() === "GET" && pathname === "/api/me/linuxdo") {
			return json(route, {
				available: true,
				connection: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "2.4.6" });
		}

		if (req.method() === "GET" && pathname === "/api/admin/users") {
			return json(route, {
				items: [],
				page: 1,
				page_size: 20,
				total: 0,
				guard: {
					admin_total: 1,
					active_admin_total: 1,
				},
			});
		}

		if (req.method() === "GET" && pathname.startsWith("/api/admin/jobs")) {
			if (pathname === "/api/admin/jobs/overview") {
				return json(route, {
					status_counts: [],
					task_type_counts: [],
					recent_events: [],
				});
			}
			if (pathname === "/api/admin/jobs/tasks") {
				return json(route, {
					items: [],
					next_cursor: null,
				});
			}
			if (pathname === "/api/admin/jobs/llm/calls") {
				return json(route, {
					items: [],
					next_cursor: null,
				});
			}
			if (pathname === "/api/admin/jobs/llm/status") {
				return json(route, {
					scheduler_enabled: true,
					max_concurrency: 1,
					ai_model_context_limit: null,
					effective_model_input_limit: 32768,
					effective_model_input_limit_source: "builtin_catalog",
					available_slots: 1,
					waiting_calls: 0,
					in_flight_calls: 0,
					calls_24h: 0,
					failed_24h: 0,
					avg_wait_ms_24h: 0,
					avg_duration_ms_24h: 0,
					last_success_at: null,
					last_failure_at: null,
				});
			}
			if (pathname === "/api/admin/jobs/translations/status") {
				return json(route, {
					scheduler_enabled: true,
					deadline_scheduler_enabled: true,
					realtime_worker_concurrency: 1,
					deadline_worker_concurrency: 1,
					queued_requests: 0,
					active_batches: 0,
					last_batch_started_at: null,
					last_batch_finished_at: null,
				});
			}
			if (pathname === "/api/admin/jobs/translations/requests") {
				return json(route, {
					items: [],
					next_cursor: null,
				});
			}
			if (pathname === "/api/admin/jobs/translations/batches") {
				return json(route, {
					items: [],
					next_cursor: null,
				});
			}
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
}

function trackModuleRequests(page: Page) {
	const requests: string[] = [];

	page.on("request", (request) => {
		const url = request.url();
		if (!url.includes("/src/")) return;
		requests.push(url);
	});

	return {
		requests,
		includes(fragment: string) {
			return requests.some((value) => value.includes(fragment));
		},
	};
}

test("anonymous root only loads landing route code path on demand", async ({
	page,
}) => {
	await installAppMocks(page, { meStatus: 401 });
	const tracked = trackModuleRequests(page);

	await page.goto("/");

	await expect(
		page.getByRole("link", { name: "使用 GitHub 登录" }),
	).toBeVisible();
	expect(tracked.includes("/src/routes/index.lazy.tsx")).toBeTruthy();
	expect(
		tracked.includes("/src/routes/-index.landing-surface.tsx"),
	).toBeTruthy();
	expect(
		tracked.includes("/src/routes/-index.dashboard-surface.tsx"),
	).toBeFalsy();
	expect(tracked.includes("/src/routes/settings.lazy.tsx")).toBeFalsy();
	expect(tracked.includes("/src/routes/admin/index.lazy.tsx")).toBeFalsy();
	expect(tracked.includes("/src/routes/admin/jobs/index.lazy.tsx")).toBeFalsy();
});

test("authenticated dashboard root does not preload settings or admin routes", async ({
	page,
}) => {
	await installAppMocks(page, { meStatus: 200, isAdmin: true });
	const tracked = trackModuleRequests(page);

	await page.goto("/");

	await expect(
		page.getByRole("heading", { level: 1, name: "OctoRill" }),
	).toBeVisible();
	expect(tracked.includes("/src/routes/index.lazy.tsx")).toBeTruthy();
	expect(
		tracked.includes("/src/routes/-index.dashboard-surface.tsx"),
	).toBeTruthy();
	expect(tracked.includes("/src/routes/settings.lazy.tsx")).toBeFalsy();
	expect(tracked.includes("/src/routes/admin/index.lazy.tsx")).toBeFalsy();
	expect(tracked.includes("/src/routes/admin/jobs/index.lazy.tsx")).toBeFalsy();
});

test("settings and admin jobs lazy chunks are fetched only when their routes are opened", async ({
	page,
}) => {
	await installAppMocks(page, { meStatus: 200, isAdmin: true });
	const tracked = trackModuleRequests(page);

	await page.goto("/settings?section=github-pat");

	await expect(page.getByRole("heading", { name: "账号与偏好" })).toBeVisible();
	expect(tracked.includes("/src/routes/settings.lazy.tsx")).toBeTruthy();
	expect(
		tracked.includes("/src/routes/admin/jobs/translations.lazy.tsx"),
	).toBeFalsy();

	await page.goto("/admin/jobs/translations?view=history");

	await expect(page.getByRole("tab", { name: "翻译调度" })).toBeVisible();
	expect(
		tracked.includes("/src/routes/admin/jobs/translations.lazy.tsx"),
	).toBeTruthy();
	expect(tracked.includes("/src/routes/admin/jobs/-helpers.tsx")).toBeTruthy();
});
