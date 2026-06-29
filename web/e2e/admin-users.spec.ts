import { type Page, type Route, expect, test } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

type MockUser = {
	id: string;
	github_user_id: number;
	login: string;
	name: string | null;
	avatar_url: string | null;
	email: string | null;
	is_admin: boolean;
	is_disabled: boolean;
	repo_total: number;
	include_own_releases: boolean;
	last_active_at: string | null;
	created_at: string;
	updated_at: string;
};

const CURRENT_USER_ID = "2f4k7m9p3x6c8v2a";
const STANDARD_USER_ID = "3g5n8q2r4y7d9w3b";
const LONG_ADMIN_LOGIN = "storybook-admin-with-a-very-long-login-name";

test.describe.configure({ mode: "serial" });

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

async function installFrozenNow(page: Page, isoString: string) {
	const fixedTime = new Date(isoString).getTime();
	await page.addInitScript((time) => {
		const RealDate = Date;
		function MockDate(this: Date, ...args: unknown[]) {
			if (!new.target) {
				return new RealDate(time).toString();
			}
			if (args.length === 0) {
				return new RealDate(time);
			}
			return Reflect.construct(RealDate, args) as Date;
		}
		MockDate.UTC = RealDate.UTC;
		MockDate.parse = RealDate.parse;
		MockDate.now = () => time;
		MockDate.prototype = RealDate.prototype;
		Object.setPrototypeOf(MockDate, RealDate);
		window.Date = MockDate as unknown as DateConstructor;
	}, fixedTime);
}

