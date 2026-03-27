import { type Route, expect, test } from "@playwright/test";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

test("dashboard refreshes cached and fresh feed data across access sync stages", async ({
	page,
}) => {
	let feedCalls = 0;
	let notificationCalls = 0;
	let briefCalls = 0;
	let feedPhase: "initial" | "cached" | "fresh" = "initial";
	const cachedTimer = setTimeout(() => {
		feedPhase = "cached";
	}, 120);
	const freshTimer = setTimeout(() => {
		feedPhase = "fresh";
	}, 800);

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
			{ taskId: "task-access-1", starDelayMs: 120, completeDelayMs: 800 },
		);

		await page.route("**/api/**", async (route) => {
			const req = route.request();
			const url = new URL(req.url());
			const { pathname, searchParams } = url;

			if (req.method() === "GET" && pathname === "/api/me") {
				return json(route, {
					user: {
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo",
						name: "Octo",
						avatar_url: null,
						email: null,
						is_admin: false,
					},
					access_sync: {
						task_id: "task-access-1",
						task_type: "sync.access_refresh",
						event_path: "/api/tasks/task-access-1/events",
						reason: "inactive_over_1h",
					},
				});
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
								excerpt: `feed refresh #${feedCalls}`,
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
		clearTimeout(cachedTimer);
		clearTimeout(freshTimer);
	}
});
