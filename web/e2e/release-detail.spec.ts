import { type Page, type Route, expect, test } from "@playwright/test";

type ApiOptions = {
	releaseId: string;
	detailTitle: string;
	translatedTitle: string;
	translatedSummary: string;
	feedTimestamp: string;
	detailPublishedAt: string;
	briefDate: string;
	briefWindowStart: string;
	briefWindowEnd: string;
	briefMarkdown: string;
	briefCreatedAt: string;
	withReactionFeed?: boolean;
	withAutoTranslateFeed?: boolean;
	autoTranslateFeedCount?: number;
	autoTranslateInitialReadyIds?: string[];
	autoTranslateResolveStatuses?: Record<string, "ready" | "missing" | "error">;
	releaseDetailPendingPolls?: number;
};

type MockApiTracker = {
	translationResolveEntityIds: string[][];
	translationBatchEntityIds: string[][];
	translationSingleEntityIds: string[];
};

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function makeAutoTranslateReleaseId(index: number) {
	return `200${`${index + 1}`.padStart(2, "0")}`;
}

function makeAutoTranslateFeedItems(
	count: number,
	options?: {
		initialReadyIds?: string[];
	},
) {
	const initialReadyIds = new Set(options?.initialReadyIds ?? []);
	return Array.from({ length: count }, (_, index) => {
		const releaseId = makeAutoTranslateReleaseId(index);
		const initialReady = initialReadyIds.has(releaseId);
		const readyPayload = makeFeedTranslationPayload(releaseId);
		return {
			kind: "release",
			ts: `2026-02-22T${`${(index % 10) + 10}`.padStart(2, "0")}:22:33Z`,
			id: releaseId,
			repo_full_name: "owner/repo",
			title: `Release ${releaseId}`,
			excerpt: [
				`- lane ${index + 1} keeps current-screen work first`,
				"- next-screen requests should submit in parallel",
				"- secondary prefetch continues within token budget",
			].join("\n"),
			subtitle: null,
			reason: null,
			subject_type: null,
			html_url: `https://github.com/owner/repo/releases/tag/v${releaseId}`,
			unread: null,
			translated: initialReady
				? {
						lang: "zh-CN",
						status: "ready",
						title: readyPayload.title_zh,
						summary: readyPayload.summary_md,
						auto_translate: true,
					}
				: {
						lang: "zh-CN",
						status: "missing",
						title: null,
						summary: null,
					},
			reactions: null,
		};
	});
}

function makeFeedTranslationPayload(
	releaseId: string,
	status: "ready" | "missing" | "error" = "ready",
) {
	return {
		producer_ref: `feed.auto_translate:release:${releaseId}`,
		entity_id: releaseId,
		kind: "release_summary",
		variant: "feed_card",
		status,
		title_zh: status === "ready" ? `发布说明 ${releaseId}` : null,
		summary_md:
			status === "ready" ? `这是 release ${releaseId} 的中文摘要。` : null,
		body_md: null,
		error: status === "error" ? `translate returned ${status}` : null,
		work_item_id: `work-feed-${releaseId}`,
		batch_id: `batch-feed-${releaseId}`,
	};
}

function makeFeedRequestId(releaseId: string) {
	return `req-feed-${releaseId}`;
}