async function installBaseMocks(
	page: Page,
	options: {
		isAdmin: boolean;
		adminApiForbidden?: boolean;
		patchMode?: "ok" | "conflict_last_admin";
		extraUsers?: number;
		currentUserLogin?: string;
	},
) {
	let currentUserIsAdmin = options.isAdmin;
	const currentUserLogin = options.currentUserLogin ?? "octo-admin";
	const users: MockUser[] = [
		{
			id: CURRENT_USER_ID,
			github_user_id: 10,
			login: currentUserLogin,
			name: "Octo Admin",
			avatar_url: null,
			email: "admin@example.com",
			is_admin: true,
			is_disabled: false,
			repo_total: 21,
			include_own_releases: true,
			last_active_at: "2026-02-26T08:00:00Z",
			created_at: "2026-02-24T08:00:00Z",
			updated_at: "2026-02-24T08:00:00Z",
		},
		{
			id: STANDARD_USER_ID,
			github_user_id: 20,
			login: "octo-user",
			name: "Octo User",
			avatar_url: null,
			email: "user@example.com",
			is_admin: false,
			is_disabled: false,
			repo_total: 7,
			include_own_releases: false,
			last_active_at: "2026-02-26T07:00:00Z",
			created_at: "2026-02-25T08:00:00Z",
			updated_at: "2026-02-25T08:00:00Z",
		},
	];
	for (let index = 0; index < (options.extraUsers ?? 0); index += 1) {
		users.push({
			id: `extra-user-${index + 1}`,
			github_user_id: 30 + index,
			login: `extra-user-${index + 1}`,
			name: `Extra User ${index + 1}`,
			avatar_url: null,
			email: `extra-user-${index + 1}@example.com`,
			is_admin: false,
			is_disabled: false,
			repo_total: index + 1,
			include_own_releases: index % 2 === 0,
			last_active_at: "2026-02-26T06:00:00Z",
			created_at: "2026-02-25T09:00:00Z",
			updated_at: "2026-02-25T09:00:00Z",
		});
	}

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: CURRENT_USER_ID,
					github_user_id: 10,
					login: currentUserLogin,
					name: "Octo Admin",
					avatar_url: null,
					email: "admin@example.com",
					is_admin: currentUserIsAdmin,
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

		if (req.method() === "GET" && pathname === "/api/admin/dashboard") {
			return json(route, {
				generated_at: "2026-02-26T08:00:00Z",
				time_zone: "Asia/Shanghai",
				summary: {
					total_users: users.length,
					active_users_today: 0,
					ongoing_tasks_total: 0,
					queued_tasks: 0,
					running_tasks: 0,
					ongoing_by_task: {
						translations: 0,
						summaries: 0,
						briefs: 0,
					},
				},
				today_live: {
					date: "2026-02-26",
					total_users: users.length,
					active_users: 0,
					ongoing_tasks_total: 0,
					queued_tasks: 0,
					running_tasks: 0,
				},
				status_breakdown: {
					queued_total: 0,
					running_total: 0,
					succeeded_total: 0,
					failed_total: 0,
					canceled_total: 0,
					business_counts: {
						ok: 0,
						partial: 0,
						failed: 0,
						disabled: 0,
					},
					total: 0,
					items: [],
				},
				task_share: [],
				trend_points: [],
				llm_health: {
					calls_24h: 0,
					failed_24h: 0,
					last_failure_at: null,
					top_failure_reasons: [],
					top_failure_sources: [],
				},
				window_meta: {
					selected_window: "7d",
					available_windows: ["7d", "30d"],
					window_start: "2026-02-20",
					window_end: "2026-02-26",
					point_count: 7,
				},
			});
		}

		if (pathname === "/api/admin/users") {
			if (options.adminApiForbidden || !currentUserIsAdmin) {
				return json(
					route,
					{
						ok: false,
						error: { code: "forbidden_admin_only", message: "forbidden" },
					},
					403,
				);
			}
			if (req.method() === "GET") {
				const role = url.searchParams.get("role") ?? "all";
				const status = url.searchParams.get("status") ?? "all";
				const query = (url.searchParams.get("query") ?? "").toLowerCase();
				const filtered = users.filter((u) => {
					if (role === "admin" && !u.is_admin) return false;
					if (role === "user" && u.is_admin) return false;
					if (status === "enabled" && u.is_disabled) return false;
					if (status === "disabled" && !u.is_disabled) return false;
					if (!query) return true;
					return (
						u.login.toLowerCase().includes(query) ||
						(u.name ?? "").toLowerCase().includes(query) ||
						(u.email ?? "").toLowerCase().includes(query)
					);
				});
				return json(route, {
					items: filtered,
					page: 1,
					page_size: 20,
					total: filtered.length,
					guard: {
						admin_total: users.filter((u) => u.is_admin).length,
						active_admin_total: users.filter(
							(u) => u.is_admin && !u.is_disabled,
						).length,
					},
				});
			}
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/users/") &&
			pathname.endsWith("/profile")
		) {
			const id = decodeURIComponent(pathname.split("/").at(-2) ?? "");
			const target = users.find((u) => u.id === id);
			if (!target) {
				return json(
					route,
					{ ok: false, error: { code: "not_found", message: "not found" } },
					404,
				);
			}
			return json(route, {
				user_id: target.id,
				daily_brief_local_time: "08:00",
				daily_brief_time_zone: "Asia/Shanghai",
				include_own_releases: target.include_own_releases,
				last_active_at: target.last_active_at,
			});
		}

		if (
			req.method() === "PATCH" &&
			pathname.startsWith("/api/admin/users/") &&
			pathname.endsWith("/profile")
		) {
			const id = decodeURIComponent(pathname.split("/").at(-2) ?? "");
			const target = users.find((u) => u.id === id);
			if (!target) {
				return json(
					route,
					{ ok: false, error: { code: "not_found", message: "not found" } },
					404,
				);
			}
			const body = req.postDataJSON() as {
				daily_brief_local_time?: string;
				daily_brief_time_zone?: string;
			};
			return json(route, {
				user_id: target.id,
				daily_brief_local_time: body.daily_brief_local_time ?? "08:00",
				daily_brief_time_zone: body.daily_brief_time_zone ?? "Asia/Shanghai",
				include_own_releases: target.include_own_releases,
				last_active_at: target.last_active_at,
			});
		}

		if (req.method() === "PATCH" && pathname.startsWith("/api/admin/users/")) {
			const id = decodeURIComponent(pathname.split("/").at(-1) ?? "");
			const body = req.postDataJSON() as {
				is_admin?: boolean;
				is_disabled?: boolean;
			};
			if (
				options.patchMode === "conflict_last_admin" &&
				id === STANDARD_USER_ID &&
				body.is_disabled === true
			) {
				return json(
					route,
					{
						ok: false,
						error: {
							code: "last_admin_guard",
							message: "at least one active admin is required",
						},
					},
					409,
				);
			}
			const target = users.find((u) => u.id === id);
			if (!target) {
				return json(
					route,
					{ ok: false, error: { code: "not_found", message: "not found" } },
					404,
				);
			}
			if (typeof body.is_admin === "boolean") {
				target.is_admin = body.is_admin;
				if (target.id === CURRENT_USER_ID) {
					currentUserIsAdmin = body.is_admin;
				}
			}
			if (typeof body.is_disabled === "boolean") {
				target.is_disabled = body.is_disabled;
			}
			target.updated_at = "2026-02-25T10:00:00Z";
			return json(route, target);
		}

		return json(
			route,
			{ error: { message: `unhandled ${req.method()} ${pathname}` } },
			404,
		);
	});
}

