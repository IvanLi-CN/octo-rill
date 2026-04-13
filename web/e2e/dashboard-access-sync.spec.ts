import { type Route, expect, test } from "@playwright/test";

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

function buildReleaseFeedItem(id: string) {
	return {
		kind: "release",
		ts: "2026-04-09T08:00:00Z",
		id,
		repo_full_name: "owner/repo",
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
	};
}

test("dashboard keeps sync as a single header action for admins", async ({
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
	test.use({ viewport: { width: 390, height: 844 } });

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
					items: Array.from({ length: 10 }, (_, index) =>
						buildReleaseFeedItem(String(20001 + index)),
					),
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
		const laneMenuTrigger = expandedWorkband.locator(
			"[data-dashboard-mobile-lane-menu-trigger]",
		);
		await expect(laneMenuTrigger).toBeVisible();
		await expect(page.locator("[data-dashboard-mobile-rail]")).toHaveCount(0);
		await expect(
			page.locator("[data-dashboard-secondary-controls]").getByRole("link", {
				name: "管理员面板",
			}),
		).toHaveCount(0);

		await expect(
			expandedWorkband.locator(
				"[data-dashboard-mobile-control-band-row='lane']",
			),
		).toHaveCount(0);
		await laneMenuTrigger.click();
		await expect(
			page.locator("[data-dashboard-mobile-lane-menu-popover]"),
		).toBeVisible();
		await expect(
			page
				.locator("[data-dashboard-mobile-lane-menu-popover]")
				.getByRole("menuitemradio", { name: "原文" }),
		).toBeVisible();
		await page
			.locator("[data-dashboard-mobile-lane-menu-popover]")
			.getByRole("menuitemradio", { name: "智能" })
			.click();
		await expect(
			page.locator("[data-dashboard-mobile-lane-menu-popover]"),
		).toHaveCount(0);
		await expandedWorkband.getByRole("tab", { name: "关注" }).click();
		await expect(laneMenuTrigger).toBeVisible();
		await expect(laneMenuTrigger).toBeDisabled();
		await laneMenuTrigger.click({ force: true });
		await expect(
			page.locator("[data-dashboard-mobile-lane-menu-popover]"),
		).toHaveCount(0);
		await expandedWorkband.getByRole("tab", { name: "全部" }).click();
		await expect(laneMenuTrigger).toBeEnabled();

		await page.evaluate(() => window.scrollTo(0, 700));
		await page.waitForTimeout(120);
		await expect(
			page.locator("[data-app-meta-footer-hidden='true']"),
		).toHaveCount(1);
		await expect(
			page.locator("[data-dashboard-header-compact='true']"),
		).toBeVisible();
		await expect(expandedWorkband).toHaveCount(0);
		await expect(page.locator("[data-dashboard-mobile-rail]")).toHaveCount(0);

		await page.evaluate(() => window.scrollTo(0, 280));
		await page.waitForTimeout(120);
		await expect(
			page.locator("[data-dashboard-header-compact='true']"),
		).toHaveCount(0);
		await expect(page.locator("[data-dashboard-mobile-rail]")).toHaveCount(0);
		await expect(expandedWorkband).toBeVisible();

		await page.evaluate(() => window.scrollTo(0, 760));
		await page.waitForTimeout(120);
		await expect(
			page.locator("[data-dashboard-header-compact='true']"),
		).toBeVisible();
		await expect(page.locator("[data-dashboard-mobile-rail]")).toHaveCount(0);
		await expect(expandedWorkband).toHaveCount(0);

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
		await page.getByRole("tab", { name: "Inbox" }).click();
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
