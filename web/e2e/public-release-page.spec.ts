import { expect, test, type Page, type Route } from "@playwright/test";
import { buildMockMeResponse } from "./mockApi";

const EMBEDDED_FRONTEND_VERSION = "v0.1.0";
const EMBEDDED_FRONTEND_VERSION_RELEASE_HREF = `/public/IvanLi-CN/octo-rill/releases/tag/${EMBEDDED_FRONTEND_VERSION}`;

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function repoAvatarDataUrl(label = "OR") {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 96 96"><rect width="96" height="96" rx="48" fill="#4f6a98"/><text x="48" y="58" font-family="Inter,Arial,sans-serif" font-size="34" font-weight="700" text-anchor="middle" fill="white">${label}</text></svg>`,
	)}`;
}

function releaseItem(index: number, overrides: Record<string, unknown> = {}) {
	const tag = `v2.${7 - index}.0`;
	return {
		release_id: `public-release-${index}`,
		repo_full_name: "octo-rill/example",
		repo_visual: {
			owner_avatar_url: repoAvatarDataUrl(),
			open_graph_image_url: null,
			uses_custom_open_graph_image: false,
		},
		tag_name: tag,
		previous_tag_name: `v2.${6 - index}.0`,
		name: `${tag} public release endpoints`,
		body:
			"## Changes\n\n" +
			Array.from({ length: 12 }, (_, line) => {
				return `- Public release regression row ${index + 1}.${line + 1}`;
			}).join("\n"),
		html_url: `https://github.com/octo-rill/example/releases/tag/${tag}`,
		published_at: `2026-05-${String(20 - index).padStart(2, "0")}T08:00:00Z`,
		is_prerelease: 0,
		is_draft: 0,
		translated: {
			lang: "zh-CN",
			status: "ready",
			title: `${tag} 公开 Release`,
			summary: "公开 Release 页面回归。",
		},
		smart: {
			lang: "zh-CN",
			status: "ready",
			title: "公开更新记录入口",
			summary: "公开页面复用 Release 卡片并保留内容切换。",
		},
		...overrides,
	};
}

async function installBaseApiMocks(
	page: Page,
	publicHandler: (route: Route, url: URL) => Promise<void> | void,
	options: { authenticated?: boolean; reactionTokenUsable?: boolean } = {},
) {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname.startsWith("/api/public/repos/")) {
			return publicHandler(route, url);
		}

		if (req.method() === "GET" && pathname === "/api/me") {
			if (options.authenticated) {
				return json(
					route,
					buildMockMeResponse({
						id: "public-route-auth-user",
						github_user_id: 4242,
						login: "octo",
						name: "Octo",
						avatar_url: null,
						email: null,
						is_admin: false,
					}),
				);
			}
			return json(
				route,
				{ error: { code: "unauthorized", message: "unauthorized" } },
				401,
			);
		}

		if (req.method() === "GET" && pathname === "/api/reaction-token/status") {
			return json(route, {
				configured: options.reactionTokenUsable === true,
				masked_token:
					options.reactionTokenUsable === true ? "ghp_****_test" : null,
				check: {
					state: options.reactionTokenUsable === true ? "valid" : "idle",
					message: null,
					checked_at:
						options.reactionTokenUsable === true
							? "2026-07-11T00:00:00Z"
							: null,
				},
				owner: null,
			});
		}

		if (req.method() === "POST" && pathname === "/api/feed/reactions/refresh") {
			return json(route, { items: [] });
		}

		if (req.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "2.7.0" });
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

async function expectPublicChrome(page: Page, owner: string, repo: string) {
	const headerLink = page.locator("header a", { hasText: "GitHub" });
	await expect(headerLink).toHaveAttribute(
		"href",
		`https://github.com/${owner}/${repo}/releases`,
	);
	await expect(headerLink.locator(".lucide-external-link")).toHaveCount(1);
	await expect(
		headerLink.locator('[data-auth-provider-icon="github"]'),
	).toHaveCount(0);

	const footerLink = page.locator("footer a", { hasText: "GitHub" });
	await expect(footerLink).toHaveAttribute(
		"href",
		`https://github.com/${owner}/${repo}`,
	);
	await expect(
		footerLink.locator('[data-auth-provider-icon="github"]'),
	).toHaveCount(1);
	await expect(footerLink.locator(".lucide-external-link")).toHaveCount(0);

	const versionLink = page.getByRole("link", {
		name: `Version ${EMBEDDED_FRONTEND_VERSION}`,
	});
	await expect(versionLink).toBeVisible();
	await expect(versionLink).toHaveAttribute(
		"href",
		EMBEDDED_FRONTEND_VERSION_RELEASE_HREF,
	);
}

