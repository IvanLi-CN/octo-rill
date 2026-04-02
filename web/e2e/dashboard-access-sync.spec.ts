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
	}, 180);
	const freshTimer = setTimeout(() => {
		feedPhase = "fresh";
	}, 3000);

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
		clearTimeout(cachedTimer);
		clearTimeout(freshTimer);
	}
});

test("dashboard keeps inbox sync busy through transient task stream errors", async ({
	page,
}) => {
	let feedCalls = 0;
	let notificationCalls = 0;
	let syncInboxCalls = 0;
	let inboxPhase: "cached" | "fresh" = "cached";
	const freshTimer = setTimeout(() => {
		inboxPhase = "fresh";
	}, 2500);

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
						task_id: null,
						task_type: null,
						event_path: null,
						reason: "none",
					},
				});
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
						html_url: null,
					},
				]);
			}

			if (req.method() === "GET" && pathname === "/api/briefs") {
				return json(route, []);
			}

			if (req.method() === "POST" && pathname === "/api/sync/notifications") {
				syncInboxCalls += 1;
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
		await expect(syncInboxButton).toBeEnabled();

		expect(syncInboxCalls).toBe(1);
		expect(feedCalls).toBeGreaterThanOrEqual(2);
		expect(notificationCalls).toBeGreaterThanOrEqual(2);
	} finally {
		clearTimeout(freshTimer);
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
					task_id: null,
					task_type: null,
					event_path: null,
					reason: "none",
				},
			});
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

	await syncInboxButton.click();
	await expect(syncInboxButton).toBeDisabled();
	await expect(syncInboxButton).toBeEnabled();
	expect(syncInboxCalls).toBe(1);
});
