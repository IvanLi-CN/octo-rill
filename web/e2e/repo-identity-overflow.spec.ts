import { expect, test, type Page, type Route } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

const TWO_LINE_REPO = "acme/release-notes-for-mobile";
const EXTREME_REPO =
	"owner-with-a-very-long-github-handle/repository-name-that-keeps-terminal-tail-2026";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function releaseItem(id: string, repoFullName: string) {
	return {
		kind: "release",
		ts: "2026-09-09T06:33:31Z",
		id,
		repo_full_name: repoFullName,
		repo_visual: null,
		title: "Repository identity overflow regression",
		body: "- The repository name remains identifiable at narrow widths",
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `https://github.com/${repoFullName}/releases/tag/v1.0.0`,
		unread: null,
		actor: null,
		translated: {
			lang: "zh-CN",
			status: "missing",
			title: null,
			summary: null,
		},
		smart: { lang: "zh-CN", status: "missing", title: null, summary: null },
		reactions: null,
	};
}

async function installMocks(page: Page) {
	await page.route("**/api/**", async (route) => {
		const request = route.request();
		const url = new URL(request.url());
		const { pathname } = url;

		if (request.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: "repo-identity-overflow-user",
					github_user_id: 30215105,
					login: "story-viewer",
					name: "Story Viewer",
					avatar_url: null,
					email: null,
					is_admin: false,
				}),
			);
		}

		if (request.method() === "GET" && pathname === "/api/feed") {
			return json(route, {
				items: [
					releaseItem("repo-identity-two-line", TWO_LINE_REPO),
					releaseItem("repo-identity-extreme", EXTREME_REPO),
				],
				next_cursor: null,
			});
		}

		if (request.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (request.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (request.method() === "GET" && pathname === "/api/dashboard/updates") {
			return json(route, {
				token: "repo-identity-overflow-token",
				generated_at: "2026-09-09T06:33:31Z",
				lists: {
					feed: { changed: false, new_count: 0, latest_keys: [] },
					briefs: { changed: false, new_count: 0, latest_keys: [] },
					notifications: { changed: false, new_count: 0, latest_keys: [] },
				},
			});
		}

		if (
			request.method() === "GET" &&
			pathname === "/api/reaction-token/status"
		) {
			return json(route, {
				configured: false,
				masked_token: null,
				check: { state: "idle", message: null, checked_at: null },
				owner: null,
			});
		}

		if (request.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "2.7.0" });
		}

		return json(
			route,
			{ error: { code: "not_found", message: pathname } },
			404,
		);
	});
}

async function expectRepoIdentity(
	page: Page,
	itemId: string,
	repoFullName: string,
	options: { expectCompact: boolean },
) {
	const card = page.locator(`[data-feed-item-key="release:${itemId}"]`);
	const label = card.locator('[data-repo-identity-label="true"]');
	await expect(label).toHaveAttribute(
		"data-repo-identity-label-mode",
		options.expectCompact ? "compact" : "full",
	);
	await expect(card.getByRole("link", { name: repoFullName })).toBeVisible();

	const metrics = await label.evaluate((element) => {
		const node = element as HTMLElement;
		const lineHeight = Number.parseFloat(
			window.getComputedStyle(node).lineHeight,
		);
		return {
			height: node.getBoundingClientRect().height,
			lineHeight,
			visibleText:
				node.querySelector<HTMLElement>("[aria-hidden='true']")?.textContent ??
				node.textContent ??
				"",
		};
	});
	expect(metrics.height).toBeLessThanOrEqual(metrics.lineHeight * 2 + 1);
	if (options.expectCompact) {
		expect(metrics.visibleText).toContain(
			repoFullName.split("/")[1]!.slice(-8),
		);
		expect(metrics.visibleText).not.toMatch(/…$/);
	}
}

async function expectNoHorizontalOverflow(page: Page) {
	const metrics = await page.evaluate(() => {
		const documentElement = document.documentElement;
		return {
			pageOverflow: documentElement.scrollWidth - documentElement.clientWidth,
			cardOverflows: Array.from(
				document.querySelectorAll<HTMLElement>("[data-feed-item-key]"),
			).map((card) => card.scrollWidth - card.clientWidth),
		};
	});
	expect(metrics.pageOverflow).toBeLessThanOrEqual(1);
	for (const cardOverflow of metrics.cardOverflows) {
		expect(cardOverflow).toBeLessThanOrEqual(1);
	}
}

for (const viewport of [
	{ name: "375px", width: 375, height: 667, extremeCompact: true },
	{ name: "390px", width: 390, height: 844, extremeCompact: true },
	{ name: "desktop", width: 1440, height: 1000, extremeCompact: true },
]) {
	test(`repo identity remains readable at ${viewport.name}`, async ({
		page,
	}) => {
		await page.setViewportSize({
			width: viewport.width,
			height: viewport.height,
		});
		await installMocks(page);
		await page.goto("/");

		await expect(
			page.locator('[data-feed-item-key="release:repo-identity-two-line"]'),
		).toBeVisible({ timeout: 15_000 });
		await expectRepoIdentity(page, "repo-identity-two-line", TWO_LINE_REPO, {
			expectCompact: false,
		});
		await expectRepoIdentity(page, "repo-identity-extreme", EXTREME_REPO, {
			expectCompact: viewport.extremeCompact,
		});
		await expectNoHorizontalOverflow(page);
	});
}