async function expectNoHorizontalOverflow(page: Page) {
	const overflow = await page.evaluate(() => {
		return (
			document.documentElement.scrollWidth -
			document.documentElement.clientWidth
		);
	});
	expect(overflow).toBeLessThanOrEqual(1);
}

test("public release pending page hides backend retry details", async ({
	page,
}) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await installBaseApiMocks(page, (route) => {
		return json(
			route,
			{
				status: "pending_sync",
				message:
					"Release data is being prepared. Retry after the suggested delay.",
				reason: "repository_registered_release_sync_pending",
				retry_after_seconds: 60,
				repo_full_name: "octo-rill/example",
				last_requested_at: "2026-05-04T08:05:00Z",
			},
			202,
		);
	});

	await page.goto("/octo-rill/example/releases");

	await expect(page.getByText("Release 数据同步中")).toBeVisible({
		timeout: 15_000,
	});
	await expect(page.getByText("同步中", { exact: true })).toBeVisible({
		timeout: 15_000,
	});
	await expect(page.getByText("约 60s 后重试")).toBeVisible({
		timeout: 15_000,
	});
	await expect(
		page.getByText("Release data is being prepared"),
	).not.toBeVisible();
	await expect(
		page.getByText("repository_registered_release_sync_pending"),
	).not.toBeVisible();
	await expectPublicChrome(page, "octo-rill", "example");
	await expectNoHorizontalOverflow(page);
});

test("public release loading page uses a skeleton instead of loading copy", async ({
	page,
}) => {
	await installBaseApiMocks(page, () => new Promise<Response>(() => {}));

	await page.goto("/octo-rill/example/releases");

	await expect(page.getByTestId("public-release-loading-skeleton")).toBeVisible(
		{ timeout: 15_000 },
	);
	await expect(
		page.getByTestId("public-release-skeleton-block").first(),
	).toBeVisible();
	await expect(page.getByText("正在读取公开 Release")).not.toBeVisible();
	await expect(page.getByText("Release 数据同步中")).not.toBeVisible();
	await expectPublicChrome(page, "octo-rill", "example");
});

test("public release error page shows http status when edge returns an empty error", async ({
	page,
}) => {
	await installBaseApiMocks(page, (route) =>
		route.fulfill({
			status: 554,
			body: "",
		}),
	);

	await page.goto("/octo-rill/example/releases");

	await expect(page.getByText("暂时无法展示")).toBeVisible();
	await expect(
		page.getByText("unknown_error: Request failed (HTTP 554)"),
	).toBeVisible();
	await expectPublicChrome(page, "octo-rill", "example");
});

test("public release list requests six cached releases before loading more", async ({
	page,
}) => {
	const seenQueries: string[] = [];
	const items = Array.from({ length: 8 }, (_, index) => releaseItem(index));
	await installBaseApiMocks(page, (route, url) => {
		seenQueries.push(url.search);
		const cursor = url.searchParams.get("cursor");
		const start = cursor ? 6 : 0;
		const limit = Number(url.searchParams.get("limit") ?? "0");
		return json(route, {
			status: "ready",
			repo_full_name: "octo-rill/example",
			next_cursor: start + limit < items.length ? "next|6" : null,
			items: items.slice(start, start + limit),
		});
	});

	await page.goto("/octo-rill/example/releases");

	await expect(
		page.getByRole("heading", { name: "octo-rill/example" }),
	).toBeVisible();
	await expect(page.locator("[data-release-id]").first()).toHaveAttribute(
		"data-highlighted",
		"false",
	);
	expect(new URLSearchParams(seenQueries[0]).get("limit")).toBe("6");
	await expect(
		page.getByRole("button", { name: "原文" }).first(),
	).toBeVisible();
	await expect(page.getByText("公开更新记录入口").first()).toBeVisible();
	await page.getByRole("button", { name: "原文" }).first().click();
	await expect(
		page
			.getByRole("heading", {
				name: /v2\.\d+\.0 public release endpoints/,
			})
			.first(),
	).toBeVisible();
	expect(seenQueries.some((query) => query.includes("cursor=next%7C6"))).toBe(
		true,
	);
	await expectPublicChrome(page, "octo-rill", "example");
});

