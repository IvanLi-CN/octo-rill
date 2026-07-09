import { expect, test, type Page, type Route } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";
import { installPasskeyBrowserMock } from "./passkeyHelpers";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function buildAnnouncementFeedItem() {
	return {
		kind: "announcement",
		ts: "2026-07-09T12:00:00Z",
		id: "announcement-64",
		repo_full_name: "lobehub/lobe-chat",
		repo_visual: null,
		title: "公告：支持站内详情页",
		body: "- 公告标题改为站内跳转\n- 详情页默认进入润色 lane",
		body_truncated: false,
		subtitle: "仓库公告",
		reason: null,
		subject_type: "discussion",
		discussion_number: 64,
		discussion_key: "lobehub/lobe-chat#64",
		html_url: "https://github.com/lobehub/lobe-chat/discussions/64",
		unread: 1,
		actor: {
			login: "maintainer",
			avatar_url: null,
			html_url: "https://github.com/maintainer",
		},
		translated: {
			lang: "zh-CN",
			status: "ready",
			title: "公告：支持站内详情页（译文）",
			summary: "- 全部 tab 中的公告也支持译文 lane",
		},
		smart: {
			lang: "zh-CN",
			status: "ready",
			title: "公告：支持站内详情页 · 润色",
			summary: "- Discussion detail route 会保留返回上下文",
		},
		reactions: null,
	};
}

function buildAnnouncementDetailResponse() {
	return {
		repo_full_name: "lobehub/lobe-chat",
		discussion_number: 64,
		discussion_key: "lobehub/lobe-chat#64",
		repo_visual: null,
		title: "公告：支持站内详情页",
		body: [
			"## What's changed",
			"",
			"- 标题会进入 Dashboard 壳层中的 discussion 详情页",
			"- 右上角保留 GitHub 外跳作为 escape hatch",
		].join("\n"),
		html_url: "https://github.com/lobehub/lobe-chat/discussions/64",
		occurred_at: "2026-07-09T12:00:00Z",
		actor: {
			login: "maintainer",
			avatar_url: null,
			html_url: "https://github.com/maintainer",
		},
		translated: {
			lang: "zh-CN",
			status: "ready",
			title: "公告：支持站内 discussion 详情页",
			summary: "- 详情页译文可用",
		},
		smart: {
			lang: "zh-CN",
			status: "ready",
			title: "公告：支持站内详情页 · 润色",
			summary: "- 默认进入润色 lane\n- 返回工作台后恢复原 tab / scope",
		},
	};
}

async function installAuthenticatedAnnouncementMocks(page: Page) {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: "announcement-user",
					github_user_id: 42,
					login: "story-viewer",
					name: "Story Viewer",
					avatar_url: null,
					email: "story-viewer@example.com",
					is_admin: false,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			const scope = url.searchParams.get("scope");
			const items = url.searchParams.get("items");
			if (scope === "repo" && items === "lobehub/lobe-chat") {
				return json(route, {
					items: [buildAnnouncementFeedItem()],
					next_cursor: null,
				});
			}

			return json(route, {
				items: [
					buildAnnouncementFeedItem(),
					{
						kind: "repo_star_received",
						ts: "2026-07-09T11:32:00Z",
						id: "star-1",
						repo_full_name: "lobehub/lobe-chat",
						repo_visual: null,
						title: null,
						body: null,
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/gaearon",
						unread: null,
						actor: {
							login: "gaearon",
							avatar_url: null,
							html_url: "https://github.com/gaearon",
						},
						translated: null,
						smart: null,
						reactions: null,
					},
				],
				next_cursor: null,
			});
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/repos/lobehub/lobe-chat/discussions/64/detail"
		) {
			return json(route, buildAnnouncementDetailResponse());
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/dashboard/updates") {
			return json(route, {
				token: "announcement-detail-token",
				generated_at: "2026-07-09T12:00:00Z",
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
				version: "2.6.0",
				source: "APP_EFFECTIVE_VERSION",
			});
		}

		if (req.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "2.6.0" });
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

async function installAnonymousAnnouncementMocks(page: Page) {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				{
					error: {
						code: "unauthorized",
						message: "unauthorized",
					},
				},
				401,
			);
		}

		if (req.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "2.6.0" });
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

test("announcement title deep link preserves global all-tab context", async ({
	page,
}) => {
	await installAuthenticatedAnnouncementMocks(page);

	await page.goto("/");

	const titleLink = page.getByRole("link", { name: "公告：支持站内详情页" });
	await expect(titleLink).toHaveAttribute(
		"href",
		"/lobehub/lobe-chat/discussions/64?from=all",
	);
	await titleLink.click();
	await expect(page).toHaveURL("/lobehub/lobe-chat/discussions/64?from=all");
	await expect(page.getByRole("tab", { name: "润色" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect(
		page.getByRole("heading", { name: "公告：支持站内详情页 · 润色" }),
	).toBeVisible();

	await page.getByRole("button", { name: "返回工作台" }).click();
	await expect(page).toHaveURL("/");
	await expect(
		page.getByRole("heading", { name: "公告：支持站内详情页" }),
	).toBeVisible();
});

test("scoped announcement detail restores the original focus route", async ({
	page,
}) => {
	await installAuthenticatedAnnouncementMocks(page);

	await page.goto("/focus/repo/lobehub/lobe-chat");

	const titleLink = page.getByRole("link", { name: "公告：支持站内详情页" });
	await expect(titleLink).toHaveAttribute(
		"href",
		"/lobehub/lobe-chat/discussions/64?scope=repo&items=lobehub%2Flobe-chat&from=all",
	);
	await titleLink.click();
	await expect(page).toHaveURL(
		"/lobehub/lobe-chat/discussions/64?scope=repo&items=lobehub%2Flobe-chat&from=all",
	);
	await expect(page.getByRole("tab", { name: "润色" })).toHaveAttribute(
		"aria-selected",
		"true",
	);

	await page.getByRole("button", { name: "返回工作台" }).click();
	await expect(page).toHaveURL("/focus/repo/lobehub/lobe-chat");
	await expect(
		page.locator(
			'[data-dashboard-scope-summary="repo"][data-dashboard-scope-summary-layout="desktop"]',
		),
	).toBeVisible();
});

test("unauthenticated discussion deep link falls back to landing instead of a public announcement page", async ({
	page,
}) => {
	await installPasskeyBrowserMock(page);
	await installAnonymousAnnouncementMocks(page);

	await page.goto("/lobehub/lobe-chat/discussions/64");

	await expect(
		page.getByRole("link", { name: "使用 GitHub 登录" }),
	).toBeVisible();
	await expect(
		page.getByRole("heading", {
			name: "集中查看与你相关的 GitHub 动态",
		}),
	).toBeVisible();
	await expect(page.getByRole("button", { name: "返回工作台" })).toHaveCount(0);
});
