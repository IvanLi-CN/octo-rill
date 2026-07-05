import { type Route, expect, test } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

const briefSummaries = [
	{
		id: "brief-2026-04-30",
		date: "2026-04-30",
		window_start: "2026-04-29T16:00:00Z",
		window_end: "2026-04-30T16:00:00Z",
		effective_time_zone: "Asia/Shanghai",
		effective_local_boundary: "08:00",
		release_count: 1,
		release_ids: ["42"],
		preview_markdown: "## 预览\n\n首条摘要预览",
		covers_repo_stars: false,
		covers_followers: false,
		created_at: "2026-04-30T16:05:00Z",
		updated_at: "2026-04-30T16:05:00Z",
	},
	{
		id: "brief-2026-04-29",
		date: "2026-04-29",
		window_start: "2026-04-28T16:00:00Z",
		window_end: "2026-04-29T16:00:00Z",
		effective_time_zone: "Asia/Shanghai",
		effective_local_boundary: "08:00",
		release_count: 2,
		release_ids: ["40", "41"],
		preview_markdown: "## 预览\n\n第二条摘要预览",
		covers_repo_stars: true,
		covers_followers: true,
		created_at: "2026-04-29T16:05:00Z",
		updated_at: "2026-04-29T16:05:00Z",
	},
];

const briefDetails = new Map([
	[
		"brief-2026-04-30",
		{
			...briefSummaries[0],
			content_markdown: "## 完整日报\n\nFULL DETAIL A SHOULD LOAD",
		},
	],
	[
		"brief-2026-04-29",
		{
			...briefSummaries[1],
			content_markdown: "## 完整日报\n\nFULL DETAIL B SHOULD LOAD",
		},
	],
]);

test("dashboard loads brief summaries first and fetches selected detail lazily", async ({
	page,
}) => {
	let summaryRequests = 0;
	const detailRequests: string[] = [];

	await page.addInitScript(() => {
		window.localStorage.clear();
	});

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
				items: [],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/dashboard/updates") {
			return json(route, {
				token: "brief-token",
				generated_at: "2026-04-30T10:00:00Z",
				lists: {
					feed: { changed: false, new_count: 0, latest_keys: [] },
					briefs: { changed: false, new_count: 0, latest_keys: [] },
					notifications: { changed: false, new_count: 0, latest_keys: [] },
				},
			});
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			summaryRequests += 1;
			return json(route, briefSummaries);
		}

		if (req.method() === "GET" && pathname.startsWith("/api/briefs/")) {
			const briefId = decodeURIComponent(pathname.replace("/api/briefs/", ""));
			detailRequests.push(briefId);
			const detail = briefDetails.get(briefId);
			return detail
				? json(route, detail)
				: json(route, { error: { code: "not_found" } }, 404);
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

	await page.goto("/briefs");

	await expect(page.getByText("FULL DETAIL A SHOULD LOAD")).toBeVisible({
		timeout: 15_000,
	});
	await expect.poll(() => summaryRequests).toBe(1);
	expect(detailRequests).toEqual(["brief-2026-04-30"]);
	await expect(page.getByText("FULL DETAIL B SHOULD LOAD")).toHaveCount(0);

	await page.getByRole("button", { name: /#2026-04-29/ }).click();
	await expect(page.getByText("FULL DETAIL B SHOULD LOAD")).toBeVisible();
	expect(detailRequests).toEqual(["brief-2026-04-30", "brief-2026-04-29"]);
});