test("public release header keeps the title and global lane selector responsive", async ({
	page,
}) => {
	const items = Array.from({ length: 2 }, (_, index) => releaseItem(index));
	let reactionTokenStatusRequests = 0;
	await installBaseApiMocks(page, (route) =>
		json(route, {
			status: "ready",
			repo_full_name: "octo-rill/example",
			next_cursor: null,
			items,
		}),
	);
	await page.route("**/api/reaction-token/status", async (route) => {
		reactionTokenStatusRequests += 1;
		return json(route, { error: { code: "unexpected" } }, 500);
	});

	await page.setViewportSize({ width: 1440, height: 1000 });
	await page.goto("/octo-rill/example/releases");

	const wordmark = page.getByRole("img", { name: "OctoRill" });
	const title = page.getByRole("heading", { name: "octo-rill/example" });
	const pageLane = page.getByTestId("public-release-page-lane");
	await page.waitForFunction(() =>
		Array.from(document.images)
			.filter((img) => img.alt === "OctoRill")
			.every((img) => img.complete && img.naturalWidth > 0),
	);
	await expect(wordmark).toBeVisible();
	await expect(title).toBeVisible();
	await expect(pageLane).toBeVisible();
	expect(
		await page
			.locator(
				"[data-testid='public-release-title-band'] [data-repo-visual-kind='owner_avatar']",
			)
			.count(),
	).toBe(1);
	expect(
		await page.locator("[data-release-id] [data-repo-visual-slot]").count(),
	).toBe(0);
	expect(await page.locator("[data-feed-lane-trigger]").count()).toBe(0);
	expect(await page.locator("[data-feed-mobile-github-link]").count()).toBe(0);
	expect(
		await page.locator("header").getByRole("link", { name: "GitHub" }).count(),
	).toBe(1);
	expect(
		await page
			.locator("[data-release-id]")
			.getByRole("link", { name: "GitHub" })
			.count(),
	).toBe(0);
	expect(reactionTokenStatusRequests).toBe(0);

	const [desktopWordmark, desktopTitle, desktopLane] = await Promise.all([
		wordmark.boundingBox(),
		title.boundingBox(),
		pageLane.boundingBox(),
	]);
	expect(desktopWordmark).not.toBeNull();
	expect(desktopTitle).not.toBeNull();
	expect(desktopLane).not.toBeNull();
	expect(desktopWordmark?.height).toBe(32);
	expect(desktopLane?.y).toBeGreaterThanOrEqual(desktopTitle?.y ?? 0);
	expect(desktopLane?.y).toBeLessThan(
		(desktopTitle?.y ?? 0) + (desktopTitle?.height ?? 0),
	);
	await expectNoHorizontalOverflow(page);

	await page.setViewportSize({ width: 390, height: 844 });
	await expect
		.poll(async () => (await wordmark.boundingBox())?.height ?? 0)
		.toBe(28);
	const [mobileTitle, mobileLane] = await Promise.all([
		title.boundingBox(),
		pageLane.boundingBox(),
	]);
	expect(mobileTitle).not.toBeNull();
	expect(mobileLane).not.toBeNull();
	expect(mobileLane?.y).toBeGreaterThanOrEqual(
		(mobileTitle?.y ?? 0) + (mobileTitle?.height ?? 0),
	);
	await expectNoHorizontalOverflow(page);
});