function userRow(page: Page, userId: string) {
	return page.locator(`[data-user-row][data-user-id="${userId}"]`);
}

test("admin user can manage users in admin panel", async ({ page }) => {
	await installBaseMocks(page, { isAdmin: true });
	await page.goto("/");

	await page.getByRole("link", { name: "管理员面板" }).click();
	await expect(page).toHaveURL(/\/admin$/);
	await expect(page.locator("[data-admin-header-main-row]")).toBeVisible({
		timeout: 10_000,
	});
	await expect(
		page.getByRole("navigation", { name: "管理员导航" }),
	).toBeVisible();
	await page.getByRole("link", { name: "用户管理" }).click();
	await expect(page).toHaveURL(/\/admin\/users$/);
	await expect(
		page.getByText(
			"这里聚焦用户资料与账号状态治理；若需查看整体运营指标，请切换到仪表盘。",
		),
	).toBeVisible();
	await expect(
		page.getByText("管理账号角色与状态：支持筛选、升降管理员、启用/禁用。"),
	).toBeVisible();
	await expect(
		page.getByRole("textbox", { name: "搜索 login、name 或 email" }),
	).toBeVisible();
	await expect(
		page.getByRole("combobox", { name: "按角色筛选" }),
	).toBeVisible();
	await expect(
		page.getByRole("combobox", { name: "按状态筛选" }),
	).toBeVisible();
	const revokeAdminButton = page
		.getByRole("button", { name: "撤销管理员" })
		.first();
	await expect(revokeAdminButton).toBeDisabled();
	await expect(page.getByText("唯一管理员，不能撤销")).toBeVisible();

	const standardUserRow = userRow(page, STANDARD_USER_ID);
	await expect(standardUserRow).toContainText("最后活动：");
	await expect(standardUserRow).toContainText("普通用户");
	await expect(standardUserRow).toContainText("有效关注：7");
	await expect(standardUserRow).toContainText("我的发布：未纳入");
	await standardUserRow.getByRole("button", { name: "详情" }).click();
	const profileSheet = page.getByRole("dialog", { name: "用户详情" });
	await expect(profileSheet).toBeVisible();
	await expect(profileSheet.getByText("UID")).toBeVisible();
	await expect(profileSheet.getByText(STANDARD_USER_ID)).toBeVisible();
	await expect(profileSheet).toContainText("有效关注仓库数 7");
	await expect(profileSheet.getByLabel("日报时间")).toContainText("08:00");
	await expect(profileSheet.getByLabel("IANA 时区")).toHaveValue(
		"Asia/Shanghai",
	);
	await page.getByRole("button", { name: "关闭", exact: true }).click();
	await expect(profileSheet).toHaveCount(0);
	await standardUserRow.getByRole("button", { name: "设为管理员" }).click();
	await expect(
		page.getByRole("alertdialog", { name: "确认管理员变更" }),
	).toBeVisible();
	await page.getByRole("button", { name: "确认更改" }).click();
	await expect(standardUserRow).toContainText("管理员");
	await expect(revokeAdminButton).toBeEnabled();

	await standardUserRow.getByRole("button", { name: "禁用" }).click();
	await expect(standardUserRow).toContainText("已禁用");
});

test("admin action error remains visible after list refresh", async ({
	page,
}) => {
	await installBaseMocks(page, {
		isAdmin: true,
		patchMode: "conflict_last_admin",
	});
	await page.goto("/");

	await page.getByRole("link", { name: "管理员面板" }).click();
	await page.getByRole("link", { name: "用户管理" }).click();
	await expect(page).toHaveURL(/\/admin\/users$/);
	await expect(page.getByRole("heading", { name: "用户管理" })).toBeVisible();

	const standardUserRow = userRow(page, STANDARD_USER_ID);
	await standardUserRow.getByRole("button", { name: "禁用" }).click();

	await expect(
		page.getByText("至少保留一名启用管理员，当前操作已被拦截。"),
	).toBeVisible();
});

