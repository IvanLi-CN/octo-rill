import { expect, test, type Page, type Route } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

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

async function seedWarmDashboardCache(page: Page) {
	await page.addInitScript(() => {
		window.localStorage.setItem(
			"octo-rill.auth-bootstrap.v3",
			JSON.stringify({
				savedAt: Date.now(),
				me: {
					user: {
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo-admin",
						name: "Octo Admin",
						avatar_url: null,
						email: "admin@example.com",
						is_admin: true,
					},
					dashboard: {
						daily_boundary_local: "08:00",
						daily_boundary_time_zone: "Asia/Shanghai",
						daily_boundary_utc_offset_minutes: 480,
					},
					access_sync: {
						task_id: null,
						task_type: null,
						event_path: null,
						reason: "none",
					},
				},
			}),
		);
		window.localStorage.setItem(
			"octo-rill.dashboard-warm.v1",
			JSON.stringify({
				savedAt: Date.now(),
				userId: "2f4k7m9p3x6c8v2a",
				routeState: { tab: "all", activeReleaseId: null },
				feedRequestType: "all",
				feedItems: [
					{
						kind: "release",
						ts: "2026-04-15T08:00:00Z",
						id: "cached-10001",
						repo_full_name: "owner/repo",
						title: "Cached Release 10001",
						body: "cached body",
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/owner/repo/releases/tag/v10001",
						unread: null,
						translated: null,
						smart: null,
						reactions: null,
					},
				],
				nextCursor: null,
				notifications: [
					{
						thread_id: "thread-cached-1",
						repo_full_name: "owner/repo",
						subject_title: "Cached inbox thread",
						subject_type: "Issue",
						reason: "subscribed",
						updated_at: "2026-04-15T08:02:00Z",
						unread: 1,
						html_url: "https://github.com/notifications/threads/1",
					},
				],
				briefs: [
					{
						id: "brief-cached-1",
						date: "2026-04-15",
						created_at: "2026-04-15T08:05:00Z",
						release_count: 3,
					},
				],
				selectedBriefId: "brief-cached-1",
			}),
		);
	});
}

async function seedPersistentAuthCache(page: Page) {
	await page.addInitScript(() => {
		window.localStorage.setItem(
			"octo-rill.auth-bootstrap.v3",
			JSON.stringify({
				savedAt: Date.now() - 2 * 60 * 60 * 1000,
				me: {
					user: {
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo-admin",
						name: "Octo Admin",
						avatar_url: null,
						email: "admin@example.com",
						is_admin: true,
					},
					dashboard: {
						daily_boundary_local: "08:00",
						daily_boundary_time_zone: "Asia/Shanghai",
						daily_boundary_utc_offset_minutes: 480,
					},
					access_sync: {
						task_id: null,
						task_type: null,
						event_path: null,
						reason: "none",
					},
				},
			}),
		);
		window.localStorage.setItem(
			"octo-rill.dashboard-warm.v1",
			JSON.stringify({
				savedAt: Date.now(),
				userId: "2f4k7m9p3x6c8v2a",
				routeState: { tab: "all", activeReleaseId: null },
				feedRequestType: "all",
				feedItems: [],
				nextCursor: null,
				notifications: [],
				briefs: [],
				selectedBriefId: null,
			}),
		);
	});
}

async function seedPersistentAdminWarmCaches(page: Page) {
	await seedPersistentAuthCache(page);
	await page.addInitScript(() => {
		window.localStorage.setItem(
			"octo-rill.admin-users-warm.v1",
			JSON.stringify({
				savedAt: Date.now(),
				userId: "2f4k7m9p3x6c8v2a",
				queryInput: "",
				query: "",
				role: "all",
				status: "all",
				page: 1,
				items: [
					{
						id: "cached-admin-user",
						github_user_id: 99,
						login: "cached-admin",
						name: "Cached Admin",
						avatar_url: null,
						email: "cached-admin@example.com",
						is_admin: true,
						is_disabled: false,
						last_active_at: "2026-04-15T08:00:00Z",
						created_at: "2026-04-15T08:00:00Z",
						updated_at: "2026-04-15T08:00:00Z",
					},
				],
				total: 1,
				guardSummary: {
					admin_total: 1,
					active_admin_total: 1,
				},
			}),
		);
	});
}

type AppAuthMockOptions = {
	meStatus: 200 | 401 | 500;
	meDelayMs?: number;
	isAdmin?: boolean;
	bootMessage?: string;
	logoutToAnonymous?: boolean;
};