async function installApiMocks(
	page: Page,
	options?: Partial<ApiOptions>,
): Promise<MockApiTracker> {
	const cfg: ApiOptions = {
		releaseId: "123",
		detailTitle: "Release 123",
		translatedTitle: "发布说明 123",
		translatedSummary: "这是 release 123 的中文详情摘要。",
		feedTimestamp: "2026-02-22T11:22:33Z",
		detailPublishedAt: "2026-02-22T11:22:33Z",
		briefDate: "2026-02-23",
		briefWindowStart: "2026-02-22T00:00:00Z",
		briefWindowEnd: "2026-02-23T00:00:00Z",
		briefMarkdown: `[repo/v1.2.3](/?tab=briefs&release=${options?.releaseId ?? "123"})`,
		briefCreatedAt: "2026-02-23T08:00:00Z",
		releaseDetailPendingPolls: 0,
		autoTranslateFeedCount: 18,
		...options,
	};
	const translationRequestPolls = new Map<string, number>();
	const tracker: MockApiTracker = {
		translationResolveEntityIds: [],
		translationBatchEntityIds: [],
		translationSingleEntityIds: [],
	};

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

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
			});
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			const items = cfg.withReactionFeed
				? [
						{
							kind: "release",
							ts: cfg.feedTimestamp,
							id: cfg.releaseId,
							repo_full_name: "owner/repo",
							title: cfg.detailTitle,
							excerpt: "- fix A\n- fix B",
							subtitle: null,
							reason: null,
							subject_type: null,
							html_url: "https://github.com/owner/repo/releases/tag/v1.2.3",
							unread: null,
							translated: {
								lang: "zh-CN",
								status: "ready",
								title: cfg.translatedTitle,
								summary: cfg.translatedSummary,
							},
							reactions: {
								counts: {
									plus1: 2,
									laugh: 0,
									heart: 0,
									hooray: 0,
									rocket: 0,
									eyes: 0,
								},
								viewer: {
									plus1: false,
									laugh: false,
									heart: false,
									hooray: false,
									rocket: false,
									eyes: false,
								},
								status: "ready",
							},
						},
					]
				: cfg.withAutoTranslateFeed
					? makeAutoTranslateFeedItems(cfg.autoTranslateFeedCount ?? 18, {
							initialReadyIds: cfg.autoTranslateInitialReadyIds,
						})
					: [];
			return json(route, { items, next_cursor: null });
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, [
				{
					date: cfg.briefDate,
					window_start: cfg.briefWindowStart,
					window_end: cfg.briefWindowEnd,
					content_markdown: cfg.briefMarkdown,
					created_at: cfg.briefCreatedAt,
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

		if (
			req.method() === "GET" &&
			pathname === `/api/releases/${cfg.releaseId}/detail`
		) {
			return json(route, {
				release_id: cfg.releaseId,
				repo_full_name: "owner/repo",
				tag_name: "v1.2.3",
				name: cfg.detailTitle,
				body: "- fix A\n- fix B",
				html_url: "https://github.com/owner/repo/releases/tag/v1.2.3",
				published_at: cfg.detailPublishedAt,
				is_prerelease: 0,
				is_draft: 0,
				translated: {
					lang: "zh-CN",
					status: "missing",
					title: null,
					summary: null,
				},
			});
		}

		if (req.method() === "POST" && pathname === "/api/translate/results") {
			const body = req.postDataJSON() as {
				items?: Array<{ entity_id?: string }>;
			};
			const entityIds = (body.items ?? []).map((item) => item.entity_id ?? "");
			tracker.translationResolveEntityIds.push(entityIds);
			return json(route, {
				items: entityIds.map((entityId) =>
					makeFeedTranslationPayload(
						entityId,
						cfg.autoTranslateResolveStatuses?.[entityId] ?? "ready",
					),
				),
			});
		}

		if (req.method() === "POST" && pathname === "/api/translate/requests") {
			const body = req.postDataJSON() as {
				mode?: string;
				item?: {
					producer_ref?: string;
					entity_id?: string;
					kind?: string;
					variant?: string;
				};
				items?: Array<{
					producer_ref?: string;
					entity_id?: string;
					kind?: string;
					variant?: string;
				}>;
			};
			if (body.mode === "wait") {
				const item = body.item;
				if (!item || item.entity_id !== cfg.releaseId) {
					return json(
						route,
						{ error: { message: "unexpected translation request" } },
						400,
					);
				}
				const requestId = `req-${item.kind ?? "translation"}-1`;
				const isPendingReleaseDetail =
					item.kind === "release_detail" && cfg.releaseDetailPendingPolls > 0;
				return json(route, {
					request_id: requestId,
					status: isPendingReleaseDetail ? "running" : "completed",
					result: {
						producer_ref:
							item.producer_ref ??
							(item.kind === "release_detail"
								? `release_detail:${cfg.releaseId}`
								: `feed.auto_translate:release:${cfg.releaseId}`),
						entity_id: cfg.releaseId,
						kind: item.kind ?? "release_summary",
						variant: item.variant ?? "feed_card",
						status: isPendingReleaseDetail ? "running" : "ready",
						title_zh: isPendingReleaseDetail ? null : cfg.translatedTitle,
						summary_md:
							item.kind === "release_detail" || isPendingReleaseDetail
								? null
								: cfg.translatedSummary,
						body_md:
							item.kind === "release_detail" && !isPendingReleaseDetail
								? cfg.translatedSummary
								: null,
						error: null,
						work_item_id:
							item.kind === "release_detail" ? "work-detail-1" : "work-feed-1",
						batch_id:
							item.kind === "release_detail"
								? "batch-detail-1"
								: "batch-feed-1",
					},
				});
			}
			if (body.mode === "async" && Array.isArray(body.items)) {
				const entityIds = body.items.map((item) => item.entity_id ?? "");
				tracker.translationBatchEntityIds.push(entityIds);
				return json(route, {
					requests: body.items.map((item) => ({
						request_id: makeFeedRequestId(item.entity_id ?? ""),
						status: "queued",
						producer_ref:
							item.producer_ref ??
							`feed.auto_translate:release:${item.entity_id ?? ""}`,
						entity_id: item.entity_id ?? "",
						kind: item.kind ?? "release_summary",
						variant: item.variant ?? "feed_card",
					})),
				});
			}
			if (body.mode === "async" && body.item) {
				const entityId = body.item.entity_id ?? "";
				tracker.translationSingleEntityIds.push(entityId);
				return json(route, {
					request_id: makeFeedRequestId(entityId),
					status: "queued",
					result:
						body.item.kind === "release_detail"
							? {
									producer_ref: `release_detail:${entityId}`,
									entity_id: entityId,
									kind: "release_detail",
									variant: "detail_card",
									status: "queued",
									title_zh: null,
									summary_md: null,
									body_md: null,
									error: null,
									work_item_id: "work-detail-1",
									batch_id: "batch-detail-1",
								}
							: {
									...makeFeedTranslationPayload(entityId),
									status: "queued",
									title_zh: null,
									summary_md: null,
								},
				});
			}
			return json(
				route,
				{ error: { message: "unsupported translation mode" } },
				400,
			);
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/translate/requests/")
		) {
			const requestId = pathname.split("/").at(-1);
			if (!requestId) {
				return json(route, { error: { message: "missing request id" } }, 400);
			}
			const polls = translationRequestPolls.get(requestId) ?? 0;
			translationRequestPolls.set(requestId, polls + 1);
			const isPendingReleaseDetail =
				requestId === "req-release_detail-1" &&
				polls < cfg.releaseDetailPendingPolls;
			if (requestId.startsWith("req-feed-")) {
				const releaseId = requestId.replace("req-feed-", "");
				return json(route, {
					request_id: requestId,
					status: "completed",
					result: makeFeedTranslationPayload(releaseId),
				});
			}
			return json(route, {
				request_id: requestId,
				status: isPendingReleaseDetail ? "running" : "completed",
				result: {
					producer_ref: `release_detail:${cfg.releaseId}`,
					entity_id: cfg.releaseId,
					kind: "release_detail",
					variant: "detail_card",
					status: isPendingReleaseDetail ? "running" : "ready",
					title_zh: isPendingReleaseDetail ? null : cfg.translatedTitle,
					summary_md: null,
					body_md: isPendingReleaseDetail ? null : cfg.translatedSummary,
					error: null,
					work_item_id: "work-detail-1",
					batch_id: "batch-detail-1",
				},
			});
		}

		return json(
			route,
			{ error: { message: `unhandled ${req.method()} ${pathname}` } },
			404,
		);
	});

	return tracker;
}

