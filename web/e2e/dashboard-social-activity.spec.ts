import { type Route, expect, test } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

test("dashboard renders mixed social activity in all tab and filters stars/followers tabs", async ({
	page,
}) => {
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
				items: [
					{
						kind: "release",
						ts: "2026-04-10T12:00:00Z",
						id: "20001",
						repo_full_name: "owner/repo",
						title: "Release 20001",
						body: "hello",
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/owner/repo/releases/tag/v20001",
						unread: null,
						translated: null,
						smart: null,
						reactions: null,
					},
					{
						kind: "repo_star_received",
						ts: "2026-04-10T11:30:00Z",
						id: "star-1",
						repo_full_name: "owner/repo",
						title: null,
						body: null,
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/octocat",
						unread: null,
						actor: {
							login: "octocat",
							avatar_url: null,
							html_url: "https://github.com/octocat",
						},
						translated: null,
						smart: null,
						reactions: null,
					},
					{
						kind: "follower_received",
						ts: "2026-04-10T11:00:00Z",
						id: "follow-1",
						repo_full_name: null,
						title: null,
						body: null,
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/monalisa",
						unread: null,
						actor: {
							login: "monalisa",
							avatar_url: null,
							html_url: "https://github.com/monalisa",
						},
						translated: null,
						smart: null,
						reactions: null,
					},
				],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
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

	await expect(page.getByRole("tab", { name: "被加星" })).toBeVisible();
	await expect(page.getByRole("tab", { name: "被关注" })).toBeVisible();
	await expect(page.getByText("octocat", { exact: true })).toBeVisible();
	await expect(page.getByText("monalisa", { exact: true })).toBeVisible();
	await expect(
		page.getByText("owner/repo", { exact: true }).nth(1),
	).toBeVisible();
	await expect(page.getByText("标星", { exact: true })).toBeVisible();
	await expect(page.getByText("关注", { exact: true })).toBeVisible();
	await expect(
		page.locator(
			'[data-social-card-kind="repo_star_received"] a[href^="https://github.com/"]',
		),
	).toHaveCount(2);
	await expect(
		page.locator(
			'[data-social-card-kind="follower_received"] a[href^="https://github.com/"]',
		),
	).toHaveCount(1);

	await page.getByRole("tab", { name: "被加星" }).click();
	await expect(page.getByText("octocat", { exact: true })).toBeVisible();
	await expect(page.getByText("monalisa", { exact: true })).toHaveCount(0);
	await expect(page.getByText("owner/repo", { exact: true })).toBeVisible();
	await expect(page.getByText("标星", { exact: true })).toBeVisible();
	await expect(
		page.locator(
			'[data-social-card-kind="repo_star_received"] a[href^="https://github.com/"]',
		),
	).toHaveCount(2);

	await page.getByRole("tab", { name: "被关注" }).click();
	await expect(page.getByText("monalisa", { exact: true })).toBeVisible();
	await expect(page.getByText("octo", { exact: true })).toBeVisible();
	await expect(page.getByText("关注", { exact: true })).toBeVisible();
	await expect(
		page.getByRole("heading", { name: "Release 20001" }),
	).toHaveCount(0);
	await expect(page.getByText("octocat", { exact: true })).toHaveCount(0);
	await expect(
		page.locator(
			'[data-social-card-kind="follower_received"] a[href^="https://github.com/"]',
		),
	).toHaveCount(1);
});

test("social activity cards fall back to placeholder avatar when image fails", async ({
	page,
}) => {
	await page.route("**/avatars/**", async (route) => {
		await route.fulfill({ status: 404, body: "missing" });
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
				items: [
					{
						kind: "follower_received",
						ts: "2026-04-10T11:00:00Z",
						id: "follow-1",
						repo_full_name: null,
						title: null,
						body: null,
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/ghost",
						unread: null,
						actor: {
							login: "ghost",
							avatar_url: `${page.url()}avatars/missing.png`,
							html_url: "https://github.com/ghost",
						},
						translated: null,
						smart: null,
						reactions: null,
					},
				],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
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

	await page.goto("/?tab=followers");
	await expect(
		page.locator('[data-social-avatar-fallback="true"]').first(),
	).toBeVisible();
});

test("switching social tabs clears stale feed items before the next dataset resolves", async ({
	page,
}) => {
	let releaseStarsResponse!: () => void;
	const starsResponseReady = new Promise<void>((resolve) => {
		releaseStarsResponse = resolve;
	});

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname, searchParams } = url;

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
			const types = searchParams.get("types");
			if (types === "stars") {
				await starsResponseReady;
				return json(route, {
					items: [
						{
							kind: "repo_star_received",
							ts: "2026-04-10T12:05:00Z",
							id: "star-new",
							repo_full_name: "owner/repo",
							title: null,
							body: null,
							body_truncated: false,
							subtitle: null,
							reason: null,
							subject_type: null,
							html_url: "https://github.com/octocat-new",
							unread: null,
							actor: {
								login: "octocat-new",
								avatar_url: null,
								html_url: "https://github.com/octocat-new",
							},
							translated: null,
							smart: null,
							reactions: null,
						},
					],
					next_cursor: null,
				});
			}

			return json(route, {
				items: [
					{
						kind: "release",
						ts: "2026-04-10T12:00:00Z",
						id: "20001",
						repo_full_name: "owner/repo",
						title: "Release 20001",
						body: "hello",
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/owner/repo/releases/tag/v20001",
						unread: null,
						translated: null,
						smart: null,
						reactions: null,
					},
					{
						kind: "repo_star_received",
						ts: "2026-04-10T11:30:00Z",
						id: "star-old",
						repo_full_name: "owner/repo",
						title: null,
						body: null,
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/octocat-old",
						unread: null,
						actor: {
							login: "octocat-old",
							avatar_url: null,
							html_url: "https://github.com/octocat-old",
						},
						translated: null,
						smart: null,
						reactions: null,
					},
				],
				next_cursor: "cursor-all-1",
			});
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
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

	await expect(page.getByText("octocat-old", { exact: true })).toBeVisible();

	await page.getByRole("tab", { name: "被加星" }).click();
	await expect(page.getByText("octocat-old", { exact: true })).toHaveCount(0);

	releaseStarsResponse();

	await expect(page.getByText("octocat-new", { exact: true })).toBeVisible();
});