test("self-demoted admin is redirected out of admin panel", async ({
	page,
}) => {
	await installBaseMocks(page, { isAdmin: true });
	await page.goto("/");

	await page.getByRole("link", { name: "管理员面板" }).click();
	await page.getByRole("link", { name: "用户管理" }).click();
	await expect(page).toHaveURL(/\/admin\/users$/);
	await expect(
		page.getByText("管理账号角色与状态：支持筛选、升降管理员、启用/禁用。"),
	).toBeVisible();

	const standardUserRow = userRow(page, STANDARD_USER_ID);
	await standardUserRow.getByRole("button", { name: "设为管理员" }).click();
	await page.getByRole("button", { name: "确认更改" }).click();
	await expect(standardUserRow).toContainText("管理员");

	const selfRow = userRow(page, CURRENT_USER_ID);
	await selfRow.getByRole("button", { name: "撤销管理员" }).click();
	await page.getByRole("button", { name: "确认更改" }).click();

	await expect(page).toHaveURL("/");
	await expect(page.getByRole("link", { name: "管理员面板" })).toHaveCount(0);
});

test("non-admin user cannot stay on admin route", async ({ page }) => {
	await installBaseMocks(page, { isAdmin: false, adminApiForbidden: true });
	await page.goto("/admin");

	await expect(page).toHaveURL("/");
	await expect(page.getByRole("link", { name: "管理员面板" })).toHaveCount(0);
	await expect(page.getByRole("heading", { name: "用户管理" })).toHaveCount(0);
});

test("admin panel keeps header utilities inline on tablet widths", async ({
	page,
}) => {
	await installBaseMocks(page, {
		isAdmin: true,
		extraUsers: 10,
		currentUserLogin: LONG_ADMIN_LOGIN,
	});

	for (const viewport of [
		{ width: 640, height: 960 },
		{ width: 757, height: 827 },
		{ width: 853, height: 1280 },
		{ width: 1023, height: 1280 },
	]) {
		await test.step(`${viewport.width}x${viewport.height}`, async () => {
			await page.setViewportSize(viewport);
			await page.goto("/admin");

			await expect(
				page.getByRole("navigation", { name: "管理员导航" }),
			).toBeVisible();
			await expect(
				page.getByRole("link", { name: "返回前台首页" }),
			).toBeVisible();

			const layout = await page.evaluate(() => {
				const mainRowElement = document.querySelector(
					"[data-admin-header-main-row]",
				);
				const navBlockElement = document.querySelector(
					"[data-admin-nav-block]",
				);
				const actionClusterElement = document.querySelector(
					"[data-admin-primary-actions]",
				);
				const loginLabelElement = document.querySelector(
					"[data-admin-login-label]",
				);
				if (
					!(mainRowElement instanceof HTMLElement) ||
					!(navBlockElement instanceof HTMLElement) ||
					!(actionClusterElement instanceof HTMLElement) ||
					!(loginLabelElement instanceof HTMLElement)
				) {
					throw new Error("Expected admin header layout anchors");
				}

				const mainRect = mainRowElement.getBoundingClientRect();
				const navRect = navBlockElement.getBoundingClientRect();
				const actionRect = actionClusterElement.getBoundingClientRect();
				const loginRect = loginLabelElement.getBoundingClientRect();
				return {
					rowOverflow: mainRowElement.scrollWidth - mainRowElement.clientWidth,
					actionTopDelta: actionRect.top - mainRect.top,
					actionVsNavTopDelta: actionRect.top - navRect.top,
					actionRight: actionRect.right,
					rowRightGap: mainRect.right - actionRect.right,
					loginRight: loginRect.right,
					rowRight: mainRect.right,
				};
			});

			expect(layout.rowOverflow).toBeLessThanOrEqual(1);
			expect(layout.actionTopDelta).toBeLessThanOrEqual(12);
			expect(layout.actionVsNavTopDelta).toBeLessThanOrEqual(12);
			expect(layout.actionRight).toBeLessThanOrEqual(layout.rowRight + 1);
			expect(layout.rowRightGap).toBeLessThanOrEqual(12);
			expect(layout.loginRight).toBeLessThanOrEqual(layout.rowRight + 1);
		});
	}
});

