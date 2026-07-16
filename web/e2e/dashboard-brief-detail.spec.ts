import { type Page, type Route, expect, test } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

const repoVisual = {
	owner_avatar_url: "https://github.com/octo.png?size=96",
	open_graph_image_url: "https://example.com/preview.png",
	uses_custom_open_graph_image: false,
};

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
			content_markdown:
				"## 完整日报\n\nFULL DETAIL B SHOULD LOAD\n\n- [owner/repo v40](/owner/repo/releases/tag/v40?from=briefs)",
		},
	],
]);

function makeReleaseFeedItem(input: {
	id: string;
	ts: string;
	tag: string;
	title: string;
}) {
	return {
		kind: "release",
		ts: input.ts,
		id: input.id,
		repo_full_name: "owner/repo",
		repo_visual: repoVisual,
		title: input.title,
		body: `- body for ${input.id}`,
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `https://github.com/owner/repo/releases/tag/${input.tag}`,
		unread: null,
		translated: {
			lang: "zh-CN",
			status: "missing",
			title: null,
			summary: null,
		},
		smart: {
			lang: "zh-CN",
			status: "missing",
			title: null,
			summary: null,
		},
		reactions: null,
	};
}

async function installDashboardBriefMocks(
	page: Page,
	options?: {
		feedItems?: unknown[];
		briefDetailFailureIds?: Set<string>;
	},
) {
	let summaryRequests = 0;
	const detailRequests: string[] = [];
	const briefDetailFailures = new Set(options?.briefDetailFailureIds ?? []);

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
				items: options?.feedItems ?? [],
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
			if (briefDetailFailures.has(briefId)) {
				briefDetailFailures.delete(briefId);
				return json(
					route,
					{ error: { code: "brief_detail_failed", message: "detail failed" } },
					500,
				);
			}
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

	return {
		getSummaryRequests: () => summaryRequests,
		getDetailRequests: () => detailRequests.slice(),
	};
}

