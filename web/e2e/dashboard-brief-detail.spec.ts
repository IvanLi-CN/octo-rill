import { writeFile } from "node:fs/promises";
import {
	type Page,
	type Route,
	type TestInfo,
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
			status: "disabled",
			title: null,
			summary: null,
		},
		reactions: null,
	};
}

function makeReadableBriefSection(input: {
	id: string;
	date: string;
	content: string;
	supplementalItems?: unknown[];
}) {
	return {
		id: `section-${input.id}`,
		kind: "brief",
		display_date: input.date,
		window_start: `${input.date}T00:00:00Z`,
		window_end: `${input.date}T23:59:59Z`,
		activity_count: 1 + (input.supplementalItems?.length ?? 0),
		brief: {
			id: input.id,
			date: input.date,
			window_start: `${input.date}T00:00:00Z`,
			window_end: `${input.date}T23:59:59Z`,
			effective_time_zone: "UTC",
			effective_local_boundary: "00:00",
			release_count: 1,
			covers_repo_stars: false,
			covers_followers: false,
			content_markdown: input.content,
			created_at: `${input.date}T23:59:00Z`,
			updated_at: `${input.date}T23:59:00Z`,
		},
		supplemental_items: input.supplementalItems ?? [],
		supplemental_next_cursor: null,
		items_next_cursor: "section-items-page-2",
	};
}

function makeReadableRawSection(input: {
	id: string;
	date: string;
	items: unknown[];
	nextCursor?: string | null;
}) {
	return {
		id: `section-${input.id}`,
		kind: "raw",
		display_date: input.date,
		window_start: null,
		window_end: null,
		activity_count: input.items.length,
		brief: null,
		items: input.items,
		supplemental_items: [],
		supplemental_next_cursor: null,
		items_next_cursor: input.nextCursor ?? null,
	};
}

function makeScrollableFirstPage(
	release40: ReturnType<typeof makeReleaseFeedItem>,
) {
	return [
		release40,
		...Array.from({ length: 29 }, (_, index) =>
			makeReleaseFeedItem({
				id: `seed-${index + 1}`,
				ts: `2026-04-${String(28 - Math.floor(index / 2)).padStart(2, "0")}T${String(10 - (index % 10)).padStart(2, "0")}:00:00Z`,
				tag: `seed-${index + 1}`,
				title: `Seed release ${index + 1}`,
			}),
		),
	];
}

