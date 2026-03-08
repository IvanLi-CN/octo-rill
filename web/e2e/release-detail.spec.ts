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
};

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

async function installApiMocks(page: Page, options?: Partial<ApiOptions>) {
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
		...options,
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
			const translated = cfg.withReactionFeed
				? {
						lang: "zh-CN",
						status: "ready",
						title: cfg.translatedTitle,
						summary: cfg.translatedSummary,
					}
				: cfg.withAutoTranslateFeed
					? {
							lang: "zh-CN",
							status: "missing",
							title: null,
							summary: null,
						}
					: null;
			const items =
				cfg.withReactionFeed || cfg.withAutoTranslateFeed
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
								translated,
								reactions: cfg.withReactionFeed
									? {
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
										}
									: null,
							},
						]
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

		if (req.method() === "POST" && pathname === "/api/translate/requests") {
			const body = req.postDataJSON() as {
				mode?: string;
				items?: Array<{ entity_id?: string; kind?: string; variant?: string }>;
			};
			const item = body.items?.[0];
			if (!item || item.entity_id !== cfg.releaseId) {
				return json(
					route,
					{ error: { message: "unexpected translation request" } },
					400,
				);
			}
			if (body.mode === "wait") {
				return json(route, {
					request_id: "req-release-detail-1",
					status: "completed",
					items: [
						{
							producer_ref: cfg.releaseId,
							entity_id: cfg.releaseId,
							kind: "release_detail",
							variant: "detail_card",
							status: "ready",
							title_zh: cfg.translatedTitle,
							summary_md: null,
							body_md: cfg.translatedSummary,
							error: null,
							work_item_id: "work-1",
							batch_id: "batch-1",
						},
					],
				});
			}
			if (body.mode === "stream") {
				return route.fulfill({
					status: 200,
					contentType: "application/x-ndjson",
					body: `${[
						JSON.stringify({
							event: "queued",
							request_id: "req-feed-stream-1",
							status: "queued",
						}),
						JSON.stringify({
							event: "completed",
							request_id: "req-feed-stream-1",
							status: "completed",
							batch_ids: ["batch-feed-1"],
							items: [
								{
									producer_ref: cfg.releaseId,
									entity_id: cfg.releaseId,
									kind: "release_summary",
									variant: "feed_card",
									status: "ready",
									title_zh: cfg.translatedTitle,
									summary_md: cfg.translatedSummary,
									body_md: null,
									error: null,
									work_item_id: "work-feed-1",
									batch_id: "batch-feed-1",
								},
							],
						}),
					].join("\n")}
`,
				});
			}
			return json(
				route,
				{ error: { message: "unsupported translation mode" } },
				400,
			);
		}

		return json(
			route,
			{ error: { message: `unhandled ${req.method()} ${pathname}` } },
			404,
		);
	});
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

test("feed auto translate resolves from stream request", async ({ page }) => {
	await installApiMocks(page, {
		withAutoTranslateFeed: true,
	});

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "Releases" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect(
		page.getByRole("heading", { name: "发布说明 123" }),
	).toBeVisible();
	await expect(
		page.getByText("这是 release 123 的中文详情摘要。", { exact: true }),
	).toBeVisible();
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