test("brief deep link canonicalizes to /briefs and list selection uses replace", async ({
	page,
}) => {
	const tracker = await installDashboardBriefMocks(page);

	await page.addInitScript(() => {
		window.localStorage.clear();
	});

	await page.goto("/?tab=briefs&brief=brief-2026-04-29");

	await expect(page).toHaveURL(/\/briefs\?brief=brief-2026-04-29$/);
	await expect(page.getByText("FULL DETAIL B SHOULD LOAD")).toBeVisible({
		timeout: 15_000,
	});
	await expect.poll(tracker.getSummaryRequests).toBe(1);
	expect(tracker.getDetailRequests()).toEqual(["brief-2026-04-29"]);

	const historyLengthBefore = await page.evaluate(() => window.history.length);
	await page.getByRole("button", { name: /#2026-04-30/ }).click();
	await expect(page).toHaveURL(/\/briefs\?brief=brief-2026-04-30$/);
	await expect(page.getByText("FULL DETAIL A SHOULD LOAD")).toBeVisible();
	const historyLengthAfter = await page.evaluate(() => window.history.length);
	expect(historyLengthAfter).toBe(historyLengthBefore);
	expect(tracker.getDetailRequests()).toEqual([
		"brief-2026-04-29",
		"brief-2026-04-30",
	]);
});

test("historical brief cards lazy-load full content, push to /briefs, and copy rich content", async ({
	page,
}) => {
	await page.addInitScript(() => {
		window.localStorage.clear();

		class TestClipboardItem {
			items: Record<string, Blob>;

			constructor(items: Record<string, Blob>) {
				this.items = items;
			}
		}

		const clipboardWrites: Array<Record<string, Blob>> = [];
		const clipboardAttempts: string[][] = [];
		const clipboardWriteTexts: string[] = [];

		Object.defineProperty(window, "ClipboardItem", {
			value: TestClipboardItem,
			configurable: true,
		});

		Object.defineProperty(window, "__clipboardWrites", {
			value: clipboardWrites,
			configurable: true,
			writable: true,
		});
		Object.defineProperty(window, "__clipboardAttempts", {
			value: clipboardAttempts,
			configurable: true,
			writable: true,
		});
		Object.defineProperty(window, "__clipboardWriteTexts", {
			value: clipboardWriteTexts,
			configurable: true,
			writable: true,
		});

		Object.defineProperty(navigator, "clipboard", {
			value: {
				write: async (items: Array<{ items: Record<string, Blob> }>) => {
					const payload = items[0]?.items;
					if (!payload) {
						throw new Error("missing clipboard payload");
					}
					clipboardAttempts.push(Object.keys(payload));
					if ("text/markdown" in payload && clipboardWrites.length === 0) {
						throw new Error("text/markdown unsupported");
					}
					clipboardWrites.push(payload);
				},
				writeText: async (text: string) => {
					clipboardWriteTexts.push(text);
				},
			},
			configurable: true,
		});
	});

	await installDashboardBriefMocks(page, {
		feedItems: [
			makeReleaseFeedItem({
				id: "40",
				ts: "2026-04-29T10:27:01Z",
				tag: "v40",
				title: "Release 40",
			}),
			makeReleaseFeedItem({
				id: "41",
				ts: "2026-04-29T09:12:00Z",
				tag: "v41",
				title: "Release 41",
			}),
		],
	});

	await page.goto("/");

	const historicalGroup = page.locator(
		'[data-feed-group-type="historical"][data-feed-brief-date="2026-04-29"]',
	);
	await expect(
		historicalGroup.getByRole("button", { name: "去日报" }),
	).toBeVisible();
	await expect(
		historicalGroup.getByText("FULL DETAIL B SHOULD LOAD"),
	).toBeVisible({
		timeout: 15_000,
	});

	await historicalGroup.getByRole("button", { name: "复制" }).click();
	await expect(
		page.locator('[data-slot="toast-title"]').filter({ hasText: "日报已复制" }),
	).toBeVisible();

	const clipboardState = await page.evaluate(async () => {
		const writes = (
			window as typeof window & {
				__clipboardWrites: Array<Record<string, Blob>>;
				__clipboardAttempts: string[][];
				__clipboardWriteTexts: string[];
			}
		).__clipboardWrites;
		const attempts = (
			window as typeof window & {
				__clipboardAttempts: string[][];
			}
		).__clipboardAttempts;
		const texts = (
			window as typeof window & {
				__clipboardWriteTexts: string[];
			}
		).__clipboardWriteTexts;
		const finalPayload = writes.at(-1) ?? {};
		const resolved = await Promise.all(
			Object.entries(finalPayload).map(async ([type, blob]) => [
				type,
				await blob.text(),
			]),
		);
		return {
			attempts,
			writeCallCount: writes.length,
			writeTextCallCount: texts.length,
			finalPayload: Object.fromEntries(resolved),
		};
	});

	expect(clipboardState.writeCallCount).toBe(1);
	expect(clipboardState.writeTextCallCount).toBe(0);
	expect(clipboardState.attempts).toEqual([
		["text/html", "text/plain", "text/markdown"],
		["text/html", "text/plain"],
	]);
	expect(clipboardState.finalPayload["text/html"]).toContain(
		"FULL DETAIL B SHOULD LOAD",
	);
	expect(clipboardState.finalPayload["text/plain"]).toContain(
		"FULL DETAIL B SHOULD LOAD",
	);

	await historicalGroup.getByRole("button", { name: "去日报" }).click();
	await expect(page).toHaveURL(/\/briefs\?brief=brief-2026-04-29$/);
	await page.goBack();
	await expect(page).toHaveURL(/\/$/);
	await expect(
		historicalGroup.getByText("FULL DETAIL B SHOULD LOAD"),
	).toBeVisible();
});

test("historical brief detail failure stays inline and can retry", async ({
	page,
}) => {
	await page.addInitScript(() => {
		window.localStorage.clear();
	});

	const tracker = await installDashboardBriefMocks(page, {
		feedItems: [
			makeReleaseFeedItem({
				id: "40",
				ts: "2026-04-29T10:27:01Z",
				tag: "v40",
				title: "Release 40",
			}),
		],
		briefDetailFailureIds: new Set(["brief-2026-04-29"]),
	});

	await page.goto("/");

	const historicalGroup = page.locator(
		'[data-feed-group-type="historical"][data-feed-brief-date="2026-04-29"]',
	);
	await expect(historicalGroup.getByText("日报正文加载失败")).toBeVisible({
		timeout: 15_000,
	});
	await expect(historicalGroup.getByText("第二条摘要预览")).toHaveCount(0);

	await historicalGroup.getByRole("button", { name: "重试" }).click();
	await expect(
		historicalGroup.getByText("FULL DETAIL B SHOULD LOAD"),
	).toBeVisible({
		timeout: 15_000,
	});
	expect(
		tracker
			.getDetailRequests()
			.filter((briefId) => briefId === "brief-2026-04-29"),
	).toHaveLength(2);
	expect(tracker.getDetailRequests()).toContain("brief-2026-04-30");
});
