import {
	type Locator,
	type Page,
	type Route,
	expect,
	test,
} from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function svgAvatarDataUrl(
	label: string,
	background: string,
	foreground = "#ffffff",
) {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240"><rect width="240" height="240" rx="120" fill="${background}"/><text x="120" y="132" font-family="Inter,Arial,sans-serif" font-size="44" font-weight="700" text-anchor="middle" fill="${foreground}">${label}</text></svg>`,
	)}`;
}

function buildReleaseFeedItem(
	id: string,
	overrides: Record<string, unknown> = {},
) {
	return {
		kind: "release",
		ts: "2026-04-09T08:00:00Z",
		id,
		repo_full_name: "owner/repo",
		repo_visual: null,
		title: `Release ${id}`,
		body: "- mobile shell proof\n- tighten spacing\n- keep sticky rail visible",
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `https://github.com/owner/repo/releases/tag/v${id}`,
		unread: null,
		translated: null,
		smart: null,
		reactions: null,
		...overrides,
	};
}

function buildSocialFeedItem(
	id: string,
	kind: "repo_star_received" | "follower_received",
) {
	return {
		kind,
		ts: "2026-04-09T08:00:00Z",
		id,
		repo_full_name: kind === "repo_star_received" ? "owner/repo" : null,
		repo_visual: null,
		title: null,
		body: null,
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: "https://github.com/octo-user",
		unread: null,
		actor: {
			login: kind === "repo_star_received" ? "linus" : "yyx990803",
			avatar_url: svgAvatarDataUrl(
				kind === "repo_star_received" ? "LN" : "YY",
				kind === "repo_star_received" ? "#6d4aff" : "#1d4ed8",
			),
			html_url:
				kind === "repo_star_received"
					? "https://github.com/linus"
					: "https://github.com/yyx990803",
		},
		translated: null,
		smart: null,
		reactions: null,
	};
}

async function getLocatorCenter(locator: Locator) {
	const box = await locator.evaluate((element) => {
		const rect = element.getBoundingClientRect();
		return {
			x: rect.left + rect.width / 2,
			y: rect.top + rect.height / 2,
			width: rect.width,
			height: rect.height,
		};
	});
	expect(box.width).toBeGreaterThan(0);
	expect(box.height).toBeGreaterThan(0);
	return box;
}

async function tapLocator(page: Page, locator: Locator) {
	const box = await getLocatorCenter(locator);
	await page.touchscreen.tap(box.x, box.y);
}

function buildReactionFooterReady() {
	return {
		counts: {
			plus1: 4,
			laugh: 1,
			heart: 2,
			hooray: 1,
			rocket: 1,
			eyes: 0,
		},
		viewer: {
			plus1: true,
			laugh: false,
			heart: false,
			hooray: false,
			rocket: false,
			eyes: false,
		},
		status: "ready",
	};
}

function rectsIntersect(
	a: { left: number; right: number; top: number; bottom: number },
	b: { left: number; right: number; top: number; bottom: number },
) {
	return (
		a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
	);
}

