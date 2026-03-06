import { type Page, type Route, expect, test } from "@playwright/test";

type ApiOptions = {
	releaseId: string;
	detailTitle: string;
	translatedTitle: string;
	translatedSummary: string;
	withReactionFeed?: boolean;
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
		...options,
	};

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(route, {
				user: {
					id: 1,
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
			return json(route, {
				items: cfg.withReactionFeed
					? [
							{
								kind: "release",
								ts: "2026-02-22T11:22:33Z",
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
					: [],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, [
				{
					date: "2026-02-23",
					window_start: "2026-02-22T00:00:00Z",
					window_end: "2026-02-23T00:00:00Z",
					content_markdown: `[repo/v1.2.3](/?tab=briefs&release=${cfg.releaseId})`,
					created_at: "2026-02-23T08:00:00Z",
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
				published_at: "2026-02-22T11:22:33Z",
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

		if (
			req.method() === "POST" &&
			pathname === "/api/translate/release/detail"
		) {
			const body = req.postDataJSON() as { release_id?: string };
			if (body.release_id !== cfg.releaseId) {
				return json(
					route,
					{ error: { message: "unexpected release id" } },
					400,
				);
			}
			return json(route, {
				lang: "zh-CN",
				status: "ready",
				title: cfg.translatedTitle,
				summary: cfg.translatedSummary,
			});
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