test("deep link with release id opens briefs tab and loads release detail", async ({
	page,
}) => {
	await installApiMocks(page, {
		releaseId: "289513858",
		detailTitle: "Release 289513858",
	});

	await page.goto("/?release=289513858");
	await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);

	await expect(page).toHaveURL(/tab=briefs/);
	await expect(page).toHaveURL(/release=289513858/);
	await expect(page.getByRole("tab", { name: "日报" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect(
		page.getByRole("heading", { name: "Release 详情" }),
	).toBeVisible();
	await expect(page.getByText("#289513858")).toBeVisible();
	await expect(page.getByText("owner/repo")).toBeVisible();
	await expect(
		page.getByRole("heading", { name: "Release 289513858" }),
	).toBeVisible();

	await page.getByRole("button", { name: "关闭" }).click();
	await expect(page).toHaveURL(/tab=briefs/);
	await expect(page).not.toHaveURL(/release=289513858/);
	await expect(page.getByRole("heading", { name: "Release 详情" })).toHaveCount(
		0,
	);
});

test("detail translate button updates card content", async ({ page }) => {
	await installApiMocks(page);

	await page.goto("/?tab=briefs&release=123");
	await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);

	await expect(
		page.getByRole("heading", { name: "Release 123" }),
	).toBeVisible();
	await page.getByRole("button", { name: "翻译" }).click();
	await expect(
		page.getByRole("heading", { name: "发布说明 123" }),
	).toBeVisible();
	await expect(
		page.getByText("这是 release 123 的中文详情摘要。", { exact: true }),
	).toBeVisible();

	await page.getByRole("button", { name: "原文" }).click();
	await expect(
		page.getByRole("heading", { name: "Release 123" }),
	).toBeVisible();
});

test("detail translate keeps polling an in-flight request until the result is ready", async ({
	page,
}) => {
	await installApiMocks(page, { releaseDetailPendingPolls: 2 });

	await page.goto("/?tab=briefs&release=123");
	await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);

	await page.getByRole("button", { name: "翻译" }).click();
	await expect(
		page.getByRole("heading", { name: "发布说明 123" }),
	).toBeVisible();
	await expect(
		page.getByText("这是 release 123 的中文详情摘要。", { exact: true }),
	).toBeVisible();
	await expect(page.getByText("translation wait timeout")).toHaveCount(0);
});

