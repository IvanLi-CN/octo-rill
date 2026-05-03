import { type Route, expect, test } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

test.describe.configure({ mode: "serial" });

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function buildReleaseFeedItem(id: string, title: string) {
	return {
		kind: "release",
		ts: "2026-04-30T08:00:00Z",
		id,
		repo_full_name: "openai/codex",
		repo_visual: null,
		title,
		body: "- refreshed dashboard content",
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `https://github.com/openai/codex/releases/tag/v${id}`,
		unread: null,
		translated: null,
		smart: null,
		reactions: null,
	};
}

test("dashboard live updates retain feed boundaries until they leave the viewport", async ({
	page,
}) => {
	let feedRequests = 0;
	let updateRequests = 0;
	let availableFeedItems = 1;
	let serverStage = 0;
	const oldFeedItems = [
		buildReleaseFeedItem("20001", "Release 20001"),
		buildReleaseFeedItem("19999", "Release 19999"),
		buildReleaseFeedItem("19998", "Release 19998"),
		buildReleaseFeedItem("19997", "Release 19997"),
		buildReleaseFeedItem("19996", "Release 19996"),
	];
	const newFeedItem = buildReleaseFeedItem("20002", "Release 20002");
	const newerFeedItem = buildReleaseFeedItem("20003", "Release 20003");
	const newestFeedItem = buildReleaseFeedItem("20004", "Release 20004");

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
					login: "octo",
					name: "Octo",
					avatar_url: null,
					email: null,
					is_admin: false,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			feedRequests += 1;
			const types = url.searchParams.get("types");
			if (types === "stars") {
				return json(route, {
					items: [],
					next_cursor: null,
				});
			}
			return json(route, {
				items:
					availableFeedItems >= 4
						? [newestFeedItem, newerFeedItem, newFeedItem, ...oldFeedItems]
						: availableFeedItems >= 3
							? [newerFeedItem, newFeedItem, ...oldFeedItems]
							: availableFeedItems >= 2
								? [newFeedItem, ...oldFeedItems]
								: oldFeedItems,
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/dashboard/updates") {
			updateRequests += 1;
			const feedType = url.searchParams.get("feed_type") ?? "all";
			const token = url.searchParams.get("token") ?? "token-stage-0";
			const tokenStage = Number(token.replace("token-stage-", ""));
			const changedKeys =
				feedType === "all" && serverStage > tokenStage
					? serverStage >= 3
						? ["release:20004"]
						: serverStage >= 2
							? ["release:20003"]
							: ["release:20002"]
					: [];
			if (changedKeys.includes("release:20004")) {
				availableFeedItems = 4;
			} else if (changedKeys.includes("release:20003")) {
				availableFeedItems = 3;
			} else if (changedKeys.includes("release:20002")) {
				availableFeedItems = 2;
			}
			const nextToken =
				changedKeys.length > 0 ? `token-stage-${serverStage}` : token;
			return json(route, {
				token: nextToken,
				generated_at: "2026-04-30T10:00:00Z",
				lists: {
					feed: {
						changed: changedKeys.length > 0,
						new_count: changedKeys.length,
						latest_keys:
							changedKeys.length > 0 ? changedKeys : ["release:20001"],
					},
					briefs: {
						changed: false,
						new_count: 0,
						latest_keys: [],
					},
					notifications: {
						changed: false,
						new_count: 0,
						latest_keys: [],
					},
				},
			});
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
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
	await expect(page.getByText("Release 20001")).toBeVisible({
		timeout: 15_000,
	});
	await expect.poll(() => updateRequests).toBe(1);

	const feedRequestsBeforeLiveUpdate = feedRequests;
	serverStage = 1;
	await page.evaluate(() => window.dispatchEvent(new Event("online")));
	await expect(page.getByText("上方有 1 条新动态")).toBeVisible();
	await expect.poll(() => feedRequests).toBe(feedRequestsBeforeLiveUpdate + 1);
	await expect(page.getByText("Release 20002")).toHaveCount(1);
	await expect.poll(() => updateRequests).toBeGreaterThanOrEqual(3);

	serverStage = 2;
	await page.evaluate(() => window.dispatchEvent(new Event("online")));
	await expect(page.getByText("Release 20003")).toBeVisible();
	await expect(page.getByText("上方有 2 条新动态")).toBeVisible();
	await expect(
		page.locator('[data-dashboard-new-content-boundary="true"]'),
	).toHaveCount(1);
	await expect.poll(() => updateRequests).toBeGreaterThanOrEqual(5);

	await page
		.locator('[data-dashboard-new-content-boundary="true"]')
		.evaluate((element) => {
			element.scrollIntoView({ block: "center", behavior: "instant" });
		});
	await page.mouse.wheel(0, -520);
	await expect(
		page.locator('[data-dashboard-new-content-boundary-sealed="true"]'),
	).toHaveCount(1);

	serverStage = 3;
	await page.evaluate(() => window.dispatchEvent(new Event("online")));
	await expect(page.getByText("Release 20004")).toBeVisible();
	await expect(
		page.locator('[data-dashboard-new-content-boundary="true"]'),
	).toHaveCount(2);
	await expect(
		page.locator('[data-dashboard-new-content-boundary-latest="false"]'),
	).toHaveCount(1);
	await expect(
		page
			.locator('[data-feed-item-fresh="true"]')
			.filter({ hasText: "Release 20003" }),
	).toBeVisible();
	await expect(
		page
			.locator('[data-feed-item-fresh="true"]')
			.filter({ hasText: "Release 20002" }),
	).toBeVisible();

	await expect(
		page.locator('[data-dashboard-new-content-boundary-latest="false"]'),
	).toBeVisible();
	await page
		.locator('[data-dashboard-new-content-boundary-latest="false"]')
		.evaluate((element) => {
			element.scrollIntoView({ block: "center", behavior: "instant" });
		});
	await expect(
		page.locator('[data-dashboard-new-content-boundary-latest="false"]'),
	).toBeVisible();
	await page
		.locator('[data-dashboard-new-content-boundary-latest="false"]')
		.evaluate(async () => {
			window.dispatchEvent(new Event("scroll"));
			await new Promise((resolve) => requestAnimationFrame(resolve));
		});
	await expect(
		page.locator('[data-dashboard-new-content-boundary="true"]'),
	).toHaveCount(2);
	await page
		.locator('[data-dashboard-new-content-boundary-latest="false"]')
		.evaluate(async (element) => {
			window.scrollBy({
				top: element.getBoundingClientRect().bottom - 48,
				behavior: "instant",
			});
			window.dispatchEvent(new Event("scroll"));
			await new Promise((resolve) => requestAnimationFrame(resolve));
		});
	await expect(
		page.locator('[data-dashboard-new-content-boundary-latest="false"]'),
	).toBeVisible();
	await expect(
		page.locator('[data-dashboard-new-content-boundary="true"]'),
	).toHaveCount(2);
	await page
		.locator('[data-dashboard-new-content-boundary-latest="false"]')
		.evaluate(async (element) => {
			await new Promise((resolve) => requestAnimationFrame(resolve));
			window.scrollBy({
				top: element.getBoundingClientRect().bottom + 24,
				behavior: "instant",
			});
			window.dispatchEvent(new Event("scroll"));
			await new Promise((resolve) => requestAnimationFrame(resolve));
		});
	await expect(
		page.locator('[data-dashboard-new-content-boundary="true"]'),
	).toHaveCount(1);
});

test("dashboard live updates do not place feed boundaries below processed existing items", async ({
	page,
}) => {
	let feedRequests = 0;
	let updateRequests = 0;
	let processedExistingItem = false;
	const feedItems = [
		buildReleaseFeedItem("20003", "Release 20003"),
		buildReleaseFeedItem("20002", "Release 20002"),
		buildReleaseFeedItem("20001", "Release 20001"),
		buildReleaseFeedItem("19999", "Release 19999"),
	];

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
					login: "octo",
					name: "Octo",
					avatar_url: null,
					email: null,
					is_admin: false,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			feedRequests += 1;
			return json(route, {
				items: feedItems.map((item) =>
					item.id === "20001" && processedExistingItem
						? {
								...item,
								smart: {
									status: "ready",
									title: "Release 20001 polished",
									summary: "Existing item processing completed",
								},
							}
						: item,
				),
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/dashboard/updates") {
			updateRequests += 1;
			const token = url.searchParams.get("token") ?? "token-stage-0";
			const changed = processedExistingItem && token === "token-stage-0";
			return json(route, {
				token: changed ? "token-stage-1" : token,
				generated_at: "2026-04-30T10:00:00Z",
				lists: {
					feed: {
						changed,
						new_count: changed ? 1 : 0,
						latest_keys: changed ? ["release:20001"] : ["release:20003"],
					},
					briefs: {
						changed: false,
						new_count: 0,
						latest_keys: [],
					},
					notifications: {
						changed: false,
						new_count: 0,
						latest_keys: [],
					},
				},
			});
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
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
	await expect(page.getByText("Release 20003")).toBeVisible({
		timeout: 15_000,
	});
	await expect.poll(() => updateRequests).toBe(1);

	const feedRequestsBeforeProcessingUpdate = feedRequests;
	processedExistingItem = true;
	await page.evaluate(() => window.dispatchEvent(new Event("online")));
	await expect
		.poll(() => feedRequests)
		.toBe(feedRequestsBeforeProcessingUpdate + 1);
	await expect(
		page
			.locator('[data-feed-item-fresh="true"]')
			.filter({ hasText: "Release 20001 polished" }),
	).toBeVisible();
	await expect(
		page.locator('[data-dashboard-new-content-boundary="true"]'),
	).toHaveCount(0);
});

test("dashboard live updates re-baselines when inbox polling is enabled later", async ({
	page,
}) => {
	await page.setViewportSize({ width: 390, height: 844 });
	const updateIncludes: string[] = [];
	const notificationUpdateTokens: (string | null)[] = [];

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
				items: [buildReleaseFeedItem("20001", "Release 20001")],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/dashboard/updates") {
			const include = url.searchParams.get("include") ?? "";
			updateIncludes.push(include);
			if (include.includes("notifications")) {
				notificationUpdateTokens.push(url.searchParams.get("token"));
			}
			return json(route, {
				token: `token-${updateIncludes.length}`,
				generated_at: "2026-04-30T10:00:00Z",
				lists: {
					feed: {
						changed: false,
						new_count: 0,
						latest_keys: ["release:20001"],
					},
					briefs: { changed: false, new_count: 0, latest_keys: [] },
					notifications: {
						changed: false,
						new_count: 0,
						latest_keys: ["notification:thread-existing"],
					},
				},
			});
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, [
				{
					thread_id: "thread-existing",
					repo_full_name: "openai/codex",
					subject_title: "Existing thread",
					subject_type: "PullRequest",
					reason: "review_requested",
					updated_at: "2026-04-30T08:00:00Z",
					unread: 1,
					html_url: null,
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
			{ error: { code: "not_found", message: pathname } },
			404,
		);
	});

	await page.goto("/");
	await expect(page.getByText("Release 20001")).toBeVisible({
		timeout: 15_000,
	});
	await expect
		.poll(() =>
			updateIncludes.some((include) => !include.includes("notifications")),
		)
		.toBe(true);

	await page.getByRole("tab", { name: "收件箱" }).click();
	await expect(page.getByText("Existing thread")).toBeVisible();
	await expect.poll(() => notificationUpdateTokens.length).toBeGreaterThan(0);
	await expect(page.getByText("刚刚同步 · 1 条Inbox 内容")).toHaveCount(0);
});