test("dashboard keeps sync as a single header action for admins", async ({
	page,
}) => {
	test.slow();

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: "2f4k7m9p3x6c8v2a",
					github_user_id: 10,
					login: "octo-admin",
					name: "Octo Admin",
					avatar_url: svgAvatarDataUrl("OA", "#4f6a98"),
					email: "admin@example.com",
					is_admin: true,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			return json(route, {
				items: [
					{
						kind: "release",
						ts: "2026-04-09T08:00:00Z",
						id: "20001",
						repo_full_name: "owner/repo",
						title: "Release 20001",
						body: "hello",
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/owner/repo/releases/tag/v20001",
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

	await page.goto("/");

	await expect(page.getByRole("button", { name: "同步" })).toHaveCount(1);
	await expect(page.locator("[data-dashboard-brand-heading]")).toHaveCount(1);
	await expect(
		page.getByRole("heading", { level: 1, name: "OctoRill" }),
	).toBeVisible();
	await expect(
		page.getByText("GitHub 动态 · 中文翻译 · 日报与 Inbox"),
	).toBeVisible();
	await expect(page.getByText(/Logged in as\s+octo-admin/)).toHaveCount(0);
	await expect(page.getByText(/Loaded\s+\d+/)).toHaveCount(0);
	await expect(
		page.locator("[data-dashboard-primary-actions]").getByRole("button", {
			name: "同步",
		}),
	).toBeVisible();
	await expect(
		page.getByRole("button", { name: "查看账号信息" }),
	).toBeVisible();
	await page.getByRole("button", { name: "查看账号信息" }).click();
	await expect(page.locator("[data-dashboard-user-card]")).toBeVisible();
	await expect(page.getByText("Octo Admin")).toBeVisible();
	await expect(page.getByText("@octo-admin")).toBeVisible();
	await expect(page.getByText("admin@example.com")).toBeVisible();
	await expect(
		page.locator("[data-dashboard-user-card]").getByLabel("管理员"),
	).toBeVisible();
	await expect(
		page.locator("[data-dashboard-user-card]").getByRole("link", {
			name: "退出登录",
		}),
	).toBeVisible();
	const secondaryControls = page.locator("[data-dashboard-secondary-controls]");
	await expect(
		secondaryControls.getByRole("button", { name: "同步" }),
	).toHaveCount(0);
	await expect(
		secondaryControls.getByRole("link", { name: "同步" }),
	).toHaveCount(0);
	await expect(
		secondaryControls.getByRole("button", { name: "原文" }),
	).toBeVisible();
	await expect(
		secondaryControls.getByRole("button", { name: "翻译" }),
	).toBeVisible();
	await expect(
		secondaryControls.getByRole("button", { name: "智能" }),
	).toBeVisible();
	await expect(
		secondaryControls.getByRole("link", { name: "管理员面板" }),
	).toBeVisible();
});

test.describe("mobile dashboard shell", () => {
	test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });

	test("dashboard switches to compact mobile chrome and moves admin entry into the user menu", async ({
		page,
	}) => {
		await page.route("**/api/**", async (route) => {
			const req = route.request();
			const url = new URL(req.url());
			const { pathname } = url;

			if (req.method() === "GET" && pathname === "/api/me") {
				return json(
					route,
					buildMockMeResponse({
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo-admin",
						name: "Octo Admin",
						avatar_url: svgAvatarDataUrl("OA", "#4f6a98"),
						email: "admin@example.com",
						is_admin: true,
					}),
				);
			}

			if (req.method() === "GET" && pathname === "/api/feed") {
				return json(route, {
					items: [
						buildReleaseFeedItem("2630", {
							title: "v2.63.0",
							body: "- tighten release rollout path\n- reduce regression risk",
							html_url: "https://github.com/owner/repo/releases/tag/v2.63.0",
							translated: {
								lang: "zh-CN",
								status: "ready",
								title: "v2.63.0（稳定版）",
								summary: "- 收紧发布链路\n- 降低回归风险",
							},
							smart: {
								lang: "zh-CN",
								status: "ready",
								title: "v2.63.0 · 版本变化",
								summary: "- 自动整理版本变化摘要",
							},
							reactions: buildReactionFooterReady(),
						}),
						...Array.from({ length: 9 }, (_, index) =>
							buildReleaseFeedItem(String(20001 + index)),
						),
					],
					next_cursor: null,
				});
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

		await page.goto("/");

		await expect(page.locator("[data-dashboard-brand-subtitle]")).toBeHidden();
		const expandedWorkband = page.locator(
			"[data-dashboard-mobile-top-shell='expanded'][data-dashboard-mobile-top-shell-section='workband']",
		);
		await expect(expandedWorkband).toBeVisible();
		await expect(
			expandedWorkband.getByRole("tab", { name: "发布" }),
		).toBeVisible();
		const mobileHeader = page.locator("[data-dashboard-header-progress]");
		const appShellHeader = page.locator("[data-app-shell-header-interacting]");
		const laneMenuTrigger = expandedWorkband.locator(
			"[data-dashboard-mobile-lane-menu-trigger]",
		);
		await expect(laneMenuTrigger).toBeVisible();
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);
		await expect(
			page.getByRole("heading", { name: "v2.63.0 · 版本变化" }),
		).toBeVisible();
		expect(await page.locator("[data-dashboard-mobile-rail]").count()).toBe(0);
		expect(
			await page
				.locator("[data-dashboard-secondary-controls]")
				.getByRole("link", {
					name: "管理员面板",
				})
				.count(),
		).toBe(0);

		expect(
			await page
				.locator("[data-dashboard-mobile-control-band-row='lane']")
				.count(),
		).toBe(0);
		await page.evaluate(() => {
			const trigger = document.querySelector(
				"[data-dashboard-mobile-lane-menu-trigger]",
			);
			const shell = document.querySelector(
				"[data-app-shell-header-interacting]",
			);
			if (
				!(trigger instanceof HTMLElement) ||
				!(shell instanceof HTMLElement)
			) {
				throw new Error("Expected mobile lane trigger and app shell header");
			}
			const touchStates: string[] = [];
			trigger.addEventListener(
				"touchstart",
				() => {
					touchStates.push(
						shell.getAttribute("data-app-shell-header-interacting") ??
							"missing",
					);
				},
				{ once: true, passive: true },
			);
			(
				window as Window & { __laneMenuTouchHeaderStates?: string[] }
			).__laneMenuTouchHeaderStates = touchStates;
		});

		await page.evaluate(() => {
			const trigger = document.querySelector(
				"[data-dashboard-mobile-lane-menu-trigger]",
			);
			const shell = document.querySelector(
				"[data-app-shell-header-interacting]",
			);
			if (
				!(trigger instanceof HTMLElement) ||
				!(shell instanceof HTMLElement)
			) {
				throw new Error("Expected mobile lane trigger and app shell header");
			}
			const rect = trigger.getBoundingClientRect();
			const createTouchEvent = (
				type: "touchstart" | "touchmove",
				offsetX = 0,
				offsetY = 0,
			) => {
				const touchPoint = {
					clientX: rect.left + rect.width / 2 + offsetX,
					clientY: rect.top + rect.height / 2 + offsetY,
				};
				const event = new Event(type, {
					bubbles: true,
					cancelable: true,
				}) as Event & {
					touches: Array<typeof touchPoint>;
					targetTouches: Array<typeof touchPoint>;
					changedTouches: Array<typeof touchPoint>;
				};
				Object.defineProperty(event, "touches", { value: [touchPoint] });
				Object.defineProperty(event, "targetTouches", {
					value: [touchPoint],
				});
				Object.defineProperty(event, "changedTouches", {
					value: [touchPoint],
				});
				return event;
			};
			trigger.dispatchEvent(createTouchEvent("touchstart"));
			trigger.dispatchEvent(createTouchEvent("touchmove", 10, 0));
			(
				window as Window & { __laneMenuSyntheticHorizontalHeaderState?: string }
			).__laneMenuSyntheticHorizontalHeaderState =
				shell.getAttribute("data-app-shell-header-interacting") ?? "missing";
			trigger.dispatchEvent(createTouchEvent("touchmove", 0, -10));
			(
				window as Window & { __laneMenuSyntheticVerticalHeaderState?: string }
			).__laneMenuSyntheticVerticalHeaderState =
				shell.getAttribute("data-app-shell-header-interacting") ?? "missing";
		});
		await expect
			.poll(() =>
				page.evaluate(
					() =>
						(
							window as Window & {
								__laneMenuSyntheticHorizontalHeaderState?: string;
							}
						).__laneMenuSyntheticHorizontalHeaderState ?? "missing",
				),
			)
			.toBe("false");
		await expect
			.poll(() =>
				page.evaluate(
					() =>
						(
							window as Window & {
								__laneMenuSyntheticVerticalHeaderState?: string;
							}
						).__laneMenuSyntheticVerticalHeaderState ?? "missing",
				),
			)
			.toBe("false");
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);

		await page.evaluate(() => {
			const trigger = document.querySelector(
				"[data-dashboard-mobile-lane-menu-trigger]",
			);
			const shell = document.querySelector(
				"[data-app-shell-header-interacting]",
			);
			if (
				!(trigger instanceof HTMLElement) ||
				!(shell instanceof HTMLElement)
			) {
				throw new Error("Expected mobile lane trigger and app shell header");
			}
			const touchStates: string[] = [];
			trigger.addEventListener(
				"touchstart",
				() => {
					touchStates.push(
						shell.getAttribute("data-app-shell-header-interacting") ??
							"missing",
					);
				},
				{ once: true, passive: true },
			);
			(
				window as Window & { __laneMenuTouchHeaderStates?: string[] }
			).__laneMenuTouchHeaderStates = touchStates;
		});

		await tapLocator(page, laneMenuTrigger);
		await expect
			.poll(() =>
				page.evaluate(
					() =>
						(window as Window & { __laneMenuTouchHeaderStates?: string[] })
							.__laneMenuTouchHeaderStates ?? [],
				),
			)
			.toEqual(["false"]);
		await expect(
			page.locator("[data-dashboard-mobile-lane-menu-popover]"),
		).toBeVisible();
		await expect(
			page
				.locator("[data-dashboard-mobile-lane-menu-popover]")
				.getByRole("menuitemradio", { name: "原文" }),
		).toBeVisible();
		await page.evaluate(() => {
			const translatedOption = document
				.querySelector("[data-dashboard-mobile-lane-menu-popover]")
				?.querySelector(
					'[role="menuitemradio"][data-feed-page-lane="translated"]',
				);
			const shell = document.querySelector(
				"[data-app-shell-header-interacting]",
			);
			if (
				!(translatedOption instanceof HTMLElement) ||
				!(shell instanceof HTMLElement)
			) {
				throw new Error("Expected translated lane option and app shell header");
			}
			const rect = translatedOption.getBoundingClientRect();
			const createTouchEvent = (
				type: "touchstart" | "touchmove",
				offsetX = 0,
				offsetY = 0,
			) => {
				const touchPoint = {
					clientX: rect.left + rect.width / 2 + offsetX,
					clientY: rect.top + rect.height / 2 + offsetY,
				};
				const event = new Event(type, {
					bubbles: true,
					cancelable: true,
				}) as Event & {
					touches: Array<typeof touchPoint>;
					targetTouches: Array<typeof touchPoint>;
					changedTouches: Array<typeof touchPoint>;
				};
				Object.defineProperty(event, "touches", { value: [touchPoint] });
				Object.defineProperty(event, "targetTouches", {
					value: [touchPoint],
				});
				Object.defineProperty(event, "changedTouches", {
					value: [touchPoint],
				});
				return event;
			};
			translatedOption.dispatchEvent(createTouchEvent("touchstart"));
			translatedOption.dispatchEvent(createTouchEvent("touchmove", 10, 0));
			(
				window as Window & {
					__laneMenuOptionSyntheticHorizontalHeaderState?: string;
				}
			).__laneMenuOptionSyntheticHorizontalHeaderState =
				shell.getAttribute("data-app-shell-header-interacting") ?? "missing";
			translatedOption.dispatchEvent(createTouchEvent("touchmove", 0, -10));
			(
				window as Window & {
					__laneMenuOptionSyntheticVerticalHeaderState?: string;
				}
			).__laneMenuOptionSyntheticVerticalHeaderState =
				shell.getAttribute("data-app-shell-header-interacting") ?? "missing";
		});
		await expect
			.poll(() =>
				page.evaluate(
					() =>
						(
							window as Window & {
								__laneMenuOptionSyntheticHorizontalHeaderState?: string;
							}
						).__laneMenuOptionSyntheticHorizontalHeaderState ?? "missing",
				),
			)
			.toBe("false");
		await expect
			.poll(() =>
				page.evaluate(
					() =>
						(
							window as Window & {
								__laneMenuOptionSyntheticVerticalHeaderState?: string;
							}
						).__laneMenuOptionSyntheticVerticalHeaderState ?? "missing",
				),
			)
			.toBe("false");
		await page
			.locator("[data-dashboard-mobile-lane-menu-popover]")
			.getByRole("menuitemradio", { name: "翻译" })
			.click();
		expect(
			await page.locator("[data-dashboard-mobile-lane-menu-popover]").count(),
		).toBe(0);
		await expect(
			page.getByRole("heading", { name: "v2.63.0（稳定版）" }),
		).toBeVisible();
		await expandedWorkband.getByRole("tab", { name: "关注" }).click();
		await expect(laneMenuTrigger).toBeVisible();
		await expect(laneMenuTrigger).toBeDisabled();
		await laneMenuTrigger.click({ force: true });
		expect(
			await page.locator("[data-dashboard-mobile-lane-menu-popover]").count(),
		).toBe(0);
		await expandedWorkband.getByRole("tab", { name: "全部" }).click();
		await expect(laneMenuTrigger).toBeEnabled();

		const client = await page.context().newCDPSession(page);
		const dispatchTouch = async (
			type: "touchStart" | "touchMove" | "touchEnd",
			x?: number,
			y?: number,
		) => {
			await client.send("Input.dispatchTouchEvent", {
				type,
				touchPoints:
					type === "touchEnd"
						? []
						: [
								{
									x: x ?? 200,
									y: y ?? 0,
									radiusX: 5,
									radiusY: 5,
									force: 1,
									id: 1,
								},
							],
			});
		};

		await dispatchTouch("touchStart", 200, 500);
		await dispatchTouch("touchMove", 200, 430);
		await page.waitForTimeout(48);
		await expect(mobileHeader).toHaveAttribute(
			"data-dashboard-header-interacting",
			"true",
		);
		const intermediateProgress = await mobileHeader.evaluate((element) =>
			Number.parseFloat(
				element.getAttribute("data-dashboard-header-progress") ?? "0",
			),
		);
		expect(intermediateProgress).toBeGreaterThan(0.25);
		expect(intermediateProgress).toBeLessThan(0.9);
		await expect(mobileHeader).toHaveAttribute(
			"data-dashboard-header-compact",
			"false",
		);
		await dispatchTouch("touchMove", 200, 360);
		await page.waitForTimeout(48);
		await dispatchTouch("touchEnd");
		await page.waitForTimeout(320);
		await expect(mobileHeader).toHaveAttribute(
			"data-dashboard-header-interacting",
			"false",
		);
		await expect(mobileHeader).toHaveAttribute(
			"data-dashboard-header-progress",
			"1.000",
		);
		await expect(mobileHeader).toHaveAttribute(
			"data-dashboard-header-compact",
			"true",
		);

		await page.mouse.wheel(0, -120);
		await page.waitForTimeout(180);
		await expect(mobileHeader).toHaveAttribute(
			"data-dashboard-header-progress",
			"0.000",
		);
		await expect(mobileHeader).toHaveAttribute(
			"data-dashboard-header-compact",
			"false",
		);

		await dispatchTouch("touchStart", 200, 520);
		await dispatchTouch("touchMove", 200, 430);
		await page.waitForTimeout(48);
		await dispatchTouch("touchMove", 200, 360);
		await page.waitForTimeout(48);
		await dispatchTouch("touchEnd");
		await page.waitForTimeout(320);
		await expect(
			page.locator("[data-app-meta-footer-hidden='true']"),
		).toHaveCount(1);
		await expect(
			page.locator("[data-dashboard-header-compact='true']"),
		).toBeVisible();
		expect(await expandedWorkband.count()).toBe(0);
		expect(await page.locator("[data-dashboard-mobile-rail]").count()).toBe(0);

		await dispatchTouch("touchStart", 200, 360);
		await dispatchTouch("touchMove", 200, 404);
		await page.waitForTimeout(48);
		await dispatchTouch("touchMove", 200, 452);
		await page.waitForTimeout(48);
		await dispatchTouch("touchEnd");
		await page.waitForTimeout(240);
		expect(
			await page.locator("[data-dashboard-header-compact='true']").count(),
		).toBe(0);
		expect(await page.locator("[data-dashboard-mobile-rail]").count()).toBe(0);
		await expect(expandedWorkband).toBeVisible();

		await dispatchTouch("touchStart", 200, 520);
		await dispatchTouch("touchMove", 200, 430);
		await page.waitForTimeout(48);
		await dispatchTouch("touchMove", 200, 340);
		await page.waitForTimeout(48);
		await dispatchTouch("touchEnd");
		await page.waitForTimeout(320);
		await expect(
			page.locator("[data-dashboard-header-compact='true']"),
		).toBeVisible();
		expect(await page.locator("[data-dashboard-mobile-rail]").count()).toBe(0);
		expect(await expandedWorkband.count()).toBe(0);

		await page.getByRole("button", { name: "查看账号信息" }).click();
		await expect(
			page.locator("[data-dashboard-user-card]").getByRole("link", {
				name: "管理员面板",
			}),
		).toBeVisible();

		await page.evaluate(() => window.scrollTo(0, 0));
		await page.waitForTimeout(120);
		await expect(
			page.locator("[data-app-meta-footer-hidden='false']"),
		).toHaveCount(1);
	});

	test("dashboard skips inbox preload on mobile until the inbox tab is opened", async ({
		page,
	}) => {
		let notificationCalls = 0;

		await page.route("**/api/**", async (route) => {
			const req = route.request();
			const url = new URL(req.url());
			const { pathname } = url;

			if (req.method() === "GET" && pathname === "/api/me") {
				return json(
					route,
					buildMockMeResponse({
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo-admin",
						name: "Octo Admin",
						avatar_url: svgAvatarDataUrl("OA", "#4f6a98"),
						email: "admin@example.com",
						is_admin: true,
					}),
				);
			}

			if (req.method() === "GET" && pathname === "/api/feed") {
				return json(route, {
					items: Array.from({ length: 4 }, (_, index) =>
						buildReleaseFeedItem(String(21001 + index)),
					),
					next_cursor: null,
				});
			}

			if (req.method() === "GET" && pathname === "/api/notifications") {
				notificationCalls += 1;
				return json(route, [
					{
						thread_id: "91001",
						repo_full_name: "owner/repo",
						subject_title: "Build failed on main",
						subject_type: "CheckSuite",
						reason: "ci_activity",
						updated_at: "2026-04-09T08:02:00Z",
						unread: 1,
						html_url: null,
					},
				]);
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

		await page.goto("/");
		await expect(
			page.locator("[data-dashboard-sidebar-inbox='true']"),
		).toHaveCount(0);
		await expect(page.getByText("Build failed on main")).toHaveCount(0);
		expect(notificationCalls).toBe(0);

		await page.getByRole("tab", { name: "收件箱" }).click();
		await expect(
			page.locator("[data-dashboard-sidebar-inbox='true']"),
		).toHaveCount(0);
		await expect(page.getByText("Build failed on main")).toBeVisible();
		const syncInboxButton = page.getByRole("button", { name: "Sync inbox" });
		const githubLink = page.getByRole("link", { name: "GitHub" }).first();
		const syncInboxBox = await syncInboxButton.boundingBox();
		const githubBox = await githubLink.boundingBox();
		expect(syncInboxBox?.width ?? 0).toBeLessThanOrEqual(36);
		expect(githubBox?.width ?? 0).toBeLessThanOrEqual(36);
		expect(notificationCalls).toBe(1);
	});

	test("dashboard keeps the all-tab mobile shell pinned while tracking live viewport height changes", async ({
		page,
	}) => {
		await page.route("**/api/**", async (route) => {
			const req = route.request();
			const url = new URL(req.url());
			const { pathname } = url;

			if (req.method() === "GET" && pathname === "/api/me") {
				return json(
					route,
					buildMockMeResponse({
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo-admin",
						name: "Octo Admin",
						avatar_url: svgAvatarDataUrl("OA", "#4f6a98"),
						email: "admin@example.com",
						is_admin: true,
					}),
				);
			}

			if (req.method() === "GET" && pathname === "/api/feed") {
				return json(route, {
					items: [
						buildReleaseFeedItem("41001", {
							ts: "2026-04-09T16:40:00+08:00",
							title: "Release 41001",
							body: "- long all-tab mobile shell proof\n- keep the sticky header pinned\n- track the live viewport height",
							reactions: buildReactionFooterReady(),
						}),
						...Array.from({ length: 10 }, (_, index) =>
							buildReleaseFeedItem(String(41002 + index), {
								ts: `2026-04-0${Math.min(8, index + 1)}T12:00:00+08:00`,
								title: `Release ${41002 + index}`,
							}),
						),
					],
					next_cursor: null,
				});
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

		await page.goto("/?tab=all");

		const shell = page.locator("[data-app-shell-mobile-chrome='true']");
		const headerState = page.locator("[data-dashboard-header-progress]");
		const stickyHeader = page.locator("[data-app-shell-header='true']");
		const readShellState = async () =>
			page.evaluate(() => {
				const shellElement = document.querySelector(
					"[data-app-shell-mobile-chrome='true']",
				);
				const headerStateElement = document.querySelector(
					"[data-dashboard-header-progress]",
				);
				const stickyHeaderElement = document.querySelector(
					"[data-app-shell-header='true']",
				);
				if (
					!(shellElement instanceof HTMLElement) ||
					!(headerStateElement instanceof HTMLElement) ||
					!(stickyHeaderElement instanceof HTMLElement)
				) {
					throw new Error("Expected shell and header elements");
				}

				return {
					boundViewportHeight: Number.parseInt(
						shellElement.getAttribute("data-app-shell-viewport-height") ?? "0",
						10,
					),
					viewportHeight: Math.round(
						window.visualViewport?.height ?? window.innerHeight,
					),
					headerTop: Math.round(
						stickyHeaderElement.getBoundingClientRect().top,
					),
					compact:
						headerStateElement.getAttribute("data-dashboard-header-compact") ===
						"true",
				};
			});

		await expect(shell).toHaveAttribute(
			"data-app-shell-viewport-height-source",
			/^(visual-viewport|window-inner-height)$/,
		);
		await expect(headerState).toHaveAttribute(
			"data-dashboard-header-compact",
			"false",
		);
		const initialState = await readShellState();
		expect(
			Math.abs(initialState.boundViewportHeight - initialState.viewportHeight),
		).toBeLessThanOrEqual(1);

		await page.mouse.wheel(0, 460);
		await page.waitForTimeout(320);

		const compactState = await readShellState();
		await expect(headerState).toHaveAttribute(
			"data-dashboard-header-compact",
			"true",
		);
		expect(compactState.compact).toBe(true);
		expect(Math.abs(compactState.headerTop)).toBeLessThanOrEqual(1);
		expect(
			Math.abs(compactState.boundViewportHeight - compactState.viewportHeight),
		).toBeLessThanOrEqual(1);

		await page.setViewportSize({ width: 390, height: 760 });
		await page.waitForTimeout(220);
		const resizedCompactState = await readShellState();
		expect(resizedCompactState.compact).toBe(true);
		expect(Math.abs(resizedCompactState.headerTop)).toBeLessThanOrEqual(1);
		expect(
			Math.abs(
				resizedCompactState.boundViewportHeight -
					resizedCompactState.viewportHeight,
			),
		).toBeLessThanOrEqual(1);

		await page.evaluate(() => window.scrollTo({ top: 0, behavior: "auto" }));
		await page.waitForTimeout(220);
		await page.setViewportSize({ width: 390, height: 844 });
		await page.waitForTimeout(220);
		const restoredState = await readShellState();
		expect(Math.abs(restoredState.headerTop)).toBeLessThanOrEqual(1);
		expect(
			Math.abs(
				restoredState.boundViewportHeight - restoredState.viewportHeight,
			),
		).toBeLessThanOrEqual(1);
		await expect(headerState).toHaveAttribute(
			"data-dashboard-header-compact",
			"false",
		);
		await expect(stickyHeader).toBeVisible();
	});

	test("grouped day divider keeps a safe gap below the reaction footer on mobile", async ({
		page,
	}) => {
		await page.route("**/api/**", async (route) => {
			const req = route.request();
			const url = new URL(req.url());
			const { pathname } = url;

			if (req.method() === "GET" && pathname === "/api/me") {
				return json(
					route,
					buildMockMeResponse({
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo-admin",
						name: "Octo Admin",
						avatar_url: svgAvatarDataUrl("OA", "#4f6a98"),
						email: "admin@example.com",
						is_admin: true,
					}),
				);
			}

			if (req.method() === "GET" && pathname === "/api/feed") {
				return json(route, {
					items: [
						{
							...buildReleaseFeedItem("30001"),
							ts: "2026-04-09T16:40:00+08:00",
							title: "Release 30001",
							body: "- keep the reaction footer visible\n- preserve grouped feed divider spacing\n- do not let the next day header collide",
							reactions: buildReactionFooterReady(),
						},
						{
							...buildReleaseFeedItem("30002"),
							ts: "2026-04-08T11:12:00+08:00",
							title: "Release 30002",
							body: "- historical group without a brief\n- still render the generate brief action on narrow screens",
						},
					],
					next_cursor: null,
				});
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

		await page.goto("/?tab=all");

		const reactionFooter = page
			.locator('[data-reaction-footer="true"]')
			.first();
		const dayLabel = page.getByText(/^2026-04-08 · 1 条 Release$/);
		const dayHeader = dayLabel.locator(
			"xpath=ancestor::*[@data-feed-day-header='true'][1]",
		);
		const actionSlot = dayHeader.locator('[data-feed-day-action-slot="true"]');
		await expect(reactionFooter).toBeVisible();
		await expect(dayHeader).toBeVisible();
		await expect(dayLabel).toBeVisible();
		await expect(
			actionSlot.getByRole("button", { name: "生成日报" }),
		).toBeVisible();

		const footerBox = await reactionFooter.boundingBox();
		const headerBox = await dayHeader.boundingBox();
		const labelBox = await dayLabel.boundingBox();
		const actionBox = await actionSlot.boundingBox();
		expect(footerBox).not.toBeNull();
		expect(headerBox).not.toBeNull();
		expect(labelBox).not.toBeNull();
		expect(actionBox).not.toBeNull();
		if (!footerBox || !headerBox || !labelBox || !actionBox) {
			throw new Error("Expected footer/header/label/action geometry");
		}

		expect(
			headerBox.y - (footerBox.y + footerBox.height),
		).toBeGreaterThanOrEqual(8);
		expect(
			Math.min(labelBox.y, actionBox.y) - (footerBox.y + footerBox.height),
		).toBeGreaterThanOrEqual(8);
		expect(
			rectsIntersect(
				{
					left: labelBox.x,
					right: labelBox.x + labelBox.width,
					top: labelBox.y,
					bottom: labelBox.y + labelBox.height,
				},
				{
					left: actionBox.x,
					right: actionBox.x + actionBox.width,
					top: actionBox.y,
					bottom: actionBox.y + actionBox.height,
				},
			),
		).toBe(false);
	});

	test("grouped day divider keeps mixed activity label separate from the list action on mobile", async ({
		page,
	}) => {
		await page.route("**/api/**", async (route) => {
			const req = route.request();
			const url = new URL(req.url());
			const { pathname } = url;

			if (req.method() === "GET" && pathname === "/api/me") {
				return json(
					route,
					buildMockMeResponse({
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo-admin",
						name: "Octo Admin",
						avatar_url: svgAvatarDataUrl("OA", "#4f6a98"),
						email: "admin@example.com",
						is_admin: true,
					}),
				);
			}

			if (req.method() === "GET" && pathname === "/api/feed") {
				return json(route, {
					items: [
						{
							...buildReleaseFeedItem("31001"),
							ts: "2026-04-04T16:40:00+08:00",
							title: "Release 31001",
							body: "- keep the reaction footer visible\n- the next divider also carries mixed activity counts\n- the list action must remain readable",
							reactions: buildReactionFooterReady(),
						},
						{
							...buildReleaseFeedItem("31002"),
							ts: "2026-04-03T23:10:00+08:00",
							title: "Release 31002",
							body: "- first release inside the historical mixed-activity group",
						},
						{
							...buildReleaseFeedItem("31003"),
							ts: "2026-04-03T21:30:00+08:00",
							title: "Release 31003",
							body: "- second release inside the historical mixed-activity group",
						},
						{
							...buildSocialFeedItem("star-31004", "repo_star_received"),
							ts: "2026-04-03T22:10:00+08:00",
						},
						{
							...buildSocialFeedItem("follow-31005", "follower_received"),
							ts: "2026-04-03T18:45:00+08:00",
						},
					],
					next_cursor: null,
				});
			}

			if (req.method() === "GET" && pathname === "/api/notifications") {
				return json(route, []);
			}

			if (req.method() === "GET" && pathname === "/api/briefs") {
				return json(route, [
					{
						id: "brief-mobile-mixed-2026-04-04",
						date: "2026-04-04",
						window_start: "2026-04-03T08:00:00+08:00",
						window_end: "2026-04-04T08:00:00+08:00",
						effective_time_zone: "Asia/Shanghai",
						effective_local_boundary: "08:00",
						release_count: 2,
						release_ids: ["31002", "31003"],
						content_markdown:
							"## 概览\n\n- 时间窗口（本地）：2026-04-03T08:00:00+08:00 → 2026-04-04T08:00:00+08:00\n- 更新项目：4 个\n- Release：2 条（预发布 0 条）\n- 其余动态：2 条\n",
						created_at: "2026-04-04T08:00:03+08:00",
					},
				]);
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

		await page.goto("/?tab=all");

		const reactionFooter = page
			.locator('[data-reaction-footer="true"]')
			.first();
		const listButton = page.getByRole("button", { name: "列表" }).first();
		const dayHeader = listButton.locator(
			"xpath=ancestor::*[@data-feed-day-header='true'][1]",
		);
		const dayLabel = dayHeader.locator('[data-feed-day-label="true"]').first();
		const actionSlot = dayHeader.locator('[data-feed-day-action-slot="true"]');
		await expect(reactionFooter).toBeVisible();
		await expect(listButton).toBeVisible();
		await expect(dayLabel).toBeVisible();
		await expect(dayLabel).toHaveText("2026-04-04 · 4 条动态");

		const footerBox = await reactionFooter.boundingBox();
		const labelBox = await dayLabel.boundingBox();
		const actionBox = await actionSlot.boundingBox();
		const buttonBox = await listButton.boundingBox();
		expect(footerBox).not.toBeNull();
		expect(labelBox).not.toBeNull();
		expect(actionBox).not.toBeNull();
		expect(buttonBox).not.toBeNull();
		if (!footerBox || !labelBox || !actionBox || !buttonBox) {
			throw new Error("Expected footer/label/action/button geometry");
		}

		expect(
			Math.min(labelBox.y, actionBox.y) - (footerBox.y + footerBox.height),
		).toBeGreaterThanOrEqual(8);
		expect(
			rectsIntersect(
				{
					left: labelBox.x,
					right: labelBox.x + labelBox.width,
					top: labelBox.y,
					bottom: labelBox.y + labelBox.height,
				},
				{
					left: buttonBox.x,
					right: buttonBox.x + buttonBox.width,
					top: buttonBox.y,
					bottom: buttonBox.y + buttonBox.height,
				},
			),
		).toBe(false);
		expect(
			actionBox.x + actionBox.width - (buttonBox.x + buttonBox.width),
		).toBeLessThanOrEqual(4);
	});
});

test("dashboard refreshes cached and fresh feed data across access sync stages", async ({
	page,
}) => {
	let feedCalls = 0;
	let notificationCalls = 0;
	let briefCalls = 0;
	let feedPhase: "initial" | "cached" | "fresh" = "initial";
	let cachedTimer: ReturnType<typeof setTimeout> | null = null;
	let freshTimer: ReturnType<typeof setTimeout> | null = null;
	let phaseTimersStarted = false;

	try {
		await page.addInitScript(
			({ taskId, starDelayMs, completeDelayMs }) => {
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
						if (!this.url.endsWith(`/api/tasks/${taskId}/events`)) return;
						this.timers.push(
							window.setTimeout(() => {
								this.dispatch("task.progress", {
									task_id: taskId,
									stage: "star_refreshed",
									repos: 1,
								});
							}, starDelayMs),
						);
						this.timers.push(
							window.setTimeout(() => {
								this.dispatch("task.completed", {
									task_id: taskId,
									status: "succeeded",
								});
							}, completeDelayMs),
						);
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
			{ taskId: "task-access-1", starDelayMs: 180, completeDelayMs: 3200 },
		);

		await page.route("**/api/**", async (route) => {
			const req = route.request();
			const url = new URL(req.url());
			const { pathname, searchParams } = url;

			if (
				!phaseTimersStarted &&
				req.method() === "GET" &&
				pathname === "/api/me"
			) {
				phaseTimersStarted = true;
				cachedTimer = setTimeout(() => {
					feedPhase = "cached";
				}, 150);
				freshTimer = setTimeout(() => {
					feedPhase = "fresh";
				}, 3000);
			}

			if (req.method() === "GET" && pathname === "/api/me") {
				return json(
					route,
					buildMockMeResponse(
						{
							id: "2f4k7m9p3x6c8v2a",
							github_user_id: 10,
							login: "octo",
							name: "Octo",
							avatar_url: null,
							email: null,
							is_admin: false,
						},
						{
							access_sync: {
								task_id: "task-access-1",
								task_type: "sync.access_refresh",
								event_path: "/api/tasks/task-access-1/events",
								reason: "inactive_over_1h",
							},
						},
					),
				);
			}

			if (req.method() === "GET" && pathname === "/api/feed") {
				feedCalls += 1;
				const stageTitle =
					feedPhase === "initial"
						? null
						: feedPhase === "cached"
							? "Cached release"
							: "Fresh release";
				const items = stageTitle
					? [
							{
								kind: "release",
								ts: `2026-02-22T11:22:3${feedCalls}Z`,
								id: "123",
								repo_full_name: "owner/repo",
								title: stageTitle,
								body: `feed refresh #${feedCalls}`,
								body_truncated: false,
								subtitle: null,
								reason: null,
								subject_type: null,
								html_url: "https://github.com/owner/repo/releases/tag/v1.2.3",
								unread: null,
								translated: null,
								reactions: null,
							},
						]
					: [];
				expect(searchParams.get("limit")).toBe("30");
				return json(route, { items, next_cursor: null });
			}

			if (req.method() === "GET" && pathname === "/api/notifications") {
				notificationCalls += 1;
				return json(route, []);
			}

			if (req.method() === "GET" && pathname === "/api/briefs") {
				briefCalls += 1;
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

		await page.goto("/");

		await expect(page.getByText("Cached release")).toBeVisible();
		await expect(page.getByText("Fresh release")).toHaveCount(0, {
			timeout: 200,
		});
		await expect(page.getByText("Fresh release")).toBeVisible();

		expect(feedCalls).toBeGreaterThanOrEqual(3);
		expect(notificationCalls).toBeGreaterThanOrEqual(3);
		expect(briefCalls).toBeGreaterThanOrEqual(3);
	} finally {
		if (cachedTimer) {
			clearTimeout(cachedTimer);
		}
		if (freshTimer) {
			clearTimeout(freshTimer);
		}
	}
});

test("dashboard keeps inbox sync busy through transient task stream errors", async ({
	page,
}) => {
	let feedCalls = 0;
	let notificationCalls = 0;
	let syncInboxCalls = 0;
	let inboxPhase: "cached" | "fresh" = "cached";
	let freshTimer: ReturnType<typeof setTimeout> | null = null;

	try {
		await page.addInitScript(
			({ taskId, errorDelayMs, completeDelayMs }) => {
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
						if (!this.url.endsWith(`/api/tasks/${taskId}/events`)) return;
						this.timers.push(
							window.setTimeout(() => {
								this.onerror?.call(
									this as unknown as EventSource,
									new Event("error"),
								);
							}, errorDelayMs),
						);
						this.timers.push(
							window.setTimeout(() => {
								this.dispatch("task.completed", {
									task_id: taskId,
									status: "succeeded",
								});
							}, completeDelayMs),
						);
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
			{ taskId: "task-inbox-1", errorDelayMs: 60, completeDelayMs: 2200 },
		);

		await page.route("**/api/**", async (route) => {
			const req = route.request();
			const url = new URL(req.url());
			const { pathname, searchParams } = url;

			if (req.method() === "GET" && pathname === "/api/me") {
				return json(
					route,
					buildMockMeResponse(
						{
							id: "2f4k7m9p3x6c8v2a",
							github_user_id: 10,
							login: "octo",
							name: "Octo",
							avatar_url: null,
							email: null,
							is_admin: false,
						},
						{
							access_sync: {
								task_id: null,
								task_type: null,
								event_path: null,
								reason: "none",
							},
						},
					),
				);
			}

			if (req.method() === "GET" && pathname === "/api/feed") {
				feedCalls += 1;
				expect(searchParams.get("limit")).toBe("30");
				return json(route, {
					items: [
						{
							kind: "release",
							ts: "2026-02-22T11:22:33Z",
							id: "123",
							repo_full_name: "owner/repo",
							title: "Existing release",
							body: "cached feed item",
							body_truncated: false,
							subtitle: null,
							reason: null,
							subject_type: null,
							html_url: "https://github.com/owner/repo/releases/tag/v1.2.3",
							unread: null,
							translated: null,
							reactions: null,
						},
					],
					next_cursor: null,
				});
			}

			if (req.method() === "GET" && pathname === "/api/notifications") {
				notificationCalls += 1;
				return json(route, [
					{
						thread_id: inboxPhase === "fresh" ? "90002" : "90001",
						repo_full_name: "owner/repo",
						subject_title:
							inboxPhase === "fresh"
								? "Fresh inbox thread"
								: "Cached inbox thread",
						subject_type: "PullRequest",
						reason: "review_requested",
						updated_at: "2026-02-22T11:22:33Z",
						unread: inboxPhase === "fresh" ? 1 : 0,
						html_url:
							inboxPhase === "fresh"
								? "https://github.com/owner/repo/pull/77"
								: "https://github.com/owner/repo/pull/42",
					},
				]);
			}

			if (req.method() === "GET" && pathname === "/api/briefs") {
				return json(route, []);
			}

			if (req.method() === "POST" && pathname === "/api/sync/notifications") {
				syncInboxCalls += 1;
				if (!freshTimer) {
					freshTimer = setTimeout(() => {
						inboxPhase = "fresh";
					}, 1500);
				}
				return json(route, {
					mode: "task_id",
					task_id: "task-inbox-1",
					task_type: "sync.notifications",
					status: "queued",
				});
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

		await page.goto("/?tab=inbox");

		const syncInboxButton = page.getByRole("button", { name: "Sync inbox" });
		await expect(syncInboxButton).toBeVisible();
		await page.getByRole("tab", { name: "收件箱" }).click();
		await expect(page.getByText("Cached inbox thread").first()).toBeVisible();

		await syncInboxButton.click();
		await expect(syncInboxButton).toBeDisabled();
		await page.waitForTimeout(120);
		await expect(syncInboxButton).toBeDisabled();

		await expect(page.getByText("Fresh inbox thread").first()).toBeVisible();
		await expect(
			page.getByRole("link", { name: /Fresh inbox thread/i }).first(),
		).toHaveAttribute("href", "https://github.com/owner/repo/pull/77");
		await expect(syncInboxButton).toBeEnabled();

		expect(syncInboxCalls).toBe(1);
		expect(feedCalls).toBeGreaterThanOrEqual(2);
		expect(notificationCalls).toBeGreaterThanOrEqual(2);
	} finally {
		if (freshTimer) {
			clearTimeout(freshTimer);
		}
	}
});

test("dashboard keeps inbox sync reachable when inbox is empty", async ({
	page,
}) => {
	let syncInboxCalls = 0;

	await page.addInitScript(
		({ taskId, completeDelayMs }) => {
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
					if (!this.url.endsWith(`/api/tasks/${taskId}/events`)) return;
					this.timers.push(
						window.setTimeout(() => {
							this.dispatch("task.completed", {
								task_id: taskId,
								status: "succeeded",
							});
						}, completeDelayMs),
					);
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
		{ taskId: "task-inbox-empty-1", completeDelayMs: 120 },
	);

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname, searchParams } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse(
					{
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo",
						name: "Octo",
						avatar_url: null,
						email: null,
						is_admin: false,
					},
					{
						access_sync: {
							task_id: null,
							task_type: null,
							event_path: null,
							reason: "none",
						},
					},
				),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			expect(searchParams.get("limit")).toBe("30");
			return json(route, {
				items: [
					{
						kind: "release",
						ts: "2026-02-22T11:22:33Z",
						id: "123",
						repo_full_name: "owner/repo",
						title: "Existing release",
						body: "cached feed item",
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/owner/repo/releases/tag/v1.2.3",
						unread: null,
						translated: null,
						reactions: null,
					},
				],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (req.method() === "POST" && pathname === "/api/sync/notifications") {
			syncInboxCalls += 1;
			return json(route, {
				mode: "task_id",
				task_id: "task-inbox-empty-1",
				task_type: "sync.notifications",
				status: "queued",
			});
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

	await page.goto("/?tab=inbox");

	const syncInboxButton = page.getByRole("button", { name: "Sync inbox" });
	await expect(syncInboxButton).toBeVisible();
	await expect(
		page.getByText("暂无通知。可以点击 Sync inbox 拉取最新数据。"),
	).toBeVisible();
	await expect(
		page.getByRole("link", { name: "GitHub" }).first(),
	).toHaveAttribute("href", "https://github.com/notifications");

	await syncInboxButton.click();
	await expect(syncInboxButton).toBeDisabled();
	await expect(syncInboxButton).toBeEnabled();
	expect(syncInboxCalls).toBe(1);
});

test("dashboard inbox cards fall back to GitHub per-thread pages when html_url is missing", async ({
	page,
}) => {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname, searchParams } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse(
					{
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo",
						name: "Octo",
						avatar_url: null,
						email: null,
						is_admin: false,
					},
					{
						access_sync: {
							task_id: null,
							task_type: null,
							event_path: null,
							reason: "none",
						},
					},
				),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			expect(searchParams.get("limit")).toBe("30");
			return json(route, { items: [], next_cursor: null });
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, [
				{
					thread_id: "90003",
					repo_full_name: "owner/repo",
					subject_title: "Fallback inbox thread",
					subject_type: "CheckSuite",
					reason: "ci_activity",
					updated_at: "2026-02-22T11:22:33Z",
					unread: 1,
					html_url: null,
				},
			]);
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

	await page.goto("/?tab=inbox");

	await expect(
		page.getByRole("link", { name: /Fallback inbox thread/i }).first(),
	).toHaveAttribute("href", "https://github.com/notifications/threads/90003");
});

test("dashboard inbox cards ignore stale repo-homepage html_url values until repair rewrites them", async ({
	page,
}) => {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname, searchParams } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse(
					{
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo",
						name: "Octo",
						avatar_url: null,
						email: null,
						is_admin: false,
					},
					{
						access_sync: {
							task_id: null,
							task_type: null,
							event_path: null,
							reason: "none",
						},
					},
				),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			expect(searchParams.get("limit")).toBe("30");
			return json(route, { items: [], next_cursor: null });
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, [
				{
					thread_id: "90004",
					repo_full_name: "owner/repo",
					subject_title: "Stale repo homepage link",
					subject_type: "PullRequest",
					reason: "review_requested",
					updated_at: "2026-02-22T11:22:33Z",
					unread: 1,
					html_url: "https://github.com/owner/repo/",
				},
			]);
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

	await page.goto("/?tab=inbox");

	await expect(
		page.getByRole("link", { name: /Stale repo homepage link/i }).first(),
	).toHaveAttribute("href", "https://github.com/notifications/threads/90004");
});

test("dashboard retries the initial sidebar bootstrap after a transient briefs failure", async ({
	page,
}) => {
	let briefsCalls = 0;

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: "brief-retry-user",
					github_user_id: 10,
					login: "octo",
					name: "Octo",
					avatar_url: null,
					email: null,
					is_admin: false,
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
			briefsCalls += 1;
			if (briefsCalls === 1) {
				return json(
					route,
					{
						error: {
							code: "briefs_temporarily_unavailable",
							message: "briefs bootstrap failed once",
						},
					},
					503,
				);
			}

			return json(route, [
				{
					id: "brief-retry-2026-04-09",
					date: "2026-04-09",
					window_start: "2026-04-08T08:00:00+08:00",
					window_end: "2026-04-09T08:00:00+08:00",
					effective_time_zone: "Asia/Shanghai",
					effective_local_boundary: "08:00",
					release_count: 1,
					release_ids: ["retry-brief-release"],
					content_markdown: "## 概览\n\n- retry brief loaded\n",
					created_at: "2026-04-09T08:01:00+08:00",
				},
			]);
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

	await page.goto("/?tab=briefs");

	await expect.poll(() => briefsCalls).toBeGreaterThanOrEqual(2);
	await expect(
		page.getByRole("button", { name: /#2026-04-09/i }),
	).toBeVisible();
});

test("dashboard retries the initial sidebar bootstrap after a transient inbox failure", async ({
	page,
}) => {
	let notificationCalls = 0;

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: "inbox-retry-user",
					github_user_id: 10,
					login: "octo",
					name: "Octo",
					avatar_url: null,
					email: null,
					is_admin: false,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			return json(route, { items: [], next_cursor: null });
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			notificationCalls += 1;
			if (notificationCalls === 1) {
				return json(
					route,
					{
						error: {
							code: "notifications_temporarily_unavailable",
							message: "notifications bootstrap failed once",
						},
					},
					503,
				);
			}

			return json(route, [
				{
					thread_id: "93001",
					repo_full_name: "owner/repo",
					subject_title: "Recovered inbox thread",
					subject_type: "PullRequest",
					reason: "review_requested",
					updated_at: "2026-04-09T08:02:00Z",
					unread: 1,
					html_url: "https://github.com/owner/repo/pull/93001",
				},
			]);
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

	await page.goto("/?tab=inbox");

	await expect.poll(() => notificationCalls).toBeGreaterThanOrEqual(2);
	await expect(page.getByText("Recovered inbox thread").first()).toBeVisible();
});

test("dashboard reaction dialog keeps PAT status unknown when the PAT status fetch fails", async ({
	page,
}) => {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: "reaction-pat-status-user",
					github_user_id: 10,
					login: "octo",
					name: "Octo",
					avatar_url: null,
					email: null,
					is_admin: false,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			return json(route, {
				items: [
					{
						...buildReleaseFeedItem("reaction-500"),
						reactions: buildReactionFooterReady(),
					},
				],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/reaction-token/status") {
			return json(
				route,
				{
					error: {
						code: "pat_status_unavailable",
						message: "GitHub PAT 状态读取失败，请稍后重试或在这里重新校验。",
					},
				},
				503,
			);
		}

		if (req.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "1.2.3" });
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

	await page.goto("/");

	await page.locator('[data-reaction-trigger="plus1"]').first().click();

	const dialog = page.getByRole("dialog", { name: "配置 GitHub PAT" });
	await expect(dialog).toBeVisible();
	await expect(dialog).toContainText(
		"GitHub PAT 状态读取失败，请稍后重试或在这里重新校验。",
	);
	await expect(dialog).toContainText("GitHub PAT 校验失败");
	await expect(dialog).not.toContainText("先补齐 GitHub PAT");
});