test("public release reactions require an authenticated session with a usable PAT", async ({
	page,
}) => {
	const items = Array.from({ length: 101 }, (_, index) => releaseItem(index));
	let reactionTokenStatusRequests = 0;
	let toggledReactionRequests = 0;
	const reactionRefreshBatchSizes: number[] = [];
	await installBaseApiMocks(
		page,
		(route) =>
			json(route, {
				status: "ready",
				repo_full_name: "octo-rill/example",
				next_cursor: null,
				items,
			}),
		{ authenticated: true, reactionTokenUsable: true },
	);
	await page.route("**/api/reaction-token/status", async (route) => {
		reactionTokenStatusRequests += 1;
		return json(route, {
			configured: true,
			masked_token: "ghp_****_test",
			check: {
				state: "valid",
				message: null,
				checked_at: "2026-07-11T00:00:00Z",
			},
			owner: null,
		});
	});
	await page.route("**/api/feed/reactions/refresh", async (route) => {
		const body = route.request().postDataJSON() as { release_ids: string[] };
		reactionRefreshBatchSizes.push(body.release_ids.length);
		return json(route, {
			items: body.release_ids.map((releaseId) => ({
				release_id: releaseId,
				reactions: {
					counts: {
						plus1: 0,
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
			})),
		});
	});
	await page.route("**/api/release/reactions/toggle", async (route) => {
		toggledReactionRequests += 1;
		const body = route.request().postDataJSON() as {
			release_id: string;
			content: string;
		};
		return json(route, {
			release_id: body.release_id,
			reactions: {
				counts: {
					plus1: body.content === "plus1" ? 1 : 0,
					laugh: 0,
					heart: 0,
					hooray: 0,
					rocket: 0,
					eyes: 0,
				},
				viewer: {
					plus1: body.content === "plus1",
					laugh: false,
					heart: false,
					hooray: false,
					rocket: false,
					eyes: false,
				},
				status: "ready",
			},
		});
	});

	await page.goto("/octo-rill/example/releases");
	const firstRelease = page.getByTestId("public-release-item-public-release-0");
	await expect(firstRelease).toBeVisible();
	await expect.poll(() => reactionTokenStatusRequests).toBe(1);
	await expect
		.poll(() => reactionRefreshBatchSizes.reduce((sum, size) => sum + size, 0))
		.toBe(101);
	expect(reactionRefreshBatchSizes).toEqual([100, 1]);
	await expect(firstRelease.locator("[data-reaction-trigger]")).toHaveCount(6);
	const plusOne = firstRelease.locator("[data-reaction-trigger='plus1']");
	await plusOne.click();
	await expect.poll(() => toggledReactionRequests).toBe(1);
	await expect(plusOne).toHaveAttribute("aria-pressed", "true");
});

test("public release retries a transient reaction refresh failure", async ({
	page,
}) => {
	const items = [releaseItem(0)];
	let refreshRequests = 0;
	await installBaseApiMocks(
		page,
		(route) =>
			json(route, {
				status: "ready",
				repo_full_name: "octo-rill/example",
				next_cursor: null,
				items,
			}),
		{ authenticated: true, reactionTokenUsable: true },
	);
	await page.route("**/api/feed/reactions/refresh", async (route) => {
		refreshRequests += 1;
		if (refreshRequests === 1) {
			return json(route, { error: { code: "temporary" } }, 500);
		}
		const body = route.request().postDataJSON() as { release_ids: string[] };
		return json(route, {
			items: body.release_ids.map((releaseId) => ({
				release_id: releaseId,
				reactions: {
					counts: {
						plus1: 0,
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
			})),
		});
	});

	await page.goto("/octo-rill/example/releases");
	const firstRelease = page.getByTestId("public-release-item-public-release-0");
	await expect(firstRelease).toBeVisible();
	await expect(firstRelease.locator("[data-reaction-trigger]")).toHaveCount(6, {
		timeout: 5_000,
	});
	expect(refreshRequests).toBe(2);
});

test("public release hides reactions without a usable PAT", async ({
	page,
}) => {
	const items = Array.from({ length: 2 }, (_, index) => releaseItem(index));
	await installBaseApiMocks(
		page,
		(route) =>
			json(route, {
				status: "ready",
				repo_full_name: "octo-rill/example",
				next_cursor: null,
				items,
			}),
		{ authenticated: true, reactionTokenUsable: false },
	);

	await page.goto("/octo-rill/example/releases");
	const firstRelease = page.getByTestId("public-release-item-public-release-0");
	await expect(firstRelease).toBeVisible();
	await expect(firstRelease.locator("[data-reaction-trigger]")).toHaveCount(0);
});

test("public release hides reactions outside the viewer's feed visibility", async ({
	page,
}) => {
	const items = Array.from({ length: 2 }, (_, index) => releaseItem(index));
	await installBaseApiMocks(
		page,
		(route) =>
			json(route, {
				status: "ready",
				repo_full_name: "octo-rill/example",
				next_cursor: null,
				items,
			}),
		{ authenticated: true, reactionTokenUsable: true },
	);

	await page.goto("/octo-rill/example/releases");
	const firstRelease = page.getByTestId("public-release-item-public-release-0");
	await expect(firstRelease).toBeVisible();
	await expect(firstRelease.locator("[data-reaction-trigger]")).toHaveCount(0);
});

test("public owned cached repo shows ready list instead of pending sync", async ({
	page,
}) => {
	await installBaseApiMocks(page, (route) => {
		return json(route, {
			status: "ready",
			repo_full_name: "IvanLi-CN/tuckmark",
			next_cursor: null,
			items: [
				releaseItem(0, {
					repo_full_name: "IvanLi-CN/tuckmark",
					tag_name: "v0.2.0-preview.11",
					previous_tag_name: "v0.1.2-preview.8",
					name: "v0.2.0-preview.11",
					html_url:
						"https://github.com/IvanLi-CN/tuckmark/releases/tag/v0.2.0-preview.11",
					body: "Tuckmark release v0.2.0-preview.11",
				}),
			],
		});
	});

	await page.goto("/IvanLi-CN/tuckmark/releases");

	await expect(
		page.getByRole("heading", { name: "IvanLi-CN/tuckmark" }),
	).toBeVisible();
	await expect(
		page.getByRole("link", { name: "公开更新记录入口" }),
	).toHaveAttribute(
		"href",
		"/public/IvanLi-CN/tuckmark/releases/tag/v0.2.0-preview.11",
	);
	await expect(page.getByText("Release 数据同步中")).not.toBeVisible();
	await expectPublicChrome(page, "IvanLi-CN", "tuckmark");
});

test("public release typed discrete highlight keeps partial targets and replaces active URL", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1280, height: 720 });
	const seenQueries: URL[] = [];
	const items = Array.from({ length: 8 }, (_, index) => releaseItem(index));
	await installBaseApiMocks(page, (route, url) => {
		if (url.pathname.endsWith("/releases/content")) {
			const requestedIds = (url.searchParams.get("release_ids") ?? "").split(
				",",
			);
			return json(route, {
				items: items
					.filter((item) => requestedIds.includes(item.release_id))
					.map((item) => ({
						...item,
						body: null,
						smart: null,
						is_highlighted: false,
						is_active_highlight: false,
					})),
			});
		}
		seenQueries.push(url);
		const selectors = url.searchParams.getAll("highlight");
		const resolved = [
			{
				selector: "tag:v2.2.0",
				release_id: "public-release-5",
				tag_name: "v2.2.0",
				ordinal: 6,
			},
			{
				selector: "id:public-release-1",
				release_id: "public-release-1",
				tag_name: "v2.6.0",
				ordinal: 2,
			},
		].sort((left, right) => left.ordinal - right.ordinal);
		const resolvedIds = resolved.map((target) => target.release_id);
		const active = resolved[0];
		return json(route, {
			status: "ready",
			repo_full_name: "octo-rill/example",
			next_cursor: null,
			items: items.map((item) => ({
				...item,
				is_highlighted: resolvedIds.includes(item.release_id),
				is_active_highlight: item.release_id === active.release_id,
			})),
			highlight: {
				mode: "discrete",
				status: "partial",
				requested: selectors,
				resolved,
				unresolved: ["tag:missing-release"],
				total: 2,
				active_release_id: active.release_id,
				active_index: 1,
			},
			segments: [
				{
					first_release_id: items[0].release_id,
					last_release_id: items[items.length - 1].release_id,
				},
			],
			gaps: [],
		});
	});

	await page.goto(
		"/public/octo-rill/example/releases?highlight=tag%3Av2.2.0&highlight=id%3Apublic-release-1&highlight=tag%3Amissing-release",
	);

	await expect(
		page.getByTestId("public-release-item-public-release-5"),
	).toHaveAttribute("data-highlighted", "true");
	await expect(
		page.getByTestId("public-release-item-public-release-1"),
	).toHaveAttribute("data-highlighted", "true");
	await expect(
		page.getByTestId("public-release-highlight-unresolved"),
	).toBeVisible();
	expect(seenQueries[0].searchParams.getAll("highlight")).toEqual([
		"tag:v2.2.0",
		"id:public-release-1",
		"tag:missing-release",
	]);
	expect(seenQueries[0].searchParams.get("cursor")).toBeNull();
	await expect(
		page.getByTestId("public-release-highlight-navigation"),
	).toContainText("1 / 2");
	await page.getByTitle("下一条高亮记录").click();
	await expect(page).toHaveURL(/highlight_active=tag%3Av2.2.0/);
	await page
		.getByTestId("public-release-page-lane")
		.getByRole("button", { name: "翻译" })
		.click();
	await expect(
		page.getByTestId("public-release-item-public-release-5"),
	).toHaveAttribute("data-highlighted", "true");
	await page
		.getByTestId("public-release-page-lane")
		.getByRole("button", { name: "润色" })
		.click();
	await expect(
		page.getByText("公开页面复用 Release 卡片并保留内容切换。").first(),
	).toBeVisible();
	await expectNoHorizontalOverflow(page);
});

test("public release pagination preserves the user-selected active highlight", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1280, height: 720 });
	await page.addInitScript(() => {
		class InertIntersectionObserver {
			observe() {}
			disconnect() {}
		}
		window.IntersectionObserver =
			InertIntersectionObserver as unknown as typeof IntersectionObserver;
	});
	const seenQueries: URL[] = [];
	const items = Array.from({ length: 8 }, (_, index) => releaseItem(index));
	const resolved = [
		{
			selector: "id:public-release-1",
			release_id: "public-release-1",
			tag_name: items[1].tag_name,
			ordinal: 2,
		},
		{
			selector: "tag:v2.2.0",
			release_id: "public-release-5",
			tag_name: items[5].tag_name,
			ordinal: 6,
		},
	];
	await installBaseApiMocks(page, (route, url) => {
		seenQueries.push(url);
		const active =
			resolved.find(
				(target) =>
					target.selector === url.searchParams.get("highlight_active"),
			) ?? resolved[0];
		const isPagination = url.searchParams.has("cursor");
		const responseItems = isPagination
			? [items[6]]
			: resolved
					.map((target) =>
						items.find((item) => item.release_id === target.release_id),
					)
					.filter((item): item is (typeof items)[number] => item !== undefined);
		return json(route, {
			status: "ready",
			repo_full_name: "octo-rill/example",
			next_cursor: isPagination ? null : "older|2",
			items: responseItems.map((item) => ({
				...item,
				is_highlighted: resolved.some(
					(target) => target.release_id === item.release_id,
				),
				is_active_highlight: item.release_id === active.release_id,
			})),
			highlight: {
				mode: "discrete",
				status: "complete",
				requested: resolved.map((target) => target.selector),
				resolved,
				unresolved: [],
				total: 2,
				active_release_id: active.release_id,
				active_index: resolved.indexOf(active) + 1,
			},
			segments: [
				{
					first_release_id: responseItems[0]?.release_id,
					last_release_id: responseItems.at(-1)?.release_id,
				},
			],
			gaps: [],
		});
	});

	await page.goto(
		"/octo-rill/example/releases?highlight=id%3Apublic-release-1&highlight=tag%3Av2.2.0",
	);
	await expect(
		page.getByTestId("public-release-highlight-navigation"),
	).toContainText("1 / 2");

	await page.getByTitle("下一条高亮记录").click();
	await expect(page).toHaveURL(/highlight_active=tag%3Av2.2.0/);
	await expect(
		page.getByTestId("public-release-item-public-release-5"),
	).toHaveAttribute("data-active-highlight", "true");

	await page.getByRole("button", { name: "更多" }).click();
	await expect
		.poll(() =>
			seenQueries.find((url) => url.searchParams.get("cursor") === "older|2"),
		)
		.not.toBeUndefined();
	const paginationQuery = seenQueries.find(
		(url) => url.searchParams.get("cursor") === "older|2",
	);
	expect(paginationQuery?.searchParams.get("highlight_active")).toBe(
		"tag:v2.2.0",
	);
	await expect(
		page.getByTestId("public-release-highlight-navigation"),
	).toContainText("2 / 2");
	await expect(
		page.getByTestId("public-release-item-public-release-5"),
	).toHaveAttribute("data-active-highlight", "true");
});

