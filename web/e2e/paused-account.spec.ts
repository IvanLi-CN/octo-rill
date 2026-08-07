import { expect, test, type Page, type Route } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

async function installMalformedCompletionEventSource(
	page: Page,
	payload: string,
) {
	await page.addInitScript((completionPayload) => {
		class MockEventSource {
			readyState = 1;
			onerror: ((this: EventSource, event: Event) => unknown) | null = null;
			private listeners = new Map<
				string,
				Set<(event: Event | MessageEvent<string>) => unknown>
			>();
			private timers: number[] = [];

			constructor(url: string | URL) {
				if (!String(url).endsWith("/api/tasks/resume-task/events")) return;
				this.timers.push(
					window.setTimeout(() => {
						if (this.readyState === 2) return;
						const event = new MessageEvent("task.completed", {
							data: completionPayload,
						});
						for (const listener of this.listeners.get("task.completed") ?? []) {
							listener.call(this as unknown as EventSource, event);
						}
					}, 30),
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
				for (const timer of this.timers) window.clearTimeout(timer);
				this.timers = [];
			}
		}

		window.EventSource = MockEventSource as unknown as typeof EventSource;
	}, payload);
}

test("paused session enters account recovery from GET /api/me", async ({
	page,
}) => {
	const apiPaths: string[] = [];

	await page.route("**/api/**", async (route) => {
		const request = route.request();
		const pathname = new URL(request.url()).pathname;
		apiPaths.push(pathname);

		if (request.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse(
					{
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo-member",
						name: "Octo Member",
						avatar_url: null,
						email: "member@example.com",
						is_admin: false,
						account_status: "paused",
						paused_at: "2026-08-06T03:15:00+08:00",
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

		return json(route, { error: { code: "unexpected_api" } }, 404);
	});

	await page.goto("/");

	await expect(page).toHaveURL(/\/account\/paused$/);
	await expect(page.getByRole("heading", { name: "账号已暂停" })).toBeVisible();
	await expect(page.getByRole("button", { name: "恢复账号" })).toBeVisible();
	expect(apiPaths).toContain("/api/me");
	expect(apiPaths).not.toContain("/api/account/status");
	expect(apiPaths).not.toContain("/api/feed");
});

test("paused recovery inherits the app canvas and keeps theme controls available", async ({
	page,
}) => {
	await page.emulateMedia({ colorScheme: "dark" });
	await page.route("**/api/**", async (route) => {
		const request = route.request();
		const pathname = new URL(request.url()).pathname;
		if (request.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: "2f4k7m9p3x6c8v2a",
					github_user_id: 10,
					login: "octo-member",
					name: "Octo Member",
					avatar_url: null,
					email: "member@example.com",
					is_admin: false,
					account_status: "paused",
					paused_at: "2026-08-06T03:15:00+08:00",
				}),
			);
		}
		return json(route, { error: { code: "unexpected_api" } }, 404);
	});

	await page.goto("/");

	await expect(
		page
			.locator("[data-paused-account-panel]")
			.locator("xpath=ancestor::main[1]"),
	).not.toHaveClass(/bg-background/);
	const themeToggle = page.locator("[data-theme-toggle]");
	await expect(themeToggle).toBeVisible();
	await expect(themeToggle.getByRole("button", { name: "浅色" })).toBeVisible();
	await expect(themeToggle.getByRole("button", { name: "深色" })).toBeVisible();
	await expect(
		themeToggle.getByRole("button", { name: "跟随系统" }),
	).toBeVisible();

	await themeToggle.getByRole("button", { name: "浅色" }).click();
	await expect
		.poll(() =>
			page.evaluate(() => ({
				resolvedTheme: document.documentElement.dataset.theme,
				isDark: document.documentElement.classList.contains("dark"),
			})),
		)
		.toEqual({ resolvedTheme: "light", isDark: false });
});

for (const [label, payload, expectedError] of [
	["invalid JSON", "not-json", "访问同步事件无效，请重试。"],
	["null JSON", "null", "访问同步事件无效，请重试。"],
	[
		"object error",
		'{"status":"failed","error":{}}',
		"访问同步未完成，请重试。",
	],
] as const) {
	test(`malformed resume completion event (${label}) becomes a retryable failure`, async ({
		page,
	}) => {
		await installMalformedCompletionEventSource(page, payload);

		await page.route("**/api/**", async (route) => {
			const request = route.request();
			const pathname = new URL(request.url()).pathname;
			if (request.method() === "GET" && pathname === "/api/me") {
				return json(
					route,
					buildMockMeResponse({
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo-member",
						name: "Octo Member",
						avatar_url: null,
						email: "member@example.com",
						is_admin: false,
						account_status: "paused",
						paused_at: "2026-08-06T03:15:00+08:00",
					}),
				);
			}
			if (request.method() === "POST" && pathname === "/api/me/resume") {
				return json(route, {
					status: "enabled",
					access_sync: {
						task_id: "resume-task",
						task_type: "sync.access_refresh",
						event_path: "/api/tasks/resume-task/events",
						reason: "account_resumed",
					},
					sync_enqueue_error: null,
				});
			}
			return json(route, { error: { code: "unexpected_api" } }, 404);
		});

		await page.goto("/");
		await page.getByRole("button", { name: "恢复账号" }).click();

		await expect(page.getByText(expectedError)).toBeVisible();
		await expect(page.getByRole("button", { name: "重试同步" })).toBeVisible();
	});
}