test("reaction fallback opens PAT dialog with accessible controls", async ({
	page,
}) => {
	await installApiMocks(page, { withReactionFeed: true });

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "Releases" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await page.getByTitle("赞").click();

	const patDialog = page.getByRole("dialog", {
		name: "配置 GitHub PAT 以启用反馈表情",
	});
	await expect(patDialog).toBeVisible();
	await expect(patDialog.getByLabel("GitHub PAT")).toBeVisible();
	await expect(
		patDialog.getByRole("button", { name: "稍后再说" }),
	).toBeVisible();
	await expect(
		patDialog.getByRole("button", { name: "保存并继续" }),
	).toBeDisabled();
	await patDialog.getByRole("button", { name: "稍后再说" }).click();
	await expect(patDialog).toHaveCount(0);
});

test("deep link with zero-padded release id still resolves detail", async ({
	page,
}) => {
	await installApiMocks(page, {
		releaseId: "123",
		detailTitle: "Release 123",
	});

	await page.goto("/?tab=briefs&release=00123");
	await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);

	await expect(page).toHaveURL(/tab=briefs/);
	await expect(page).toHaveURL(/release=123/);
	await expect(
		page.getByRole("heading", { name: "Release 123" }),
	).toBeVisible();
	await expect(page.getByText("#123")).toBeVisible();
});

test("feed auto translate resolves visible cards and the next 10 through results aggregation", async ({
	page,
}) => {
	const tracker = await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 18,
	});

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "Releases" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect
		.poll(() => tracker.translationResolveEntityIds.length)
		.toBeGreaterThan(0);

	const firstWindow = tracker.translationResolveEntityIds[0];
	expect(firstWindow.length).toBeGreaterThan(0);
	expect(firstWindow).toEqual(
		Array.from({ length: firstWindow.length }, (_, index) =>
			makeAutoTranslateReleaseId(index),
		),
	);

	expect(new Set(firstWindow).size).toBe(firstWindow.length);
	expect(tracker.translationBatchEntityIds).toHaveLength(0);
	expect(tracker.translationSingleEntityIds).toHaveLength(0);

	await expect(
		page.getByRole("heading", { name: "发布说明 20001" }),
	).toBeVisible();
	await expect(
		page.getByText("这是 release 20001 的中文摘要。", { exact: true }),
	).toBeVisible();
});