test("public release typed range uses virtual rows and loads both directions", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1280, height: 720 });
	const seenQueries: URL[] = [];
	const items = Array.from({ length: 20 }, (_, index) => releaseItem(index));
	await installBaseApiMocks(page, (route, url) => {
		seenQueries.push(url);
		const start = 0;
		const end = 19;
		const cursor = url.searchParams.get("cursor");
		const direction = url.searchParams.get("direction") ?? "older";
		const offset = cursor ? Number(cursor.split("|").at(-1)) : 4;
		const limit = Math.min(Number(url.searchParams.get("limit") ?? "12"), 12);
		const pageItems =
			direction === "newer"
				? items.slice(Math.max(start, offset - limit), offset)
				: items.slice(offset, Math.min(end + 1, offset + limit));
		const nextOffset = offset + pageItems.length;
		return json(route, {
			status: "ready",
			repo_full_name: "octo-rill/example",
			next_cursor:
				direction === "older" && nextOffset <= end
					? `range|${nextOffset}`
					: null,
			previous_cursor:
				direction === "newer"
					? Math.max(start, offset - pageItems.length) > start
						? `range|${Math.max(start, offset - pageItems.length)}`
						: null
					: `range|${offset}`,
			items: pageItems.map((item) => ({
				...item,
				is_highlighted: true,
				is_active_highlight: item.release_id === "public-release-4",
			})),
			highlight: {
				mode: "range",
				status: "complete",
				requested: ["tag:v2.7.0", "id:public-release-19"],
				resolved: [
					{
						selector: "tag:v2.7.0",
						release_id: "public-release-0",
						tag_name: "v2.7.0",
						ordinal: 1,
					},
					{
						selector: "id:public-release-19",
						release_id: "public-release-19",
						tag_name: items[19].tag_name,
						ordinal: 20,
					},
				],
				unresolved: [],
				total: 20,
				active_release_id: "public-release-4",
				active_index: 5,
			},
			segments: [
				{
					first_release_id: pageItems[0]?.release_id,
					last_release_id: pageItems.at(-1)?.release_id,
				},
			],
			gaps: [],
		});
	});

	await page.goto(
		"/octo-rill/example/releases?highlight_start=tag%3Av2.7.0&highlight_end=id%3Apublic-release-19&highlight_active=id%3Apublic-release-4",
	);
	await expect(
		page.getByTestId("public-release-item-public-release-4"),
	).toHaveAttribute("data-highlighted", "true");
	await expect(page.getByTestId("public-release-virtual-list")).toBeVisible();
	expect(await page.locator("[data-release-id]").count()).toBeLessThan(
		items.length,
	);

	await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
	await expect.poll(() => seenQueries.length).toBeGreaterThan(1);
	await expect
		.poll(async () => {
			await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
			return await page
				.getByTestId("public-release-item-public-release-19")
				.count();
		})
		.toBe(1);
	await expect(
		page.getByTestId("public-release-item-public-release-19"),
	).toHaveAttribute("data-highlighted", "true");
	expect(
		seenQueries.slice(1).every((url) => {
			return (
				url.searchParams.get("highlight_start") === "tag:v2.7.0" &&
				url.searchParams.get("highlight_end") === "id:public-release-19"
			);
		}),
	).toBe(true);

	await page.evaluate(() => window.scrollTo(0, 0));
	await expect
		.poll(() =>
			seenQueries.some((url) => url.searchParams.get("direction") === "newer"),
		)
		.toBe(true);
	const newerQuery = seenQueries.find(
		(url) => url.searchParams.get("direction") === "newer",
	);
	expect(newerQuery?.searchParams.get("cursor")).toBe("range|4");
	expect(newerQuery?.searchParams.get("highlight_start")).toBe("tag:v2.7.0");
	expect(newerQuery?.searchParams.get("highlight_end")).toBe(
		"id:public-release-19",
	);
	await expectNoHorizontalOverflow(page);
});