async function installAppAuthMocks(page: Page, options: AppAuthMockOptions) {
	let meCalls = 0;
	const isAdmin = options.isAdmin ?? false;
	let loggedOut = false;

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			meCalls += 1;
			if (options.meDelayMs) {
				await sleep(options.meDelayMs);
			}

			if (options.logoutToAnonymous && loggedOut) {
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

			if (options.meStatus === 200) {
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

			return json(
				route,
				{
					error: {
						code: options.meStatus === 401 ? "unauthorized" : "boot_failed",
						message:
							options.bootMessage ??
							(options.meStatus === 401 ? "unauthorized" : "boot failed"),
					},
				},
				options.meStatus,
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

		if (req.method() === "GET" && pathname === "/api/version") {
			return json(route, {
				ok: true,
				version: "2.4.6",
				source: "APP_EFFECTIVE_VERSION",
			});
		}

		if (req.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "2.4.6" });
		}

		if (req.method() === "GET" && pathname === "/api/admin/users") {
			return json(route, {
				items: [
					{
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo-admin",
						name: "Octo Admin",
						avatar_url: null,
						email: "admin@example.com",
						is_admin: true,
						is_disabled: false,
						last_active_at: "2026-04-15T08:00:00Z",
						created_at: "2026-04-15T08:00:00Z",
						updated_at: "2026-04-15T08:00:00Z",
					},
				],
				page: 1,
				page_size: 20,
				total: 1,
				guard: {
					admin_total: 1,
					active_admin_total: 1,
				},
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

	await page.route("**/auth/logout", async (route) => {
		loggedOut = true;
		await route.fulfill({
			status: 302,
			headers: {
				location: "/",
			},
			body: "",
		});
	});

	return {
		getMeCalls: () => meCalls,
	};
}

test("authenticated bootstrap keeps landing CTA hidden until /api/me resolves", async ({
	page,
}) => {
	await installAppAuthMocks(page, {
		meStatus: 200,
		meDelayMs: 900,
		isAdmin: true,
	});

	await page.goto("/", { waitUntil: "domcontentloaded" });

	await expect(page.locator("[data-app-boot]")).toBeVisible();
	await expect(page.getByRole("link", { name: "连接到 GitHub" })).toHaveCount(
		0,
	);
	await expect(page.locator("[data-landing-login-card]")).toHaveCount(0);
	await expect(
		page.getByText("应用正在完成初始化，请稍候片刻。"),
	).toBeVisible();

	await expect(
		page.getByRole("heading", { level: 1, name: "OctoRill" }),
	).toBeVisible();
	await expect(page.locator("[data-app-boot]")).toHaveCount(0);
	await expect(page.getByRole("link", { name: "连接到 GitHub" })).toHaveCount(
		0,
	);
});

test("anonymous bootstrap shows boot surface first and only reveals landing after 401", async ({
	page,
}) => {
	await installAppAuthMocks(page, {
		meStatus: 401,
		meDelayMs: 900,
	});

	await page.goto("/");

	await expect(page.locator("[data-app-boot]")).toBeVisible();
	await expect(page.getByRole("link", { name: "连接到 GitHub" })).toHaveCount(
		0,
	);
	await expect(page.locator("[data-landing-login-card]")).toHaveCount(0);

	await expect(page.locator("[data-app-boot]")).toHaveCount(0);
	await expect(page.locator("[data-landing-login-card]")).toBeVisible();
	await expect(page.getByRole("link", { name: "连接到 GitHub" })).toBeVisible();
});

test("stale authenticated startup cache is cleared after /api/me returns 401", async ({
	page,
}) => {
	await seedPersistentAuthCache(page);
	await installAppAuthMocks(page, {
		meStatus: 401,
		meDelayMs: 300,
	});

	await page.goto("/");

	await expect(page.locator("[data-landing-login-card]")).toBeVisible();
	await expect(page.getByRole("link", { name: "连接到 GitHub" })).toBeVisible();
	await expect
		.poll(async () =>
			page.evaluate(() =>
				window.localStorage.getItem("octo-rill.auth-bootstrap.v3"),
			),
		)
		.toBeNull();
	await expect
		.poll(async () =>
			page.evaluate(() =>
				window.localStorage.getItem("octo-rill.dashboard-warm.v1"),
			),
		)
		.toBeNull();
});

test("30-day startup seed does not survive transient /api/me failures as logged-in state", async ({
	page,
}) => {
	await seedPersistentAuthCache(page);
	await installAppAuthMocks(page, {
		meStatus: 500,
		meDelayMs: 300,
		bootMessage: "boot exploded",
	});

	await page.goto("/");

	await expect(page.locator("[data-landing-login-card]")).toBeVisible();
	await expect(page.getByRole("link", { name: "连接到 GitHub" })).toBeVisible();
	await expect(page.getByText("boot exploded")).toBeVisible();
	await expect(page.getByText("Cached Release 10001")).toHaveCount(0);
});

test("admin routes do not trust 30-day startup cache for privileged first paint", async ({
	page,
}) => {
	await seedPersistentAdminWarmCaches(page);
	await installAppAuthMocks(page, {
		meStatus: 200,
		meDelayMs: 900,
		isAdmin: false,
	});

	await page.goto("/admin/users", { waitUntil: "domcontentloaded" });

	await expect(page.getByText("Cached Admin")).toHaveCount(0);
	await expect(
		page.getByRole("navigation", { name: "管理员导航" }),
	).toHaveCount(0);

	await expect(page).toHaveURL(/\/$/);
	await expect(page.locator("[data-landing-login-card]")).toHaveCount(0);
});

test("admin deep links wait for live auth before redirecting stale 30-day cache", async ({
	page,
}) => {
	await seedPersistentAdminWarmCaches(page);
	await installAppAuthMocks(page, {
		meStatus: 200,
		meDelayMs: 900,
		isAdmin: true,
	});

	await page.goto("/admin/users", { waitUntil: "domcontentloaded" });

	await expect(page).toHaveURL(/\/admin\/users$/);
	await expect(page.locator("[data-landing-login-card]")).toHaveCount(0);
	await expect(page.getByText("Cached Admin")).toHaveCount(0);
	await expect(
		page.getByRole("navigation", { name: "管理员导航" }),
	).toHaveCount(0);
	await expect(page.getByRole("heading", { name: "用户管理" })).toBeVisible();
	await expect(
		page.getByRole("navigation", { name: "管理员导航" }),
	).toBeVisible();
});

test("logout clears startup cache before redirecting to anonymous landing", async ({
	page,
}) => {
	await seedPersistentAuthCache(page);
	await installAppAuthMocks(page, {
		meStatus: 200,
		isAdmin: true,
		logoutToAnonymous: true,
	});

	await page.goto("/");
	await expect(
		page.getByRole("heading", { level: 1, name: "OctoRill" }),
	).toBeVisible();

	await page.getByRole("button", { name: "查看账号信息" }).click();
	await page.getByRole("link", { name: "退出登录" }).click();

	await expect(page.getByRole("link", { name: "连接到 GitHub" })).toBeVisible();
	await expect
		.poll(async () =>
			page.evaluate(() =>
				window.localStorage.getItem("octo-rill.auth-bootstrap.v3"),
			),
		)
		.toBeNull();
});

test("dashboard to admin to dashboard stays in SPA mode and does not re-bootstrap auth", async ({
	page,
}) => {
	const tracker = await installAppAuthMocks(page, {
		meStatus: 200,
		isAdmin: true,
	});

	await page.goto("/");
	await expect(
		page.getByRole("heading", { level: 1, name: "OctoRill" }),
	).toBeVisible();
	await expect(page.getByRole("link", { name: "连接到 GitHub" })).toHaveCount(
		0,
	);

	const navigationEntryCount = await page.evaluate(
		() => performance.getEntriesByType("navigation").length,
	);

	await page
		.locator("[data-dashboard-secondary-controls]")
		.getByRole("link", { name: "管理员面板" })
		.click();
	await expect(page).toHaveURL(/\/admin$/);
	await expect(page.getByRole("img", { name: "OctoRill" })).toBeVisible();
	await expect(
		page.getByRole("navigation", { name: "管理员导航" }),
	).toBeVisible();
	await expect(page.locator("[data-app-boot]")).toHaveCount(0);
	await expect(page.getByRole("link", { name: "连接到 GitHub" })).toHaveCount(
		0,
	);

	await page.getByRole("link", { name: "返回前台首页" }).click();
	await expect(page).toHaveURL(/\/$/);
	await expect(
		page.getByRole("heading", { level: 1, name: "OctoRill" }),
	).toBeVisible();
	await expect(page.locator("[data-app-boot]")).toHaveCount(0);
	await expect(page.getByRole("link", { name: "连接到 GitHub" })).toHaveCount(
		0,
	);

	expect(tracker.getMeCalls()).toBe(1);
	await expect
		.poll(() =>
			page.evaluate(() => performance.getEntriesByType("navigation").length),
		)
		.toBe(navigationEntryCount);
});

test("warm cache resumes the dashboard shell immediately before the network refresh completes", async ({
	page,
}) => {
	await seedWarmDashboardCache(page);
	let feedCalls = 0;

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			await sleep(1500);
			return json(
				route,
				buildMockMeResponse({
					id: "2f4k7m9p3x6c8v2a",
					github_user_id: 10,
					login: "octo-admin",
					name: "Octo Admin",
					avatar_url: null,
					email: "admin@example.com",
					is_admin: true,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			feedCalls += 1;
			await sleep(1500);
			return json(route, {
				items: [
					{
						kind: "release",
						ts: "2026-04-15T09:00:00Z",
						id: "fresh-20002",
						repo_full_name: "owner/repo",
						title: "Fresh Release 20002",
						body: "fresh body",
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/owner/repo/releases/tag/v20002",
						unread: null,
						translated: null,
						smart: null,
						reactions: null,
					},
				],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			await sleep(1500);
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			await sleep(1500);
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

		if (req.method() === "GET" && pathname === "/api/version") {
			return json(route, {
				ok: true,
				version: "2.4.6",
				source: "APP_EFFECTIVE_VERSION",
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

	await page.goto("/", { waitUntil: "domcontentloaded" });

	await expect(page.locator("[data-app-boot]")).toHaveCount(0);
	await expect(page.getByRole("link", { name: "连接到 GitHub" })).toHaveCount(
		0,
	);
	await expect(
		page.locator('[data-dashboard-hydration-source="warm-cache"]'),
	).toHaveCount(1);
	await expect(
		page.locator("[data-dashboard-secondary-controls]"),
	).toBeVisible();
	await expect(page.getByText("Fresh Release 20002")).toBeVisible();
	expect(feedCalls).toBeGreaterThan(0);
});
