import { type Route, expect, test } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

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

test("dashboard live updates only refresh feed after the user reveals a new batch", async ({
	page,
}) => {
	let feedRequests = 0;
	let updateRequests = 0;
	const allFeedUpdateTokens: (string | null)[] = [];
	let revealFeed = false;
	let failNextFeedRefresh = false;
	const oldFeedItem = buildReleaseFeedItem("20001", "Release 20001");
	const newFeedItem = buildReleaseFeedItem("20002", "Release 20002");
	const newerFeedItem = buildReleaseFeedItem("20003", "Release 20003");

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
			if (failNextFeedRefresh) {
				failNextFeedRefresh = false;
				return json(
					route,
					{
						error: {
							code: "feed_refresh_failed",
							message: "feed refresh failed",
						},
					},
					500,
				);
			}
			return json(route, {
				items: revealFeed
					? [newerFeedItem, newFeedItem, oldFeedItem]
					: [oldFeedItem],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/dashboard/updates") {
			updateRequests += 1;
			const feedType = url.searchParams.get("feed_type") ?? "all";
			const token = url.searchParams.get("token");
			if (feedType === "all" && updateRequests > 1) {
				allFeedUpdateTokens.push(token);
			}
			const changedKeys =
				feedType === "all" && updateRequests > 1
					? updateRequests >= 3
						? ["release:20003", "release:20002"]
						: ["release:20002"]
					: [];
			return json(route, {
				token: `token-${updateRequests}`,
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
	await expect(page.getByText("Release 20001")).toBeVisible();
	await expect.poll(() => updateRequests).toBe(1);

	const feedRequestsBeforeLiveUpdate = feedRequests;
	await page.evaluate(() => window.dispatchEvent(new Event("online")));
	await expect(page.getByText("有 1 条新动态")).toBeVisible();
	await expect.poll(() => allFeedUpdateTokens.length).toBe(1);
	expect(feedRequests).toBe(feedRequestsBeforeLiveUpdate);
	await expect(page.getByText("Release 20002")).toHaveCount(0);
	await page.evaluate(() => window.dispatchEvent(new Event("online")));
	await expect.poll(() => allFeedUpdateTokens.length).toBe(2);
	expect(allFeedUpdateTokens.at(-1)).toBe("token-1");
	await expect(page.getByText("有 2 条新动态")).toBeVisible();
	await page.getByRole("tab", { name: "加星" }).click();
	await expect(page.getByText("有 2 条新动态")).toHaveCount(0);
	await page.getByRole("tab", { name: "全部" }).click();
	await expect(page.getByText("有 2 条新动态")).toBeVisible();

	revealFeed = true;
	failNextFeedRefresh = true;
	const feedRequestsBeforeFailedReveal = feedRequests;
	await page.getByRole("button", { name: "显示" }).click();
	await expect(page.getByText("有 2 条新动态")).toBeVisible();
	expect(feedRequests).toBe(feedRequestsBeforeFailedReveal + 1);

	const feedRequestsBeforeReveal = feedRequests;
	await page.getByRole("button", { name: "显示" }).click();
	await expect(page.getByText("Release 20003")).toBeVisible();
	await expect(page.getByText("Release 20002")).toBeVisible();
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
	expect(feedRequests).toBe(feedRequestsBeforeReveal + 1);
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
	await expect(page.getByText("Release 20001")).toBeVisible();
	await expect
		.poll(() =>
			updateIncludes.some((include) => !include.includes("notifications")),
		)
		.toBe(true);

	await page.getByRole("tab", { name: "收件箱" }).click();
	await expect(page.getByText("Existing thread")).toBeVisible();
	await expect.poll(() => notificationUpdateTokens.length).toBeGreaterThan(0);
	await expect(page.getByText("有 1 条新Inbox 内容")).toHaveCount(0);
});