test("centered range navigation preserves the absolute highlight index", async ({
	page,
}) => {
	const items = Array.from({ length: 40 }, (_, index) => releaseItem(index));
	await installBaseApiMocks(page, (route, url) => {
		const pageItems = items.slice(5, 35);
		const activeReleaseId =
			url.searchParams.get("highlight_active")?.replace(/^id:/, "") ??
			"public-release-19";
		const activeIndex =
			items.findIndex((item) => item.release_id === activeReleaseId) + 1;
		return json(route, {
			status: "ready",
			repo_full_name: "octo-rill/example",
			next_cursor: null,
			previous_cursor: null,
			items: pageItems.map((item) => ({
				...item,
				is_highlighted: true,
				is_active_highlight: item.release_id === activeReleaseId,
			})),
			highlight: {
				mode: "range",
				status: "complete",
				requested: ["id:public-release-0", "id:public-release-39"],
				resolved: [
					{
						selector: "id:public-release-0",
						release_id: "public-release-0",
						tag_name: items[0].tag_name,
						ordinal: 1,
					},
					{
						selector: "id:public-release-39",
						release_id: "public-release-39",
						tag_name: items[39].tag_name,
						ordinal: 40,
					},
				],
				unresolved: [],
				total: 40,
				active_release_id: activeReleaseId,
				active_index: activeIndex,
			},
			segments: [
				{
					first_release_id: pageItems[0].release_id,
					last_release_id: pageItems.at(-1)?.release_id,
				},
			],
			gaps: [],
		});
	});

	await page.goto(
		"/octo-rill/example/releases?highlight_start=id%3Apublic-release-0&highlight_end=id%3Apublic-release-39&highlight_active=id%3Apublic-release-19",
	);
	await expect(
		page.getByTestId("public-release-highlight-navigation"),
	).toContainText("20 / 40");
	await page.getByTitle("下一条高亮记录").click();
	await expect(
		page.getByTestId("public-release-highlight-navigation"),
	).toContainText("21 / 40");
	await expect(page).toHaveURL(/highlight_active=id%3Apublic-release-20/);
});

