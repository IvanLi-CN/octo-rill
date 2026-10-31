import { expect, test } from "@playwright/test";

import {
	applySmartIfSourceMatches,
	applyTranslationIfSourceMatches,
	buildSmartSourceKey,
	buildTranslationSourceKey,
} from "../src/feed/autoLaneRequests";
import type { FeedItem } from "../src/feed/types";
import {
	buildMockMeResponse,
	json,
	mockUnavailableDashboardReadableFeed,
} from "./mockApi";

test.describe.configure({ mode: "serial" });

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

test("late feed projections are rejected after same-key source changes", () => {
	const original = {
		...buildReleaseFeedItem("source-key", "Same release"),
		body: "Release body revision one",
	} as FeedItem;
	const refreshed = {
		...original,
		body: "Release body revision two",
	} as FeedItem;
	const staleTranslation = {
		lang: "zh-CN",
		status: "ready",
		title: "Stale translation",
		summary: "Stale translation body",
	} as const;
	const staleSmart = {
		lang: "zh-CN",
		status: "ready",
		title: "Stale polish",
		summary: "Stale polish body",
	} as const;

	expect(
		applyTranslationIfSourceMatches(
			refreshed,
			buildTranslationSourceKey(original),
			staleTranslation,
		),
	).toBe(refreshed);
	expect(
		applySmartIfSourceMatches(
			refreshed,
			buildSmartSourceKey(original),
			staleSmart,
		),
	).toBe(refreshed);
});

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

		if (req.method() === "GET" && pathname === "/api/dashboard/feed") {
			return mockUnavailableDashboardReadableFeed(route);
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

test("dashboard live updates retire pending projections when a feed source changes", async ({
	page,
}) => {
	const originalSource = "Release body revision one";
	const updatedSource = "Release body revision two";
	const staleTranslation = "Stale translation for revision one";
	const freshTranslation = "Fresh translation for revision two";
	const staleSmart = "Stale polish for revision one";
	const freshSmart = "Fresh polish for revision two";
	let sourceRevision = 1;
	let feedRequests = 0;
	let updateRequests = 0;
	let translationRequests = 0;
	let smartRequests = 0;
	let releaseOldTranslation: (() => void) | null = null;
	let releaseOldSmart: (() => void) | null = null;
	let markOldTranslationStarted = () => {};
	let markOldSmartStarted = () => {};
	let markOldTranslationPollStarted = () => {};
	let markOldSmartPollStarted = () => {};
	let markOldTranslationFinished = () => {};
	let markOldSmartFinished = () => {};
	const oldTranslationGate = new Promise<void>((resolve) => {
		releaseOldTranslation = resolve;
	});
	const oldSmartGate = new Promise<void>((resolve) => {
		releaseOldSmart = resolve;
	});
	const oldTranslationStarted = new Promise<void>((resolve) => {
		markOldTranslationStarted = resolve;
	});
	const oldSmartStarted = new Promise<void>((resolve) => {
		markOldSmartStarted = resolve;
	});
	const oldTranslationPollStarted = new Promise<void>((resolve) => {
		markOldTranslationPollStarted = resolve;
	});
	const oldSmartPollStarted = new Promise<void>((resolve) => {
		markOldSmartPollStarted = resolve;
	});
	const oldTranslationFinished = new Promise<void>((resolve) => {
		markOldTranslationFinished = resolve;
	});
	const oldSmartFinished = new Promise<void>((resolve) => {
		markOldSmartFinished = resolve;
	});
	const submittedSources: Array<{ kind: string; body: string | null }> = [];

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

		if (req.method() === "GET" && pathname === "/api/dashboard/feed") {
			return mockUnavailableDashboardReadableFeed(route);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			feedRequests += 1;
			return json(route, {
				items: [
					{
						...buildReleaseFeedItem("20001", "Release 20001"),
						body: sourceRevision === 1 ? originalSource : updatedSource,
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
					},
				],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/dashboard/updates") {
			updateRequests += 1;
			const token = url.searchParams.get("token") ?? "token-stage-0";
			const changed = sourceRevision > 1 && token === "token-stage-0";
			return json(route, {
				token: changed ? "token-stage-1" : token,
				generated_at: "2026-04-30T10:00:00Z",
				lists: {
					feed: {
						changed,
						new_count: changed ? 1 : 0,
						latest_keys: changed ? ["release:20001"] : ["release:20001"],
					},
					briefs: { changed: false, new_count: 0, latest_keys: [] },
					notifications: { changed: false, new_count: 0, latest_keys: [] },
				},
			});
		}

		if (req.method() === "POST" && pathname === "/api/translate/results") {
			const body = req.postDataJSON() as {
				request_ids?: Record<string, string>;
				items?: Array<{
					kind?: string;
					producer_ref?: string;
					entity_id?: string;
					variant?: string;
					source_blocks?: Array<{ slot?: string; text?: string }>;
				}>;
			};
			const results = [];
			let oldTranslationPollCompleted = false;
			let oldSmartPollCompleted = false;
			for (const item of body.items ?? []) {
				const isTranslation = item.kind === "release_summary";
				const isSmart = item.kind === "release_smart";
				if (!isTranslation && !isSmart) continue;
				if (isTranslation) translationRequests += 1;
				if (isSmart) smartRequests += 1;
				const sourceBody =
					item.source_blocks?.find((block) => block.slot === "body_markdown")
						?.text ?? null;
				const requestId = item.producer_ref
					? body.request_ids?.[item.producer_ref]
					: undefined;
				submittedSources.push({ kind: item.kind ?? "", body: sourceBody });
				if (sourceBody === originalSource && isTranslation && !requestId) {
					markOldTranslationStarted();
				}
				if (sourceBody === originalSource && isSmart && !requestId) {
					markOldSmartStarted();
				}
				if (sourceBody === originalSource && isTranslation && requestId) {
					markOldTranslationPollStarted();
					await oldTranslationGate;
					oldTranslationPollCompleted = true;
				}
				if (sourceBody === originalSource && isSmart && requestId) {
					markOldSmartPollStarted();
					await oldSmartGate;
					oldSmartPollCompleted = true;
				}
				const pendingOldRequest = sourceBody === originalSource && !requestId;
				const resultText = isTranslation
					? sourceBody === updatedSource
						? freshTranslation
						: staleTranslation
					: sourceBody === updatedSource
						? freshSmart
						: staleSmart;
				results.push({
					producer_ref: item.producer_ref,
					entity_id: item.entity_id,
					kind: item.kind,
					variant: item.variant,
					status: pendingOldRequest ? "running" : "ready",
					request_id: pendingOldRequest
						? `${item.kind ?? "request"}-old-request`
						: (requestId ?? `${item.kind ?? "request"}-current-request`),
					title_zh: resultText,
					summary_md: resultText,
					body_md: resultText,
					error: null,
					error_code: null,
					error_summary: null,
					error_detail: null,
				});
			}
			const response = await json(route, { items: results });
			if (oldTranslationPollCompleted) markOldTranslationFinished();
			if (oldSmartPollCompleted) markOldSmartFinished();
			return response;
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
				check: { state: "idle", message: null, checked_at: null },
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

	try {
		await page.goto("/");
		const release = page.locator('[data-feed-item-key="release:20001"]');
		await expect(release).toBeVisible({ timeout: 15_000 });
		await expect.poll(() => updateRequests).toBe(1);
		await release.getByRole("tab", { name: "翻译" }).click();
		await oldTranslationStarted;
		await release.getByRole("tab", { name: "润色" }).click();
		await oldSmartStarted;
		await Promise.all([oldTranslationPollStarted, oldSmartPollStarted]);

		const waitForUpdatedProjection = (kind: string) =>
			page.waitForResponse((response) => {
				const request = response.request();
				if (
					request.method() !== "POST" ||
					new URL(response.url()).pathname !== "/api/translate/results"
				) {
					return false;
				}
				const body = request.postDataJSON() as {
					items?: Array<{
						kind?: string;
						source_blocks?: Array<{ slot?: string; text?: string }>;
					}>;
				};
				return body.items?.some(
					(item) =>
						item.kind === kind &&
						item.source_blocks?.some(
							(block) =>
								block.slot === "body_markdown" && block.text === updatedSource,
						),
				);
			});
		const updatedTranslationResponse =
			waitForUpdatedProjection("release_summary");
		const updatedSmartResponse = waitForUpdatedProjection("release_smart");
		sourceRevision = 2;
		await page.evaluate(() => window.dispatchEvent(new Event("online")));
		await expect.poll(() => feedRequests).toBeGreaterThan(1);
		await page.getByRole("button", { name: "上方有 1 条新动态" }).click();
		await expect(release).toBeVisible();
		await expect
			.poll(() =>
				submittedSources.map((source) => `${source.kind}:${source.body}`),
			)
			.toContain(`release_summary:${updatedSource}`);
		await expect
			.poll(() =>
				submittedSources.map((source) => `${source.kind}:${source.body}`),
			)
			.toContain(`release_smart:${updatedSource}`);
		await expect.poll(() => translationRequests).toBeGreaterThan(1);
		await expect.poll(() => smartRequests).toBeGreaterThan(1);
		const [translationResponse, smartResponse] = await Promise.all([
			updatedTranslationResponse,
			updatedSmartResponse,
		]);
		await Promise.all([
			translationResponse.finished(),
			smartResponse.finished(),
		]);
		await page.evaluate(
			() =>
				new Promise<void>((resolve) => requestAnimationFrame(() => resolve())),
		);

		await release.getByRole("tab", { name: "原文" }).click();
		await expect(
			release.getByRole("tabpanel", { name: "原文" }).getByText(updatedSource),
		).toBeVisible();
		const translatedTab = release.getByRole("tab", { name: "翻译" });
		await translatedTab.click();
		await expect(translatedTab).toHaveAttribute("aria-selected", "true");
		await expect(
			release
				.getByRole("tabpanel", { name: "翻译" })
				.getByText(freshTranslation),
		).toBeVisible();
		const smartTab = release.getByRole("tab", { name: "润色" });
		await smartTab.click();
		await expect(smartTab).toHaveAttribute("aria-selected", "true");
		await expect(
			release.getByRole("tabpanel", { name: "润色" }).getByText(freshSmart),
		).toBeVisible();

		releaseOldTranslation?.();
		releaseOldSmart?.();
		await Promise.all([oldTranslationFinished, oldSmartFinished]);
		await expect(page.getByText(staleTranslation)).toHaveCount(0);
		await expect(page.getByText(staleSmart)).toHaveCount(0);
		await translatedTab.click();
		await expect(
			release
				.getByRole("tabpanel", { name: "翻译" })
				.getByText(freshTranslation),
		).toBeVisible();
		await smartTab.click();
		await expect(
			release.getByRole("tabpanel", { name: "润色" }).getByText(freshSmart),
		).toBeVisible();
	} finally {
		releaseOldTranslation?.();
		releaseOldSmart?.();
	}
});

test("same-key source refresh retries terminal translation and smart failures", async ({
	page,
}) => {
	const originalSource = "Terminal source revision one";
	const updatedSource = "Terminal source revision two";
	const freshTranslation = "Translation recovered for revision two";
	const freshSmart = "Polish recovered for revision two";
	let sourceRevision = 1;
	let updateRequests = 0;
	const submittedSources: Array<{ kind: string; body: string | null }> = [];

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

		if (req.method() === "GET" && pathname === "/api/dashboard/feed") {
			return mockUnavailableDashboardReadableFeed(route);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			return json(route, {
				items: [
					{
						...buildReleaseFeedItem("20001", "Same release source refresh"),
						body: sourceRevision === 1 ? originalSource : updatedSource,
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
					},
				],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/dashboard/updates") {
			updateRequests += 1;
			const token = url.searchParams.get("token") ?? "token-stage-0";
			const changed = sourceRevision > 1 && token === "token-stage-0";
			return json(route, {
				token: changed ? "token-stage-1" : token,
				generated_at: "2026-04-30T10:00:00Z",
				lists: {
					feed: {
						changed,
						new_count: changed ? 1 : 0,
						latest_keys: changed ? ["release:20001"] : [],
					},
					briefs: { changed: false, new_count: 0, latest_keys: [] },
					notifications: { changed: false, new_count: 0, latest_keys: [] },
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
					source_blocks?: Array<{ slot?: string; text?: string }>;
				}>;
			};
			return json(route, {
				items: (body.items ?? []).map((item) => {
					const sourceBody =
						item.source_blocks?.find((block) => block.slot === "body_markdown")
							?.text ?? null;
					const isTranslation = item.kind === "release_summary";
					submittedSources.push({ kind: item.kind ?? "", body: sourceBody });
					const terminal = sourceBody === originalSource;
					const output = terminal
						? isTranslation
							? "Terminal translation fixture failure"
							: "Terminal smart fixture failure"
						: isTranslation
							? freshTranslation
							: freshSmart;
					return {
						producer_ref: item.producer_ref,
						entity_id: item.entity_id,
						kind: item.kind,
						variant: item.variant,
						status: terminal ? "error" : "ready",
						title_zh: output,
						summary_md: output,
						body_md: output,
						error: terminal ? output : null,
						error_code: terminal ? "fixture_terminal" : null,
						error_summary: terminal ? output : null,
						error_detail: null,
					};
				}),
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
				check: { state: "idle", message: null, checked_at: null },
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
	const release = page.locator('[data-feed-item-key="release:20001"]');
	await expect(release).toBeVisible({ timeout: 15_000 });
	await expect.poll(() => updateRequests).toBeGreaterThan(0);
	await expect
		.poll(() =>
			submittedSources.map((source) => `${source.kind}:${source.body}`),
		)
		.toContain(`release_summary:${originalSource}`);
	await expect
		.poll(() =>
			submittedSources.map((source) => `${source.kind}:${source.body}`),
		)
		.toContain(`release_smart:${originalSource}`);

	await release.getByRole("tab", { name: "翻译" }).click();
	await expect(
		release.getByText("Terminal translation fixture failure"),
	).toBeVisible();
	await release.getByRole("tab", { name: "润色" }).click();
	await expect(
		release.getByText("Terminal smart fixture failure"),
	).toBeVisible();

	sourceRevision = 2;
	await page.evaluate(() => window.dispatchEvent(new Event("online")));
	const newContentButton = page.getByRole("button", {
		name: "上方有 1 条新动态",
	});
	await expect(newContentButton).toBeVisible();
	await newContentButton.click();
	await expect(
		release.getByRole("tabpanel", { name: "原文" }).getByText(updatedSource),
	).toBeVisible();
	await expect
		.poll(() =>
			submittedSources.map((source) => `${source.kind}:${source.body}`),
		)
		.toContain(`release_summary:${updatedSource}`);
	await expect
		.poll(() =>
			submittedSources.map((source) => `${source.kind}:${source.body}`),
		)
		.toContain(`release_smart:${updatedSource}`);

	await release.getByRole("tab", { name: "翻译" }).click();
	await expect(release.getByText(freshTranslation)).toBeVisible();
	await release.getByRole("tab", { name: "润色" }).click();
	await expect(release.getByText(freshSmart)).toBeVisible();
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

		if (req.method() === "GET" && pathname === "/api/dashboard/feed") {
			return mockUnavailableDashboardReadableFeed(route);
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

		if (req.method() === "GET" && pathname === "/api/dashboard/feed") {
			return mockUnavailableDashboardReadableFeed(route);
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

test("dashboard discards live-update responses from before access sync", async ({
	page,
}) => {
	const taskId = "task-live-update-stale";
	let readableRequests = 0;
	let updateRequests = 0;
	let syncRequests = 0;
	const updateRequestTokens: Array<string | null> = [];
	let markHeldUpdateStarted: (() => void) | null = null;
	let releaseHeldUpdate: (() => void) | null = null;
	const heldUpdateStarted = new Promise<void>((resolve) => {
		markHeldUpdateStarted = resolve;
	});
	const heldUpdateGate = new Promise<void>((resolve) => {
		releaseHeldUpdate = resolve;
	});

	await page.addInitScript(
		({ taskId }) => {
			type TestWindow = Window & {
				__failLiveUpdateSync?: () => void;
			};
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

				constructor(url: string | URL) {
					this.url = String(url);
					if (!this.url.endsWith(`/api/tasks/${taskId}/events`)) return;
					(window as TestWindow).__failLiveUpdateSync = () => {
						const event = new MessageEvent("task.completed", {
							data: JSON.stringify({
								task_id: taskId,
								status: "failed",
								error: "access sync failed",
							}),
						});
						for (const listener of this.listeners.get("task.completed") ?? []) {
							listener.call(this as unknown as EventSource, event);
						}
					};
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
				}
			}

			Object.defineProperty(window, "EventSource", {
				configurable: true,
				writable: true,
				value: MockEventSource,
			});
		},
		{ taskId },
	);

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
		if (req.method() === "GET" && pathname === "/api/dashboard/feed") {
			readableRequests += 1;
			return json(route, {
				sections: [
					{
						id: "2026-04-30",
						date: "2026-04-30",
						kind: "raw",
						item_count: 1,
						brief: null,
						items: [
							buildReleaseFeedItem("live-baseline", "Access-rebased release"),
						],
						items_next_cursor: null,
						supplemental_items: [],
					},
				],
				next_cursor: null,
			});
		}
		if (req.method() === "GET" && pathname === "/api/feed") {
			return json(route, { items: [], next_cursor: null });
		}
		if (req.method() === "GET" && pathname === "/api/dashboard/updates") {
			updateRequests += 1;
			updateRequestTokens.push(url.searchParams.get("token"));
			if (updateRequests === 2) {
				markHeldUpdateStarted?.();
				await heldUpdateGate;
				return json(route, {
					token: "stale-access-token",
					lists: {
						feed: {
							changed: true,
							new_count: 1,
							latest_keys: ["release:stale-after-access"],
						},
						briefs: { changed: false, new_count: 0, latest_keys: [] },
						notifications: { changed: false, new_count: 0, latest_keys: [] },
					},
				});
			}
			return json(route, {
				token: "baseline-access-token",
				lists: {
					feed: { changed: false, new_count: 0, latest_keys: [] },
					briefs: { changed: false, new_count: 0, latest_keys: [] },
					notifications: { changed: false, new_count: 0, latest_keys: [] },
				},
			});
		}
		if (req.method() === "POST" && pathname === "/api/sync/all") {
			syncRequests += 1;
			return json(route, { task_id: taskId });
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
			{ error: { code: "not_found", message: pathname } },
			404,
		);
	});

	await page.goto("/");
	await expect(page.getByText("Access-rebased release")).toBeVisible({
		timeout: 15_000,
	});
	await expect.poll(() => updateRequests).toBeGreaterThan(0);
	const readableRequestsBeforeSync = readableRequests;
	await page.evaluate(() => window.dispatchEvent(new Event("online")));
	await expect.poll(() => updateRequests).toBeGreaterThanOrEqual(2);
	await heldUpdateStarted;

	await page.getByRole("banner").getByRole("button", { name: "同步" }).click();
	await expect.poll(() => syncRequests).toBe(1);
	await expect
		.poll(() =>
			page.evaluate(() =>
				Boolean(
					(window as typeof window & { __failLiveUpdateSync?: () => void })
						.__failLiveUpdateSync,
				),
			),
		)
		.toBe(true);
	await page.evaluate(() => {
		(
			window as typeof window & { __failLiveUpdateSync?: () => void }
		).__failLiveUpdateSync?.();
	});
	await expect
		.poll(() => readableRequests)
		.toBeGreaterThan(readableRequestsBeforeSync);
	await expect(page.getByText("Access-rebased release")).toBeVisible();
	releaseHeldUpdate?.();
	await expect.poll(() => updateRequests).toBeGreaterThanOrEqual(3);
	await expect
		.poll(() => updateRequestTokens.at(-1))
		.toBe("baseline-access-token");
	await expect(
		page.locator('[data-dashboard-new-content-boundary="true"]'),
	).toHaveCount(0);
});