test("admin users list keeps compact single-line rows with horizontal scroll on narrow screens", async ({
	page,
}) => {
	await installBaseMocks(page, {
		isAdmin: true,
		currentUserLogin: LONG_ADMIN_LOGIN,
	});
	await page.setViewportSize({ width: 768, height: 1024 });
	await page.goto("/admin/users");

	const tableShell = page.locator("[data-admin-users-table-shell]");
	await expect(tableShell).toBeVisible();
	const standardUserRow = userRow(page, STANDARD_USER_ID);
	await expect(standardUserRow).toContainText("有效关注：7");
	await expect(standardUserRow).toContainText("我的发布：未纳入");

	const metrics = await page.evaluate((userId) => {
		const shell = document.querySelector(
			"[data-admin-users-table-shell]",
		) as HTMLElement | null;
		const scrollContainer = shell?.querySelector(
			'[data-slot="table-container"]',
		) as HTMLElement | null;
		const row = document.querySelector(
			`[data-user-row][data-user-id="${userId}"]`,
		) as HTMLElement | null;
		const loginText = row?.querySelector("td:nth-child(1) p:nth-child(1)");
		const secondaryText = row?.querySelector("td:nth-child(1) p:nth-child(2)");
		if (
			!(shell instanceof HTMLElement) ||
			!(scrollContainer instanceof HTMLElement) ||
			!(row instanceof HTMLElement) ||
			!(loginText instanceof HTMLElement) ||
			!(secondaryText instanceof HTMLElement)
		) {
			throw new Error("Expected admin users table metrics anchors");
		}
		return {
			shellScrollOverflow:
				scrollContainer.scrollWidth - scrollContainer.clientWidth,
			rowHeight: row.getBoundingClientRect().height,
			loginLineHeight: Number.parseFloat(
				getComputedStyle(loginText).lineHeight,
			),
			secondaryLineHeight: Number.parseFloat(
				getComputedStyle(secondaryText).lineHeight,
			),
			loginWhiteSpace: getComputedStyle(loginText).whiteSpace,
			secondaryWhiteSpace: getComputedStyle(secondaryText).whiteSpace,
		};
	}, STANDARD_USER_ID);

	expect(metrics.shellScrollOverflow).toBeGreaterThan(0);
	expect(metrics.rowHeight).toBeLessThanOrEqual(
		metrics.loginLineHeight + metrics.secondaryLineHeight + 40,
	);
	expect(metrics.loginWhiteSpace).toBe("nowrap");
	expect(metrics.secondaryWhiteSpace).toBe("nowrap");
});

test("admin users list keeps the action column visible on desktop widths", async ({
	page,
}) => {
	await installBaseMocks(page, {
		isAdmin: true,
		currentUserLogin: LONG_ADMIN_LOGIN,
	});
	await page.setViewportSize({ width: 1600, height: 1100 });
	await page.goto("/admin/users");

	const tableShell = page.locator("[data-admin-users-table-shell]");
	await expect(tableShell).toBeVisible();
	await expect(userRow(page, STANDARD_USER_ID)).toBeVisible();

	const layout = await page.evaluate((userId) => {
		const shell = document.querySelector(
			"[data-admin-users-table-shell]",
		) as HTMLElement | null;
		const container = shell?.querySelector(
			'[data-slot="table-container"]',
		) as HTMLElement | null;
		const row = document.querySelector(
			`[data-user-row][data-user-id="${userId}"]`,
		) as HTMLElement | null;
		const actionCell = row?.querySelector(
			"td:nth-child(5)",
		) as HTMLElement | null;
		const actionButtons = Array.from(
			actionCell?.querySelectorAll("button") ?? [],
		) as HTMLElement[];
		if (
			!(shell instanceof HTMLElement) ||
			!(container instanceof HTMLElement) ||
			!(row instanceof HTMLElement) ||
			!(actionCell instanceof HTMLElement) ||
			actionButtons.length === 0
		) {
			throw new Error("Expected desktop admin users layout anchors");
		}

		const shellRect = shell.getBoundingClientRect();
		const actionRect = actionCell.getBoundingClientRect();
		const lastButtonRect =
			actionButtons.at(-1)?.getBoundingClientRect() ?? actionRect;

		return {
			scrollOverflow: container.scrollWidth - container.clientWidth,
			actionCellLeft: actionRect.left,
			actionCellRight: actionRect.right,
			lastButtonRight: lastButtonRect.right,
			shellLeft: shellRect.left,
			shellRight: shellRect.right,
		};
	}, STANDARD_USER_ID);

	expect(layout.scrollOverflow).toBeLessThanOrEqual(1);
	expect(layout.actionCellLeft).toBeGreaterThanOrEqual(layout.shellLeft - 1);
	expect(layout.actionCellRight).toBeLessThanOrEqual(layout.shellRight + 1);
	expect(layout.lastButtonRight).toBeLessThanOrEqual(layout.shellRight + 1);
});