async function installDashboardBriefMocks(
	page: Page,
	options?: {
		feedItems?: unknown[];
		feedPage?: (cursor: string | null) => {
			items: unknown[];
			nextCursor: string | null;
		};
		readableSectionPage?: (cursor: string | null) => unknown;
		readableSectionPageFailures?: Array<string | null>;
		deferReadableSectionPageCursors?: Array<string | null>;
		readableSectionItemsPage?: (
			sectionId: string,
			cursor: string | null,
		) => unknown;
		deferReadableSectionItems?: string[];
		readableSectionItemsFailures?: string[];
		deferFeedPageCursors?: Array<string | null>;
		feedPageFailureCursors?: Array<string | null>;
		briefDetailFailureIds?: Set<string>;
	},
) {
	let summaryRequests = 0;
	const detailRequests: string[] = [];
	const feedRequests: Array<string | null> = [];
	const readableSectionRequests: Array<string | null> = [];
	const readableSectionItemRequests: Array<{
		sectionId: string;
		cursor: string | null;
	}> = [];
	const feedPageFailures = new Map<string | null, number>();
	const deferredFeedPageCursors = new Set(options?.deferFeedPageCursors ?? []);
	const deferredFeedPageResolvers = new Map<string | null, () => void>();
	const deferredReadableItemResolvers = new Map<string, () => void>();
	const deferredReadablePageResolvers = new Map<string | null, () => void>();
	const deferredReadableItems = new Set(
		options?.deferReadableSectionItems ?? [],
	);
	const deferredReadablePages = new Set(
		options?.deferReadableSectionPageCursors ?? [],
	);
	const readableItemFailures = new Set(
		options?.readableSectionItemsFailures ?? [],
	);
	const readablePageFailures = new Map<string | null, number>();
	for (const cursor of options?.readableSectionPageFailures ?? []) {
		readablePageFailures.set(
			cursor,
			(readablePageFailures.get(cursor) ?? 0) + 1,
		);
	}
	for (const cursor of options?.feedPageFailureCursors ?? []) {
		feedPageFailures.set(cursor, (feedPageFailures.get(cursor) ?? 0) + 1);
	}
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
			const cursor = url.searchParams.get("cursor");
			feedRequests.push(cursor);
			if (deferredFeedPageCursors.has(cursor)) {
				await new Promise<void>((resolve) => {
					deferredFeedPageResolvers.set(cursor, resolve);
				});
				deferredFeedPageCursors.delete(cursor);
			}
			const remainingFailures = feedPageFailures.get(cursor) ?? 0;
			if (remainingFailures > 0) {
				feedPageFailures.set(cursor, remainingFailures - 1);
				return json(
					route,
					{ error: { code: "feed_page_failed", message: "page failed" } },
					500,
				);
			}
			if (options?.feedPage) {
				const page = options.feedPage(cursor);
				return json(route, {
					items: page.items,
					next_cursor: page.nextCursor,
				});
			}
			return json(route, {
				items: options?.feedItems ?? [],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/dashboard/feed") {
			const cursor = url.searchParams.get("cursor");
			readableSectionRequests.push(cursor);
			if (deferredReadablePages.has(cursor)) {
				await new Promise<void>((resolve) => {
					deferredReadablePageResolvers.set(cursor, resolve);
				});
				deferredReadablePages.delete(cursor);
			}
			const remainingReadableFailures = readablePageFailures.get(cursor) ?? 0;
			if (remainingReadableFailures > 0) {
				readablePageFailures.set(cursor, remainingReadableFailures - 1);
				return json(
					route,
					{
						error: {
							code: "readable_page_failed",
							message: "readable page failed",
						},
					},
					500,
				);
			}
			if (!options?.readableSectionPage) {
				const fallbackSections = briefSummaries.map((summary) => ({
					id: `section-${summary.id}`,
					date: summary.date,
					kind: "brief",
					brief: briefDetails.get(summary.id),
					items: [],
					supplemental_items: [],
					item_count: summary.release_count,
					items_next_cursor: null,
				}));
				return json(route, {
					sections: cursor ? [] : fallbackSections,
					next_cursor: null,
				});
			}
			return json(route, options.readableSectionPage(cursor));
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/dashboard/feed/sections/") &&
			pathname.endsWith("/items")
		) {
			const sectionId = decodeURIComponent(pathname.split("/").at(-2) ?? "");
			const cursor = url.searchParams.get("cursor");
			readableSectionItemRequests.push({ sectionId, cursor });
			if (deferredReadableItems.has(sectionId)) {
				await new Promise<void>((resolve) => {
					deferredReadableItemResolvers.set(sectionId, resolve);
				});
			}
			if (readableItemFailures.has(sectionId)) {
				readableItemFailures.delete(sectionId);
				return json(
					route,
					{
						error: {
							code: "section_items_failed",
							message: "section items failed",
						},
					},
					500,
				);
			}
			const payload = options?.readableSectionItemsPage?.(
				sectionId,
				cursor,
			) ?? { items: [], next_cursor: null };
			return json(route, payload);
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

		if (req.method() === "POST" && pathname === "/api/translate/results") {
			const body = req.postDataJSON() as {
				items?: Array<{
					producer_ref?: string;
					entity_id?: string;
					kind?: string;
					variant?: string;
				}>;
			};
			return json(route, {
				items: (body.items ?? []).map((item) => {
					const entityId = item.entity_id ?? "";
					return {
						producer_ref: item.producer_ref ?? "feed-card",
						entity_id: entityId,
						kind: item.kind ?? "release_summary",
						variant: item.variant ?? "feed_card",
						status: "ready",
						title_zh: `Release ${entityId}`,
						summary_md: "Mock translation summary",
						body_md: `Mock translation body for ${entityId}`,
						error: null,
						error_code: null,
						error_summary: null,
						error_detail: null,
						work_item_id: null,
						batch_id: null,
					};
				}),
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
		getFeedRequests: () => feedRequests.slice(),
		getReadableSectionRequests: () => readableSectionRequests.slice(),
		getReadableSectionItemRequests: () => readableSectionItemRequests.slice(),
		getSummaryRequests: () => summaryRequests,
		getDetailRequests: () => detailRequests.slice(),
		releaseFeedPage: (cursor: string | null) => {
			const resolve = deferredFeedPageResolvers.get(cursor);
			if (!resolve) {
				throw new Error(`No deferred feed request for cursor ${cursor}`);
			}
			deferredFeedPageResolvers.delete(cursor);
			resolve();
		},
		releaseReadableSectionItems: (sectionId: string) => {
			const resolve = deferredReadableItemResolvers.get(sectionId);
			if (!resolve)
				throw new Error(`No deferred readable item request for ${sectionId}`);
			deferredReadableItemResolvers.delete(sectionId);
			deferredReadableItems.delete(sectionId);
			resolve();
		},
		releaseReadableSectionPage: (cursor: string | null) => {
			const resolve = deferredReadablePageResolvers.get(cursor);
			if (!resolve)
				throw new Error(
					`No deferred readable page request for cursor ${cursor}`,
				);
			deferredReadablePageResolvers.delete(cursor);
			deferredReadablePages.delete(cursor);
			resolve();
		},
	};
}

async function capturePaginationEvidence(
	page: Page,
	testInfo: TestInfo,
	name: string,
	requests: Array<string | null>,
) {
	const imagePath = testInfo.outputPath(`${name}.png`);
	await page.screenshot({ path: imagePath });
	await testInfo.attach(name, {
		path: imagePath,
		contentType: "image/png",
	});
	const requestsPath = testInfo.outputPath(`${name}-requests.json`);
	await writeFile(requestsPath, JSON.stringify({ requests }, null, 2));
	await testInfo.attach(`${name}-requests`, {
		path: requestsPath,
		contentType: "application/json",
	});
}

async function scrollPaginationSentinelIntoView(page: Page) {
	await page
		.locator("[data-feed-pagination-sentinel]")
		.scrollIntoViewIfNeeded();
}

test("dashboard advances to the next readable section without a continuation stop", async ({
	page,
}) => {
	await page.addInitScript(() => {
		window.localStorage.clear();
	});

	const supplemental = makeReleaseFeedItem({
		id: "supplemental-1",
		ts: "2026-04-29T10:27:01Z",
		tag: "supplemental-1",
		title: "Uncovered supplemental release",
	});
	const tracker = await installDashboardBriefMocks(page, {
		readableSectionPage: (cursor) =>
			cursor === null
				? {
						sections: [
							makeReadableBriefSection({
								id: "brief-readable-1",
								date: "2026-04-30",
								content: "## 完整日报\n\nREADABLE BRIEF ONE",
								supplementalItems: [supplemental],
							}),
						],
						next_cursor: "readable-section-page-2",
					}
				: {
						sections: [
							makeReadableBriefSection({
								id: "brief-readable-2",
								date: "2026-04-29",
								content: "## 完整日报\n\nREADABLE BRIEF TWO",
							}),
						],
						next_cursor: null,
					},
	});

	await page.goto("/");
	await expect(page.getByText("READABLE BRIEF ONE")).toBeVisible({
		timeout: 15_000,
	});
	await expect(page.getByText("Uncovered supplemental release")).toBeVisible();
	await expect.poll(tracker.getReadableSectionRequests).toContain(null);
	await expect(tracker.getFeedRequests()).toHaveLength(0);

	await scrollPaginationSentinelIntoView(page);
	await expect(page.getByText("READABLE BRIEF TWO")).toBeVisible({
		timeout: 15_000,
	});
	await expect
		.poll(tracker.getReadableSectionRequests)
		.toContain("readable-section-page-2");
	await expect(
		page.getByRole("button", { name: "继续加载历史动态" }),
	).toHaveCount(0);
});

test("dashboard fetches covered raw releases only after switching a brief to list", async ({
	page,
}) => {
	await page.addInitScript(() => window.localStorage.clear());
	const covered = makeReleaseFeedItem({
		id: "covered-release",
		ts: "2026-04-30T12:00:00Z",
		tag: "covered",
		title: "Covered release appears in list",
	});
	const tracker = await installDashboardBriefMocks(page, {
		readableSectionPage: () => ({
			sections: [
				{
					id: "section-brief-list",
					date: "2026-04-30",
					kind: "brief",
					brief: {
						id: "brief-list",
						date: "2026-04-30",
						window_start: "2026-04-30T00:00:00Z",
						window_end: "2026-04-30T23:59:59Z",
						release_count: 1,
						release_ids: ["covered-release"],
						content_markdown: "## 完整日报\n\n日报正文没有被截断。",
						covers_repo_stars: false,
						covers_followers: false,
						preview_markdown: "日报摘要不应显示",
						effective_time_zone: "UTC",
						effective_local_boundary: "00:00",
						created_at: "2026-04-30T23:00:00Z",
						updated_at: "2026-04-30T23:00:00Z",
					},
					items: [],
					supplemental_items: [],
					item_count: 1,
					items_next_cursor: null,
				},
			],
			next_cursor: null,
		}),
		readableSectionItemsPage: () => ({ items: [covered], next_cursor: null }),
	});

	await page.goto("/");
	await expect(page.getByText("日报正文没有被截断。")).toBeVisible();
	await expect(page.getByText("Covered release appears in list")).toHaveCount(
		0,
	);
	await page.getByRole("button", { name: "列表" }).click();
	await expect(page.getByText("Covered release appears in list")).toBeVisible();
	await expect
		.poll(tracker.getReadableSectionItemRequests)
		.toEqual([{ sectionId: "section-brief-list", cursor: null }]);
	await expect(tracker.getFeedRequests()).toHaveLength(0);
});

test("dashboard keeps a history date without a brief readable and manual", async ({
	page,
}) => {
	await page.addInitScript(() => window.localStorage.clear());
	const raw = makeReleaseFeedItem({
		id: "raw-history",
		ts: "2026-04-28T10:00:00Z",
		tag: "raw-history",
		title: "没有日报的原始发布",
	});
	const rawContinuation = makeReleaseFeedItem({
		id: "raw-history-continuation",
		ts: "2026-04-28T09:00:00Z",
		tag: "raw-history-continuation",
		title: "没有日报区块的明细续页",
	});
	const tracker = await installDashboardBriefMocks(page, {
		readableSectionPage: () => ({
			sections: [
				makeReadableRawSection({
					id: "raw-history",
					date: "2026-04-28",
					items: [raw],
					nextCursor: "raw-history-page-2",
				}),
			],
			next_cursor: null,
		}),
		readableSectionItemsPage: (_sectionId, cursor) =>
			cursor === "raw-history-page-2"
				? { items: [rawContinuation], next_cursor: null }
				: { items: [], next_cursor: null },
	});

	await page.goto("/");
	await expect(page.getByText("没有日报的原始发布")).toBeVisible();
	await expect(page.getByText("没有日报区块的明细续页")).toBeVisible();
	await expect(page.getByRole("button", { name: "生成日报" })).toBeVisible();
	await expect.poll(tracker.getReadableSectionItemRequests).toContainEqual({
		sectionId: "section-raw-history",
		cursor: "raw-history-page-2",
	});
	await expect(tracker.getFeedRequests()).toHaveLength(0);
});

test("dashboard paginates list details independently from the readable stream", async ({
	page,
}) => {
	await page.addInitScript(() => window.localStorage.clear());
	const first = makeReleaseFeedItem({
		id: "detail-first",
		tag: "detail-first",
		title: "首批列表发布",
	});
	const second = makeReleaseFeedItem({
		id: "detail-second",
		tag: "detail-second",
		title: "区块明细续页发布",
	});
	const tracker = await installDashboardBriefMocks(page, {
		readableSectionPage: () => ({
			sections: [
				makeReadableBriefSection({
					id: "brief-detail-pagination",
					date: "2026-04-30",
					content: "## 日报优先区块",
				}),
			],
			next_cursor: null,
		}),
		readableSectionItemsPage: (_sectionId, cursor) =>
			cursor === null
				? { items: [first], next_cursor: "detail-page-2" }
				: { items: [first, second], next_cursor: null },
	});

	await page.goto("/");
	await page.getByRole("button", { name: "列表" }).click();
	await expect(page.getByText("首批列表发布")).toBeVisible();
	await expect(page.getByText("区块明细续页发布")).toBeVisible();
	await expect(page.getByText("首批列表发布")).toHaveCount(1);
	await expect.poll(tracker.getReadableSectionItemRequests).toContainEqual({
		sectionId: "section-brief-detail-pagination",
		cursor: "detail-page-2",
	});
	await expect
		.poll(() =>
			tracker.getReadableSectionRequests().filter((cursor) => cursor !== null),
		)
		.toHaveLength(0);
});

test("dashboard shows the centered wave capsule while section details load", async ({
	page,
}) => {
	await page.addInitScript(() => window.localStorage.clear());
	const tracker = await installDashboardBriefMocks(page, {
		readableSectionPage: () => ({
			sections: [
				makeReadableBriefSection({
					id: "brief-loading",
					date: "2026-04-30",
					content: "## 完整日报\n\n等待列表明细",
				}),
			],
			next_cursor: null,
		}),
		deferReadableSectionItems: ["section-brief-loading"],
		readableSectionItemsPage: () => ({ items: [], next_cursor: null }),
	});

	await page.goto("/");
	await page.getByRole("button", { name: "列表" }).click();
	const loading = page.locator("[data-readable-pagination-loading='true']");
	await expect(loading).toBeVisible();
	await expect(
		loading.locator("[data-feed-pagination-wave-dot='true']"),
	).toHaveCount(3);
	await tracker.releaseReadableSectionItems("section-brief-loading");
	await expect(loading).toHaveCount(0);
});

test("dashboard shows the centered wave capsule while a readable page loads", async ({
	page,
}) => {
	await page.addInitScript(() => window.localStorage.clear());
	const tracker = await installDashboardBriefMocks(page, {
		deferReadableSectionPageCursors: ["readable-page-2"],
		readableSectionPage: (cursor) =>
			cursor === null
				? {
						sections: [
							makeReadableBriefSection({
								id: "brief-page-1",
								date: "2026-04-30",
								content: "## 第一页日报",
							}),
						],
						next_cursor: "readable-page-2",
					}
				: { sections: [], next_cursor: null },
	});

	await page.goto("/");
	await expect(page.getByText("第一页日报")).toBeVisible();
	await scrollPaginationSentinelIntoView(page);
	const loading = page.locator("[data-readable-pagination-loading='true']");
	await expect(loading).toBeVisible();
	await expect(
		loading.locator("[data-feed-pagination-wave-dot='true']"),
	).toHaveCount(3);
	tracker.releaseReadableSectionPage("readable-page-2");
	await expect(loading).toHaveCount(0);
});

test("dashboard retries a failed readable page in place", async ({ page }) => {
	await page.addInitScript(() => window.localStorage.clear());
	const tracker = await installDashboardBriefMocks(page, {
		readableSectionPageFailures: ["readable-page-2"],
		readableSectionPage: (cursor) =>
			cursor === null
				? {
						sections: [
							makeReadableBriefSection({
								id: "brief-error-1",
								date: "2026-04-30",
								content: "## 第一页日报",
							}),
						],
						next_cursor: "readable-page-2",
					}
				: {
						sections: [
							makeReadableBriefSection({
								id: "brief-error-2",
								date: "2026-04-29",
								content: "## 重试后日报",
							}),
						],
						next_cursor: null,
					},
	});

	await page.goto("/");
	await expect(page.getByRole("button", { name: "重试加载" })).toBeVisible({
		timeout: 15_000,
	});
	await expect
		.poll(tracker.getReadableSectionRequests)
		.toContain("readable-page-2");
	await page.getByRole("button", { name: "重试加载" }).click();
	await expect(page.getByText("重试后日报")).toBeVisible();
	await expect
		.poll(() =>
			tracker
				.getReadableSectionRequests()
				.filter((cursor) => cursor === "readable-page-2"),
		)
		.toHaveLength(2);
});

test("dashboard stops after a readable cursor repeats", async ({ page }) => {
	await page.addInitScript(() => window.localStorage.clear());
	const tracker = await installDashboardBriefMocks(page, {
		readableSectionPage: (cursor) =>
			cursor === null
				? {
						sections: [
							makeReadableBriefSection({
								id: "repeat-cursor-first",
								date: "2026-04-30",
								content: "## 首个可读区块",
							}),
						],
						next_cursor: "repeat-cursor",
					}
				: {
						sections: [
							makeReadableBriefSection({
								id: "repeat-cursor-second",
								date: "2026-04-29",
								content: "## 重复 cursor 后仍只交付一次",
							}),
						],
						next_cursor: "repeat-cursor",
					},
	});

	await page.goto("/");
	await expect(page.getByText("首个可读区块")).toBeVisible();
	await scrollPaginationSentinelIntoView(page);
	await expect(page.getByText("重复 cursor 后仍只交付一次")).toBeVisible();
	await expect
		.poll(() =>
			tracker
				.getReadableSectionRequests()
				.filter((cursor) => cursor === "repeat-cursor"),
		)
		.toHaveLength(1);
	await expect(
		page.locator("[data-readable-pagination-end='true']"),
	).toBeVisible();
});

test.skip("legacy: dashboard pauses pagination when an appended page is fully folded into a brief", async ({
	page,
}, testInfo) => {
	await page.addInitScript(() => {
		window.localStorage.clear();
	});

	const release40 = makeReleaseFeedItem({
		id: "40",
		ts: "2026-04-29T10:27:01Z",
		tag: "v40",
		title: "Release 40",
	});
	const release41 = makeReleaseFeedItem({
		id: "41",
		ts: "2026-04-29T09:12:00Z",
		tag: "v41",
		title: "Release 41",
	});
	const release43 = makeReleaseFeedItem({
		id: "43",
		ts: "2026-04-13T09:12:00Z",
		tag: "v43",
		title: "Release 43",
	});
	const firstPage = makeScrollableFirstPage(release40);

	const tracker = await installDashboardBriefMocks(page, {
		feedPage: (cursor) => {
			if (cursor === null) {
				return { items: firstPage, nextCursor: "cursor-page-2" };
			}
			if (cursor === "cursor-page-2") {
				return { items: [release41], nextCursor: "cursor-page-3" };
			}
			return { items: [release43], nextCursor: null };
		},
	});

	await page.goto("/");

	await expect(
		page.locator(
			'[data-feed-group-type="historical"][data-feed-brief-date="2026-04-29"]',
		),
	).toBeVisible({ timeout: 15_000 });
	await expect.poll(tracker.getFeedRequests).toEqual([null]);
	await scrollPaginationSentinelIntoView(page);
	await expect.poll(() => tracker.getFeedRequests()).toContain("cursor-page-2");
	await expect(
		page.getByRole("button", { name: "继续加载历史动态" }),
	).toBeVisible({ timeout: 15_000 });
	await expect.poll(tracker.getFeedRequests).toEqual([null, "cursor-page-2"]);
	await page
		.getByRole("button", { name: "继续加载历史动态" })
		.evaluate((element) => element.scrollIntoView({ block: "center" }));
	await capturePaginationEvidence(
		page,
		testInfo,
		"folded-history-paused",
		tracker.getFeedRequests(),
	);

	await page.getByRole("button", { name: "继续加载历史动态" }).click();
	await expect(page.locator('[data-feed-item-key="release:43"]')).toBeVisible();
	await expect
		.poll(tracker.getFeedRequests)
		.toEqual([null, "cursor-page-2", "cursor-page-3"]);
	await expect(
		page.getByRole("button", { name: "继续加载历史动态" }),
	).toHaveCount(0);
	await page
		.locator('[data-feed-item-key="release:43"]')
		.scrollIntoViewIfNeeded();
	await capturePaginationEvidence(
		page,
		testInfo,
		"folded-history-resumed",
		tracker.getFeedRequests(),
	);
});

test.skip("legacy: dashboard keeps paginating when an appended page remains visible at the sentinel", async ({
	page,
}, testInfo) => {
	await page.addInitScript(() => {
		window.localStorage.clear();
	});

	const firstPage = Array.from({ length: 30 }, (_, index) =>
		makeReleaseFeedItem({
			id: String(index + 1),
			ts: `2026-04-${String(30 - Math.floor(index / 5)).padStart(2, "0")}T${String(10 - (index % 10)).padStart(2, "0")}:00:00Z`,
			tag: `v${index + 1}`,
			title: `Release ${index + 1}`,
		}),
	);
	const release31 = makeReleaseFeedItem({
		id: "31",
		ts: "2026-04-24T09:12:00Z",
		tag: "v31",
		title: "Release 31",
	});
	const release32 = makeReleaseFeedItem({
		id: "32",
		ts: "2026-04-24T08:12:00Z",
		tag: "v32",
		title: "Release 32",
	});

	const tracker = await installDashboardBriefMocks(page, {
		feedPage: (cursor) => {
			if (cursor === null) {
				return { items: firstPage, nextCursor: "cursor-page-2" };
			}
			if (cursor === "cursor-page-2") {
				return { items: [release31], nextCursor: "cursor-page-3" };
			}
			return { items: [release32], nextCursor: null };
		},
	});

	await page.goto("/");
	await expect(page.locator('[data-feed-item-key="release:30"]')).toBeVisible({
		timeout: 15_000,
	});
	await expect.poll(tracker.getFeedRequests).toEqual([null]);

	await scrollPaginationSentinelIntoView(page);
	await expect(page.locator('[data-feed-item-key="release:32"]')).toBeVisible({
		timeout: 15_000,
	});
	await expect
		.poll(tracker.getFeedRequests)
		.toEqual([null, "cursor-page-2", "cursor-page-3"]);
	await expect(page.getByText("已到尽头（共 32 条）")).toBeVisible();
	await page
		.locator('[data-feed-item-key="release:32"]')
		.scrollIntoViewIfNeeded();
	await capturePaginationEvidence(
		page,
		testInfo,
		"visible-sentinel-auto-pagination",
		tracker.getFeedRequests(),
	);
});

test.skip("legacy: dashboard shows pagination loading while the next page is in flight", async ({
	page,
}, testInfo) => {
	await page.addInitScript(() => {
		window.localStorage.clear();
	});

	const firstPage = Array.from({ length: 30 }, (_, index) =>
		makeReleaseFeedItem({
			id: String(index + 1),
			ts: `2026-04-${String(30 - Math.floor(index / 5)).padStart(2, "0")}T${String(10 - (index % 10)).padStart(2, "0")}:00:00Z`,
			tag: `v${index + 1}`,
			title: `Release ${index + 1}`,
		}),
	);
	const release31 = makeReleaseFeedItem({
		id: "31",
		ts: "2026-04-24T09:12:00Z",
		tag: "v31",
		title: "Release 31",
	});

	const tracker = await installDashboardBriefMocks(page, {
		deferFeedPageCursors: ["cursor-page-2"],
		feedPage: (cursor) =>
			cursor === null
				? { items: firstPage, nextCursor: "cursor-page-2" }
				: { items: [release31], nextCursor: null },
	});

	await page.goto("/");
	await expect(page.locator('[data-feed-item-key="release:30"]')).toBeVisible({
		timeout: 15_000,
	});
	await expect.poll(tracker.getFeedRequests).toEqual([null]);

	await scrollPaginationSentinelIntoView(page);
	await expect.poll(tracker.getFeedRequests).toEqual([null, "cursor-page-2"]);
	const loadingStatus = page.locator("[data-feed-pagination-loading='true']");
	await expect(loadingStatus).toHaveAttribute("role", "status");
	await expect(loadingStatus).toHaveAttribute("aria-label", "加载中");
	await expect(loadingStatus).not.toContainText("加载中");
	const loadingDots = loadingStatus.locator(
		"[data-feed-pagination-wave-dot='true']",
	);
	await expect(loadingDots).toHaveCount(3);
	await expect(loadingDots.first()).toHaveCSS(
		"animation-name",
		"feed-pagination-wave",
	);
	const loadingTooltip = page.getByRole("tooltip");
	await expect(loadingTooltip).toHaveCount(0);
	await loadingStatus.hover();
	await page.waitForTimeout(300);
	await expect(loadingTooltip).toHaveCount(0);
	await expect(loadingTooltip).toHaveText("加载中");
	await loadingStatus.scrollIntoViewIfNeeded();
	await capturePaginationEvidence(
		page,
		testInfo,
		"pagination-loading",
		tracker.getFeedRequests(),
	);

	tracker.releaseFeedPage("cursor-page-2");
	await expect(page.locator('[data-feed-item-key="release:31"]')).toBeVisible();
	await expect(loadingStatus).toHaveCount(0);
	await expect.poll(tracker.getFeedRequests).toEqual([null, "cursor-page-2"]);
});

test.skip("legacy: dashboard resumes pagination when folded history switches to list view", async ({
	page,
}, testInfo) => {
	await page.addInitScript(() => {
		window.localStorage.clear();
	});

	const release40 = makeReleaseFeedItem({
		id: "40",
		ts: "2026-04-29T10:27:01Z",
		tag: "v40",
		title: "Release 40",
	});
	const release41 = makeReleaseFeedItem({
		id: "41",
		ts: "2026-04-29T09:12:00Z",
		tag: "v41",
		title: "Release 41",
	});
	const release43 = makeReleaseFeedItem({
		id: "43",
		ts: "2026-04-13T09:12:00Z",
		tag: "v43",
		title: "Release 43",
	});
	const firstPage = makeScrollableFirstPage(release40);

	const tracker = await installDashboardBriefMocks(page, {
		feedPage: (cursor) => {
			if (cursor === null) {
				return { items: firstPage, nextCursor: "cursor-page-2" };
			}
			if (cursor === "cursor-page-2") {
				return { items: [release41], nextCursor: "cursor-page-3" };
			}
			return { items: [release43], nextCursor: null };
		},
	});

	await page.goto("/");

	const historicalGroup = page.locator(
		'[data-feed-group-type="historical"][data-feed-brief-date="2026-04-29"]',
	);
	await expect(historicalGroup).toBeVisible({ timeout: 15_000 });
	await expect.poll(tracker.getFeedRequests).toEqual([null]);
	await scrollPaginationSentinelIntoView(page);
	await expect.poll(() => tracker.getFeedRequests()).toContain("cursor-page-2");
	await expect(
		page.getByRole("button", { name: "继续加载历史动态" }),
	).toBeVisible({ timeout: 15_000 });

	await historicalGroup.getByRole("button", { name: "列表" }).click();
	await expect(
		historicalGroup.locator('[data-feed-item-key="release:40"]'),
	).toBeVisible();
	await expect(
		historicalGroup.locator('[data-feed-item-key="release:41"]'),
	).toBeVisible();
	await expect(
		historicalGroup.getByRole("button", { name: "日报" }),
	).toBeVisible();
	await expect(
		page.getByRole("button", { name: "继续加载历史动态" }),
	).toHaveCount(0);
	await scrollPaginationSentinelIntoView(page);
	await expect(page.locator('[data-feed-item-key="release:43"]')).toBeVisible();
	await expect
		.poll(tracker.getFeedRequests)
		.toEqual([null, "cursor-page-2", "cursor-page-3"]);
	await historicalGroup.scrollIntoViewIfNeeded();
	await capturePaginationEvidence(
		page,
		testInfo,
		"folded-history-list-view",
		tracker.getFeedRequests(),
	);
});

test.skip("legacy: dashboard retries an appended page before entering explicit continuation", async ({
	page,
}) => {
	await page.addInitScript(() => {
		window.localStorage.clear();
	});

	const release40 = makeReleaseFeedItem({
		id: "40",
		ts: "2026-04-29T10:27:01Z",
		tag: "v40",
		title: "Release 40",
	});
	const release41 = makeReleaseFeedItem({
		id: "41",
		ts: "2026-04-29T09:12:00Z",
		tag: "v41",
		title: "Release 41",
	});
	const release43 = makeReleaseFeedItem({
		id: "43",
		ts: "2026-04-13T09:12:00Z",
		tag: "v43",
		title: "Release 43",
	});
	const firstPage = makeScrollableFirstPage(release40);

	const tracker = await installDashboardBriefMocks(page, {
		feedPageFailureCursors: ["cursor-page-2", "cursor-page-2"],
		feedPage: (cursor) => {
			if (cursor === null) {
				return { items: firstPage, nextCursor: "cursor-page-2" };
			}
			if (cursor === "cursor-page-2") {
				return { items: [release41], nextCursor: "cursor-page-3" };
			}
			return { items: [release43], nextCursor: null };
		},
	});

	await page.goto("/");
	await expect.poll(tracker.getFeedRequests).toEqual([null]);
	await scrollPaginationSentinelIntoView(page);
	await expect(page.getByRole("button", { name: "继续加载" })).toBeVisible({
		timeout: 15_000,
	});
	await expect
		.poll(tracker.getFeedRequests)
		.toEqual([null, "cursor-page-2", "cursor-page-2"]);

	await page.getByRole("button", { name: "继续加载" }).click();
	await expect(
		page.getByRole("button", { name: "继续加载历史动态" }),
	).toBeVisible({ timeout: 15_000 });
	await expect
		.poll(tracker.getFeedRequests)
		.toEqual([null, "cursor-page-2", "cursor-page-2", "cursor-page-2"]);

	await page.getByRole("button", { name: "继续加载历史动态" }).click();
	await expect(page.locator('[data-feed-item-key="release:43"]')).toBeVisible();
	await expect
		.poll(tracker.getFeedRequests)
		.toEqual([
			null,
			"cursor-page-2",
			"cursor-page-2",
			"cursor-page-2",
			"cursor-page-3",
		]);
});

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

test.skip("legacy: historical brief detail failure stays inline and can retry", async ({
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