test("feed auto translate clears stale ready cards when aggregation returns missing", async ({
	page,
}) => {
	const releaseId = makeAutoTranslateReleaseId(0);

	await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 12,
		autoTranslateInitialReadyIds: [releaseId],
		autoTranslateResolveStatuses: {
			[releaseId]: "missing",
		},
	});

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "Releases" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect(
		page.getByRole("heading", { name: `Release ${releaseId}` }),
	).toBeVisible();
	await expect(
		page.getByRole("heading", { name: `发布说明 ${releaseId}` }),
	).toHaveCount(0);
	await expect(
		page.getByText(`这是 release ${releaseId} 的中文摘要。`, { exact: true }),
	).toHaveCount(0);
	await expect(
		page.getByRole("button", { name: "翻译" }).first(),
	).toBeDisabled();
});

test.describe("localized timestamps", () => {
	test.describe("non-DST browser timezone", () => {
		test.use({ timezoneId: "Asia/Shanghai" });

		test("release feed cards render timestamps in the browser timezone", async ({
			page,
		}) => {
			await installApiMocks(page, { withReactionFeed: true });

			await page.goto("/?tab=releases");
			await expect(page.getByRole("tab", { name: "Releases" })).toHaveAttribute(
				"aria-selected",
				"true",
			);
			await expect(
				page.getByText("2026-02-22 19:22:33", { exact: true }),
			).toBeVisible();
		});

		test("release detail cards render published_at in the browser timezone", async ({
			page,
		}) => {
			await installApiMocks(page);

			await page.goto("/?tab=briefs&release=123");
			await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);
			await expect(
				page.getByText("#123 · 2026-02-22 19:22:33", { exact: true }),
			).toBeVisible();
		});

		test("brief windows and markdown timestamps stay correct in a non-DST timezone", async ({
			page,
		}) => {
			await installApiMocks(page, {
				briefDate: "2026-07-23",
				briefWindowStart: "2026-07-22T00:00:00Z",
				briefWindowEnd: "2026-07-23T00:00:00Z",
				briefMarkdown:
					"## 概览\n\n- 时间窗口：2026-07-22T00:00:00Z → 2026-07-23T00:00:00Z\n\n## 项目更新\n\n- [repo/v1.2.3](/?tab=briefs&release=123) · 2026-07-22T11:22:33Z · [GitHub Release](https://github.com/owner/repo/releases/tag/v1.2.3)",
				briefCreatedAt: "2026-07-23T08:00:00Z",
			});

			await page.goto("/?tab=briefs&release=123");
			await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);
			const briefPanel = page.getByRole("tabpanel", { name: "日报" });
			await expect(
				page.getByText(
					"#2026-07-23 · 2026-07-22 08:00:00 → 2026-07-23 08:00:00",
					{
						exact: true,
					},
				),
			).toBeVisible();
			await expect(briefPanel).toContainText("2026-07-22 19:22:33");
			await expect(briefPanel).not.toContainText("2026-07-22T11:22:33Z");
		});
	});

	test.describe("DST browser timezone", () => {
		test.use({ timezoneId: "America/New_York" });

		test("brief windows and markdown timestamps use the browser DST offset", async ({
			page,
		}) => {
			await installApiMocks(page, {
				briefDate: "2026-07-23",
				briefWindowStart: "2026-07-22T00:00:00Z",
				briefWindowEnd: "2026-07-23T00:00:00Z",
				briefMarkdown:
					"## 概览\n\n- 时间窗口：2026-07-22T00:00:00Z → 2026-07-23T00:00:00Z\n\n## 项目更新\n\n- [repo/v1.2.3](/?tab=briefs&release=123) · 2026-07-22T11:22:33Z · [GitHub Release](https://github.com/owner/repo/releases/tag/v1.2.3)",
				briefCreatedAt: "2026-07-23T08:00:00Z",
			});

			await page.goto("/?tab=briefs&release=123");
			await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);
			const briefPanel = page.getByRole("tabpanel", { name: "日报" });
			await expect(
				page.getByText(
					"#2026-07-23 · 2026-07-21 20:00:00 → 2026-07-22 20:00:00",
					{
						exact: true,
					},
				),
			).toBeVisible();
			await expect(briefPanel).toContainText("2026-07-22 07:22:33");
			await expect(briefPanel).not.toContainText("2026-07-22T11:22:33Z");
		});
	});
});
