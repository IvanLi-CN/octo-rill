import { expect, test, type Page, type Route } from "@playwright/test";

const EMBEDDED_FRONTEND_VERSION = "v0.1.0";
const EMBEDDED_FRONTEND_VERSION_RELEASE_HREF = `/IvanLi-CN/octo-rill/releases/tag/${EMBEDDED_FRONTEND_VERSION}`;

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
		published_at: "2026-05-04T08:00:00Z",
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
) {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname.startsWith("/api/public/repos/")) {
			return publicHandler(route, url);
		}

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				{ error: { code: "unauthorized", message: "unauthorized" } },
				401,
			);
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

	await expect(page.getByText("正在准备 Release 数据")).toBeVisible();
	await expect(page.getByText("同步排队中")).toBeVisible();
	await expect(page.getByText("约 60s 后重试")).toBeVisible();
	await expect(
		page.getByText("Release data is being prepared"),
	).not.toBeVisible();
	await expect(
		page.getByText("repository_registered_release_sync_pending"),
	).not.toBeVisible();
	await expectPublicChrome(page, "octo-rill", "example");
	await expectNoHorizontalOverflow(page);
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
	expect(new URLSearchParams(seenQueries[0]).get("limit")).toBe("6");
	await expect(
		page.getByRole("button", { name: "原文" }).first(),
	).toBeVisible();
	await expect(page.getByText("v2.7.0 public release endpoints")).toBeVisible();
	await expect(page.getByText("v2.0.0 public release endpoints")).toBeVisible();
	expect(seenQueries.some((query) => query.includes("cursor=next%7C6"))).toBe(
		true,
	);
	await expectPublicChrome(page, "octo-rill", "example");
});

test("public release detail keeps the shared chrome stable", async ({
	page,
}) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await installBaseApiMocks(page, (route) => {
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
	await expectPublicChrome(page, "octo-rill", "example");
	await expectNoHorizontalOverflow(page);
});