test("public release detail keeps the shared chrome stable", async ({
	page,
}) => {
	const detailRequests: URL[] = [];
	await page.setViewportSize({ width: 390, height: 844 });
	await installBaseApiMocks(page, (route, url) => {
		detailRequests.push(url);
		return json(route, {
			...releaseItem(0, {
				name: "公开更新记录入口",
				body: "这次版本把公开仓库的 Release 列表与详情开放为可直接分享的页面，并提供可重试的 REST API。",
				translated: {
					lang: "zh-CN",
					status: "ready",
					title: "公开更新记录入口",
					summary:
						"这次版本把公开仓库的 Release 列表与详情开放为可直接分享的页面，并提供可重试的 REST API。",
				},
				smart: {
					lang: "zh-CN",
					status: "ready",
					title: "公开更新记录入口",
					summary:
						"这次版本把公开仓库的 Release 列表与详情开放为可直接分享的页面，并提供可重试的 REST API。",
				},
			}),
		});
	});

	await page.goto("/octo-rill/example/releases/tag/v2.7.0");

	await expect(
		page.getByRole("heading", { name: "公开更新记录入口" }),
	).toBeVisible();
	await expect(page.getByRole("tab", { name: "润色" })).toBeVisible();
	expect(detailRequests).toHaveLength(1);
	expect(detailRequests[0]?.searchParams.get("content")).toBe("all");
	await page.getByRole("tab", { name: "翻译" }).click();
	await expect(page.getByRole("tab", { name: "翻译" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expectPublicChrome(page, "octo-rill", "example");
	await expectNoHorizontalOverflow(page);
});

test("authenticated footer version link opens the public-only release page", async ({
	page,
}) => {
	const publicRequests: string[] = [];
	const privateDetailRequests: string[] = [];
	await installBaseApiMocks(
		page,
		(route, url) => {
			publicRequests.push(`${url.pathname}${url.search}`);
			return json(route, {
				...releaseItem(0, {
					repo_full_name: "IvanLi-CN/octo-rill",
					tag_name: EMBEDDED_FRONTEND_VERSION,
					name: "Footer public release target",
					translated: {
						lang: "zh-CN",
						status: "ready",
						title: "Footer public release target",
						summary: "Footer version links must stay on the public page.",
					},
					smart: {
						lang: "zh-CN",
						status: "ready",
						title: "Footer public release target",
						summary: "Footer version links must stay on the public page.",
					},
				}),
			});
		},
		{ authenticated: true },
	);
	await page.route("**/api/repos/**/releases/tag/**/detail", async (route) => {
		privateDetailRequests.push(route.request().url());
		return json(
			route,
			{
				error: {
					code: "unexpected_private_detail_request",
					message: "footer version link should not open dashboard detail",
				},
			},
			500,
		);
	});

	await page.goto("/public/octo-rill/example/releases/tag/v2.7.0");
	await page
		.getByRole("link", { name: `Version ${EMBEDDED_FRONTEND_VERSION}` })
		.click();

	await expect(page).toHaveURL(
		new RegExp(
			`/public/IvanLi-CN/octo-rill/releases/tag/${EMBEDDED_FRONTEND_VERSION}$`,
		),
	);
	await expect(
		page.getByRole("heading", { name: "Footer public release target" }),
	).toBeVisible();
	expect(
		publicRequests.some((request) =>
			request.startsWith(
				`/api/public/repos/IvanLi-CN/octo-rill/releases/tag/${EMBEDDED_FRONTEND_VERSION}`,
			),
		),
	).toBe(true);
	expect(privateDetailRequests).toEqual([]);
});