test.describe("mobile admin shell", () => {
	test.use({ viewport: { width: 390, height: 844 } });

	test("admin panel shares the compact mobile shell without breaking navigation", async ({
		page,
	}) => {
		await installBaseMocks(page, { isAdmin: true, extraUsers: 10 });
		await page.goto("/admin");

		await expect(page.getByRole("img", { name: "OctoRill" })).toBeVisible();
		await expect(
			page.getByRole("navigation", { name: "管理员导航" }),
		).toBeVisible();
		await expect(
			page.locator("[data-app-meta-footer-hidden='false']"),
		).toHaveCount(1);

		await page.evaluate(() => window.scrollTo(0, 720));
		await page.waitForTimeout(120);
		await expect(
			page.locator("[data-app-meta-footer-hidden='true']"),
		).toHaveCount(1);
		await expect(
			page.locator("[data-admin-header-compact='true']"),
		).toHaveCount(1);

		await page.evaluate(() => window.scrollTo(0, 240));
		await page.waitForTimeout(120);
		await expect(
			page.locator("[data-admin-header-compact='true']"),
		).toHaveCount(0);
		await expect(
			page.getByRole("navigation", { name: "管理员导航" }),
		).toBeVisible();

		await page.evaluate(() => window.scrollTo(0, 0));
		await page.waitForTimeout(120);
		await expect(
			page.locator("[data-app-meta-footer-hidden='false']"),
		).toHaveCount(1);
	});
});

test.describe("daily brief time formatting", () => {
	test.describe("DST browser timezone", () => {
		test.use({ timezoneId: "America/New_York" });

		test("daily brief time uses the current browser DST offset", async ({
			page,
		}) => {
			await installFrozenNow(page, "2026-07-15T12:00:00Z");
			await installBaseMocks(page, { isAdmin: true });
			await page.goto("/");

			await page.getByRole("link", { name: "管理员面板" }).click();
			await page.getByRole("link", { name: "用户管理" }).click();
			const standardUserRow = userRow(page, STANDARD_USER_ID);
			await standardUserRow.getByRole("button", { name: "详情" }).click();

			const profileSheet = page.getByRole("dialog", { name: "用户详情" });
			await expect(profileSheet.getByLabel("日报时间")).toContainText("08:00");
			await expect(profileSheet.getByLabel("IANA 时区")).toHaveValue(
				"Asia/Shanghai",
			);
		});
	});

	test.describe("fixed-offset browser timezone", () => {
		test.use({ timezoneId: "Asia/Shanghai" });

		test("daily brief time stays correct in a non-DST timezone", async ({
			page,
		}) => {
			await installFrozenNow(page, "2026-07-15T12:00:00Z");
			await installBaseMocks(page, { isAdmin: true });
			await page.goto("/");

			await page.getByRole("link", { name: "管理员面板" }).click();
			await page.getByRole("link", { name: "用户管理" }).click();
			const standardUserRow = userRow(page, STANDARD_USER_ID);
			await standardUserRow.getByRole("button", { name: "详情" }).click();

			const profileSheet = page.getByRole("dialog", { name: "用户详情" });
			await expect(profileSheet.getByLabel("日报时间")).toContainText("08:00");
			await expect(profileSheet.getByLabel("IANA 时区")).toHaveValue(
				"Asia/Shanghai",
			);
		});
	});
});
