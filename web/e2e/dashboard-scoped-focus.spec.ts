import { expect, test, type Page, type Route } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function buildScopedRelease(
	id: string,
	repoFullName: string,
	title: string,
	publishedAt = "2026-04-30T08:00:00Z",
) {
	return {
		kind: "release",
		ts: publishedAt,
		id,
		repo_full_name: repoFullName,
		repo_visual: null,
		title,
		body: "- scoped dashboard content",
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `https://github.com/${repoFullName}/releases/tag/${encodeURIComponent(title)}`,
		unread: null,
		translated: null,
		smart: null,
		reactions: null,
	};
}

function buildRepoStar(
	id: string,
	repoFullName: string,
	login: string,
	ts = "2026-04-30T07:30:00Z",
) {
	return {
		kind: "repo_star_received",
		ts,
		id,
		repo_full_name: repoFullName,
		repo_visual: null,
		title: null,
		body: null,
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `https://github.com/${login}`,
		unread: null,
		actor: {
			login,
			avatar_url: null,
			html_url: `https://github.com/${login}`,
		},
		translated: null,
		smart: null,
		reactions: null,
	};
}

async function installScopedFocusMocks(
	page: Page,
	options?: {
		includeOwnReleases?: boolean;
	},
) {
	const includeOwnReleases = options?.includeOwnReleases ?? false;

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse(
					{
						id: "story-user",
						github_user_id: 42,
						login: "IvanLi-CN",
						name: "Ivan Li",
						avatar_url: null,
						email: "ivan@example.com",
						is_admin: false,
					},
					{ include_own_releases: includeOwnReleases },
				),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			const scope = url.searchParams.get("scope");
			const items = url.searchParams.get("items");
			const org = url.searchParams.get("org");
			const types = url.searchParams.get("types");

			if (scope === "repo" && items === "lobehub/lobe-chat") {
				if (types === "releases") {
					return json(route, {
						items: [
							buildScopedRelease(
								"focus-repo-1",
								"lobehub/lobe-chat",
								"桌面版 Stable v2.1.47",
							),
						],
						next_cursor: null,
					});
				}
				return json(route, {
					items: [
						buildScopedRelease(
							"focus-repo-1",
							"lobehub/lobe-chat",
							"桌面版 Stable v2.1.47",
						),
						buildRepoStar("focus-repo-star", "lobehub/lobe-chat", "gaearon"),
					],
					next_cursor: null,
				});
			}

			if (scope === "mine") {
				return json(route, {
					items: includeOwnReleases
						? [
								buildScopedRelease(
									"mine-release-1",
									"IvanLi-CN/octo-rill",
									"v2.65.0",
								),
							]
						: [],
					next_cursor: null,
				});
			}

			if (scope === "org" && org === "acme") {
				return json(route, {
					items: [
						buildScopedRelease("acme-release-1", "acme/rocket", "v3.2.1"),
						buildRepoStar("acme-star-1", "acme/rocket", "linus"),
					],
					next_cursor: null,
				});
			}

			return json(route, {
				items: [
					buildScopedRelease(
						"global-release-1",
						"lobehub/lobe-chat",
						"桌面版 Stable v2.1.47",
					),
					buildRepoStar("global-star-1", "lobehub/lobe-chat", "gaearon"),
				],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/dashboard/updates") {
			return json(route, {
				token: "scoped-focus-token",
				generated_at: "2026-04-30T10:00:00Z",
				lists: {
					feed: {
						changed: false,
						new_count: 0,
						latest_keys: [],
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

		if (req.method() === "GET" && pathname === "/api/reaction-token/status") {
			return json(route, {
				configured: false,
				masked_token: null,
				check: {
					state: "idle",
					message: null,
					checked_at: null,
				},
				owner: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/version") {
			return json(route, {
				ok: true,
				version: "2.4.6",
				source: "APP_EFFECTIVE_VERSION",
			});
		}

		if (req.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "2.4.6" });
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/releases/focus-repo-1/detail"
		) {
			return json(route, {
				release_id: "focus-repo-1",
				repo_full_name: "lobehub/lobe-chat",
				repo_visual: null,
				tag_name: "v2.1.47",
				previous_tag_name: "v2.1.46",
				name: "桌面版 Stable v2.1.47",
				body: "- scoped release detail body",
				body_truncated: false,
				html_url: "https://github.com/lobehub/lobe-chat/releases/tag/v2.1.47",
				published_at: "2026-04-30T08:00:00Z",
				is_prerelease: 0,
				is_draft: 0,
				translated: {
					lang: "zh-CN",
					status: "missing",
					title: null,
					summary: null,
				},
				smart: {
					lang: "zh-CN",
					status: "insufficient",
					title: null,
					summary: null,
				},
			});
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/repos/lobehub/lobe-chat/releases/tag/v2.1.47/detail"
		) {
			return json(route, {
				release_id: "focus-repo-1",
				repo_full_name: "lobehub/lobe-chat",
				repo_visual: null,
				tag_name: "v2.1.47",
				previous_tag_name: "v2.1.46",
				name: "桌面版 Stable v2.1.47",
				body: "- scoped release detail body",
				body_truncated: false,
				html_url: "https://github.com/lobehub/lobe-chat/releases/tag/v2.1.47",
				published_at: "2026-04-30T08:00:00Z",
				is_prerelease: 0,
				is_draft: 0,
				translated: {
					lang: "zh-CN",
					status: "missing",
					title: null,
					summary: null,
				},
				smart: {
					lang: "zh-CN",
					status: "insufficient",
					title: null,
					summary: null,
				},
			});
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
}

test("repo identity on release card opens scoped focus route and keeps releases sub-tab when source tab is releases", async ({
	page,
}) => {
	await installScopedFocusMocks(page);

	await page.goto("/releases");
	await expect(page.getByText("桌面版 Stable v2.1.47")).toBeVisible();
	const releasesPanel = page.getByRole("tabpanel", {
		name: "发布",
		exact: true,
	});
	const repoLink = releasesPanel
		.locator('[data-feed-item-key="release:global-release-1"]')
		.getByRole("link", { name: /^lobehub\/lobe-chat$/ });
	await expect(repoLink).toHaveAttribute(
		"href",
		"/focus/repo/lobehub/lobe-chat/releases",
	);
	await repoLink.click();
	await expect(page).toHaveURL("/focus/repo/lobehub/lobe-chat/releases");
	await expect(
		page.locator(
			'[data-dashboard-scope-summary="repo"][data-dashboard-scope-summary-layout="desktop"]',
		),
	).toBeVisible();
	await expect(page.getByText("桌面版 Stable v2.1.47")).toBeVisible();
	await expect(
		page.locator("[data-dashboard-sidebar-inbox='true']"),
	).toHaveCount(0);
});

test("account menu shows mine entry only when include_own_releases is enabled", async ({
	page,
}) => {
	await installScopedFocusMocks(page, { includeOwnReleases: true });

	await page.goto("/");
	await page.getByRole("button", { name: "查看账号信息" }).click();
	await expect(page.getByRole("link", { name: "我的仓库动态" })).toBeVisible();
	await page.getByRole("link", { name: "我的仓库动态" }).click();
	await expect(page).toHaveURL("/focus/mine");
	await expect(
		page.locator(
			'[data-dashboard-scope-summary="mine"][data-dashboard-scope-summary-layout="desktop"]',
		),
	).toBeVisible();

	await installScopedFocusMocks(page, { includeOwnReleases: false });
	await page.goto("/");
	await page.getByRole("button", { name: "查看账号信息" }).click();
	await expect(page.getByRole("link", { name: "我的仓库动态" })).toHaveCount(0);
});

test("scoped release detail closes back to the original focus route", async ({
	page,
}) => {
	await installScopedFocusMocks(page);

	await page.goto(
		"/lobehub/lobe-chat/releases/tag/v2.1.47?scope=repo&items=lobehub%2Flobe-chat&from=releases",
	);
	await expect(page).toHaveURL(
		"/lobehub/lobe-chat/releases/tag/v2.1.47?scope=repo&items=lobehub%2Flobe-chat&from=releases",
	);
	const detailDialog = page.getByRole("dialog");
	await expect(detailDialog).toBeVisible();
	await detailDialog.getByRole("button", { name: "关闭" }).click();
	await expect(page).toHaveURL("/focus/repo/lobehub/lobe-chat/releases");
	await expect(
		page.locator(
			'[data-dashboard-scope-summary="repo"][data-dashboard-scope-summary-layout="desktop"]',
		),
	).toBeVisible();
});
