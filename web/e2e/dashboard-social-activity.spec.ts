import {
	type Locator,
	type Page,
	type Route,
	expect,
	test,
} from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function socialPrimaryDesktop(page: Locator | Page, text: string) {
	return page
		.locator("[data-social-card-primary-full-label]")
		.filter({ hasText: text });
}

async function expectInlineSocialCard(
	card: Locator,
	options?: { maxActionCenterOffsetX?: number | null },
) {
	const requestedMaxActionCenterOffsetX =
		options && "maxActionCenterOffsetX" in options
			? options.maxActionCenterOffsetX
			: undefined;
	const row = card.locator("[data-social-card-row]").first();
	await expect(row).toBeVisible();
	const actor = card.locator('[data-social-card-segment="actor"]').first();
	const action = card.locator('[data-social-card-segment="action"]').first();
	const target = card.locator('[data-social-card-segment="target"]').first();
	await expect(actor).toBeVisible();
	await expect(action).toBeVisible();
	await expect(target).toBeVisible();

	const layout = await card.getAttribute("data-social-card-layout");
	expect(layout).toBe("inline-compact");

	const geometry = await row.evaluate((node) => {
		const actorNode = node.querySelector<HTMLElement>(
			'[data-social-card-segment="actor"]',
		);
		const actionNode = node.querySelector<HTMLElement>(
			'[data-social-card-segment="action"]',
		);
		const targetNode = node.querySelector<HTMLElement>(
			'[data-social-card-segment="target"]',
		);
		if (!actorNode || !actionNode || !targetNode) {
			return null;
		}
		const actorGroup =
			actorNode.querySelector<HTMLElement>(
				'[data-social-card-entity-group="actor"]',
			) ?? actorNode;
		const targetGroup =
			targetNode.querySelector<HTMLElement>(
				'[data-social-card-entity-group="target"]',
			) ?? targetNode;

		const rowRect = node.getBoundingClientRect();
		const actorRect = actorGroup.getBoundingClientRect();
		const actionRect = actionNode.getBoundingClientRect();
		const targetRect = targetGroup.getBoundingClientRect();
		const rowCenterY = rowRect.top + rowRect.height / 2;
		const centerDelta = (rect: DOMRect) =>
			Math.abs(rect.top + rect.height / 2 - rowCenterY);

		const actionCenterX = actionRect.left + actionRect.width / 2;
		const rowCenterX = rowRect.left + rowRect.width / 2;
		return {
			rowHeight: rowRect.height,
			rowOverflow: node.scrollWidth - node.clientWidth,
			actorLeft: actorRect.left,
			actionLeft: actionRect.left,
			targetLeft: targetRect.left,
			actorWidth: actorRect.width,
			targetWidth: targetRect.width,
			actorCenterDelta: centerDelta(actorRect),
			actionCenterDelta: centerDelta(actionRect),
			targetCenterDelta: centerDelta(targetRect),
			rowLeftGap: actorRect.left - rowRect.left,
			rowRightGap: rowRect.right - targetRect.right,
			actionCenterOffsetX: Math.abs(actionCenterX - rowCenterX),
			balanceMode: node.dataset.socialCardBalanceMode ?? "centered",
			actionText: actionNode.textContent?.trim() ?? "",
		};
	});

	expect(geometry).not.toBeNull();
	if (!geometry) {
		throw new Error("Expected inline social card geometry to exist");
	}
	expect(geometry.actorLeft).toBeLessThan(geometry.actionLeft);
	expect(geometry.actionLeft).toBeLessThan(geometry.targetLeft);
	expect(geometry.rowOverflow).toBeLessThanOrEqual(1);
	expect(geometry.actorCenterDelta).toBeLessThan(geometry.rowHeight * 0.32);
	expect(geometry.actionCenterDelta).toBeLessThan(geometry.rowHeight * 0.32);
	expect(geometry.targetCenterDelta).toBeLessThan(geometry.rowHeight * 0.32);
	expect(geometry.rowLeftGap).toBeLessThanOrEqual(14);
	expect(geometry.rowRightGap).toBeLessThanOrEqual(14);
	const maxActionCenterOffsetX =
		requestedMaxActionCenterOffsetX === undefined
			? geometry.balanceMode === "adaptive"
				? null
				: 12
			: requestedMaxActionCenterOffsetX;
	if (maxActionCenterOffsetX !== null) {
		expect(geometry.actionCenterOffsetX).toBeLessThanOrEqual(
			maxActionCenterOffsetX,
		);
	}
	expect(geometry.actionText).toBe("");

	for (const label of await card.locator("[data-social-card-primary]").all()) {
		await expect(label).toHaveAttribute("data-social-card-primary-full", /.+/);
		await expect(label).toHaveAttribute(
			"data-social-card-primary-mobile",
			/.+/,
		);
	}
	await expect(
		card.locator("[data-social-card-secondary-mobile-label]"),
	).toHaveCount(0);
	await expect(row.locator(".lucide-arrow-up-right")).toHaveCount(0);
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

	await expect(page.getByRole("tab", { name: "加星" })).toBeVisible();
	await expect(page.getByRole("tab", { name: "关注" })).toBeVisible();
	await expect(socialPrimaryDesktop(page, "octocat")).toBeVisible();
	await expect(socialPrimaryDesktop(page, "monalisa")).toBeVisible();
	await expect(socialPrimaryDesktop(page, "owner/repo")).toBeVisible();
	await expect(
		page.locator(
			'[data-social-card-kind="repo_star_received"] [data-social-card-segment="target"] [data-social-card-primary-full-label]',
		),
	).toHaveText("owner/repo");
	await expect(
		page.locator(
			'[data-social-card-kind="repo_star_received"] a[href^="https://github.com/"]:visible',
		),
	).toHaveCount(2);
	await expect(
		page.locator(
			'[data-social-card-kind="repo_star_received"][data-social-card-time-visible="true"] [data-social-card-timestamp]',
		),
	).toHaveCount(1);
	await expect(
		page.locator(
			'[data-social-card-kind="follower_received"] a[href^="https://github.com/"]:visible',
		),
	).toHaveCount(1);
	await expect(
		page.locator(
			'[data-social-card-kind="follower_received"][data-social-card-time-visible="false"] [data-social-card-timestamp]',
		),
	).toHaveCount(0);

	await page.getByRole("tab", { name: "加星" }).click();
	await expect(socialPrimaryDesktop(page, "octocat")).toBeVisible();
	await expect(socialPrimaryDesktop(page, "monalisa")).toHaveCount(0);
	await expect(socialPrimaryDesktop(page, "owner/repo")).toBeVisible();
	await expect(
		page.locator(
			'[data-social-card-kind="repo_star_received"] a[href^="https://github.com/"]:visible',
		),
	).toHaveCount(2);
	await expect(
		page.locator(
			'[data-social-card-kind="repo_star_received"][data-social-card-time-visible="true"] [data-social-card-timestamp]',
		),
	).toHaveCount(1);

	await page.getByRole("tab", { name: "关注" }).click();
	await expect(socialPrimaryDesktop(page, "monalisa")).toBeVisible();
	await expect(socialPrimaryDesktop(page, "octo")).toBeVisible();
	await expect(
		page.getByRole("heading", { name: "Release 20001" }),
	).toHaveCount(0);
	await expect(socialPrimaryDesktop(page, "octocat")).toHaveCount(0);
	await expect(
		page.locator(
			'[data-social-card-kind="follower_received"] a[href^="https://github.com/"]:visible',
		),
	).toHaveCount(1);
	await expect(
		page.locator(
			'[data-social-card-kind="follower_received"][data-social-card-time-visible="false"] [data-social-card-timestamp]',
		),
	).toHaveCount(0);
});

test("dashboard keeps social cards inline on mobile widths without horizontal overflow", async ({
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
					login: "octo-rill-owner",
					name: "Octo Owner",
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
						kind: "repo_star_received",
						ts: "2026-04-10T11:30:00Z",
						id: "star-mobile-inline",
						repo_full_name: "octo-rill/mobile-dashboard-social-activity-feed",
						title: null,
						body: null,
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/frontend-systems-maintainer",
						unread: null,
						actor: {
							login: "frontend-systems-maintainer",
							avatar_url: null,
							html_url: "https://github.com/frontend-systems-maintainer",
						},
						translated: null,
						smart: null,
						reactions: null,
					},
					{
						kind: "follower_received",
						ts: "2026-04-10T11:00:00Z",
						id: "follow-mobile-inline",
						repo_full_name: null,
						title: null,
						body: null,
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/design-ops-collaborator",
						unread: null,
						actor: {
							login: "design-ops-collaborator",
							avatar_url: null,
							html_url: "https://github.com/design-ops-collaborator",
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

	for (const width of [390, 375]) {
		await page.setViewportSize({ width, height: 844 });
		await page.goto("/");

		const starCard = page
			.locator('[data-social-card-kind="repo_star_received"]')
			.first();
		const followerCard = page
			.locator('[data-social-card-kind="follower_received"]')
			.first();

		await expect(starCard).toBeVisible();
		await expect(followerCard).toBeVisible();
		await expectInlineSocialCard(starCard);
		await expectInlineSocialCard(followerCard);
		await expect(
			starCard.locator(
				'[data-social-card-segment="actor"] [data-social-card-primary-mobile-label]',
			),
		).toHaveJSProperty("textContent", "frontend-systems-maintainer");
		await expect(
			starCard
				.locator("[data-social-card-row]")
				.first()
				.locator('a[data-social-card-segment="actor"]'),
		).toHaveAttribute("href", "https://github.com/frontend-systems-maintainer");
		const starRepoMobileLabel = starCard.locator(
			'[data-social-card-segment="target"] [data-social-card-primary-mobile-label]',
		);
		await expect(
			starCard
				.locator("[data-social-card-row]")
				.first()
				.locator('a[data-social-card-segment="target"]'),
		).toHaveAttribute(
			"href",
			"https://github.com/octo-rill/mobile-dashboard-social-activity-feed",
		);
		const repoMetrics = await starRepoMobileLabel.evaluate((node) => ({
			clientWidth: node.clientWidth,
			scrollWidth: node.scrollWidth,
			text: node.textContent ?? "",
			visibleChars: (node.textContent ?? "").replace("…", "").length,
		}));
		const starTargetRightGap = await starCard
			.locator("[data-social-card-row]")
			.first()
			.evaluate((node) => {
				const targetGroup = node.querySelector<HTMLElement>(
					'[data-social-card-entity-group="target"]',
				);
				const rowRect = node.getBoundingClientRect();
				const targetRect = targetGroup?.getBoundingClientRect();
				return targetRect
					? rowRect.right - targetRect.right
					: Number.POSITIVE_INFINITY;
			});
		expect(repoMetrics.text.startsWith("octo-rill/")).toBe(true);
		expect(repoMetrics.text.includes("…")).toBe(true);
		expect(repoMetrics.visibleChars).toBeGreaterThanOrEqual(36);
		expect(repoMetrics.scrollWidth).toBeLessThanOrEqual(
			repoMetrics.clientWidth,
		);
		expect(starTargetRightGap).toBeLessThanOrEqual(14);
		await expect(
			followerCard.locator(
				'[data-social-card-segment="actor"] [data-social-card-primary-mobile-label]',
			),
		).toHaveJSProperty("textContent", "design-ops-collaborator");
		await expect(
			followerCard
				.locator("[data-social-card-row]")
				.first()
				.locator('a[data-social-card-segment="actor"]'),
		).toHaveAttribute("href", "https://github.com/design-ops-collaborator");
		await expect(
			followerCard.locator(
				'[data-social-card-segment="target"] [data-social-card-primary-mobile-label]',
			),
		).toHaveJSProperty("textContent", "octo-rill-owner");
		await expect(
			followerCard
				.locator("[data-social-card-row]")
				.first()
				.locator('a[data-social-card-segment="target"]'),
		).toHaveAttribute("href", "https://github.com/octo-rill-owner");
		const followerTargetRightGap = await followerCard
			.locator("[data-social-card-row]")
			.first()
			.evaluate((node) => {
				const targetGroup = node.querySelector<HTMLElement>(
					'[data-social-card-entity-group="target"]',
				);
				const rowRect = node.getBoundingClientRect();
				const targetRect = targetGroup?.getBoundingClientRect();
				return targetRect
					? rowRect.right - targetRect.right
					: Number.POSITIVE_INFINITY;
			});
		expect(followerTargetRightGap).toBeLessThanOrEqual(14);
		await expect(starCard.locator("[data-social-card-timestamp]")).toHaveCount(
			1,
		);
		await expect(
			followerCard.locator("[data-social-card-timestamp]"),
		).toHaveCount(0);
	}
});

test("dashboard keeps the action centered with left-only, right-only, and bilateral long copy", async ({
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
					id: "viewer-centered-proof",
					github_user_id: 10,
					login: "IvanLi-CN",
					name: "Octo Owner",
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
						kind: "repo_star_received",
						ts: "2026-04-10T11:45:00Z",
						id: "star-right-long",
						repo_full_name: "IvanLi-CN/mobile-dashboard-social-adaptive-case",
						title: null,
						body: null,
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/ms",
						unread: null,
						actor: {
							login: "ms",
							avatar_url: null,
							html_url: "https://github.com/ms",
						},
						translated: null,
						smart: null,
						reactions: null,
					},
					{
						kind: "follower_received",
						ts: "2026-04-10T11:30:00Z",
						id: "follow-left-long",
						repo_full_name: null,
						title: null,
						body: null,
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url: "https://github.com/design-ops-collaborator-case",
						unread: null,
						actor: {
							login: "design-ops-collaborator-case",
							avatar_url: null,
							html_url: "https://github.com/design-ops-collaborator-case",
						},
						translated: null,
						smart: null,
						reactions: null,
					},
					{
						kind: "repo_star_received",
						ts: "2026-04-10T11:15:00Z",
						id: "star-both-long",
						repo_full_name:
							"IvanLi-CN/mobile-dashboard-social-activity-feed-bilateral-proof",
						title: null,
						body: null,
						body_truncated: false,
						subtitle: null,
						reason: null,
						subject_type: null,
						html_url:
							"https://github.com/frontend-systems-maintainer-centered-proof",
						unread: null,
						actor: {
							login: "frontend-systems-maintainer-centered-proof",
							avatar_url: null,
							html_url:
								"https://github.com/frontend-systems-maintainer-centered-proof",
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

	await page.setViewportSize({ width: 390, height: 844 });
	await page.goto("/");

	const cards = page.locator("[data-social-card-kind]");
	await expect(cards).toHaveCount(3);

	for (const card of await cards.all()) {
		await expectInlineSocialCard(card, { maxActionCenterOffsetX: null });
	}

	const leftLongCard = page.locator(
		'[data-social-card-kind="follower_received"]',
	);
	const rightLongCard = page
		.locator('[data-social-card-kind="repo_star_received"]')
		.first();
	const bilateralLongCard = page
		.locator('[data-social-card-kind="repo_star_received"]')
		.nth(1);

	const leftLongWidths = await leftLongCard
		.locator("[data-social-card-row]")
		.first()
		.evaluate((node) => {
			const actor = node.querySelector<HTMLElement>(
				'[data-social-card-segment="actor"]',
			);
			const target = node.querySelector<HTMLElement>(
				'[data-social-card-segment="target"]',
			);
			const actorGroup =
				actor?.querySelector<HTMLElement>(
					'[data-social-card-entity-group="actor"]',
				) ?? actor;
			const targetGroup =
				target?.querySelector<HTMLElement>(
					'[data-social-card-entity-group="target"]',
				) ?? target;
			const actorLabel = actorGroup?.querySelector<HTMLElement>(
				"[data-social-card-primary]",
			);
			const targetLabel = targetGroup?.querySelector<HTMLElement>(
				"[data-social-card-primary]",
			);
			const action = node.querySelector<HTMLElement>(
				'[data-social-card-segment="action"]',
			);
			return {
				actorWidth: actorGroup?.getBoundingClientRect().width ?? 0,
				targetWidth: targetGroup?.getBoundingClientRect().width ?? 0,
				actorOverflow:
					(actorLabel?.scrollWidth ?? 0) - (actorLabel?.clientWidth ?? 0),
				targetOverflow:
					(targetLabel?.scrollWidth ?? 0) - (targetLabel?.clientWidth ?? 0),
				gapLeft:
					action && actorGroup
						? action.getBoundingClientRect().left -
							actorGroup.getBoundingClientRect().right
						: 0,
				gapRight:
					action && targetGroup
						? targetGroup.getBoundingClientRect().left -
							action.getBoundingClientRect().right
						: 0,
				balanceMode: node.dataset.socialCardBalanceMode ?? "centered",
			};
		});
	expect(leftLongWidths.actorWidth).toBeGreaterThanOrEqual(
		leftLongWidths.targetWidth,
	);
	expect(
		Math.abs(leftLongWidths.gapLeft - leftLongWidths.gapRight),
	).toBeLessThanOrEqual(2);
	expect(leftLongWidths.balanceMode).toBe("adaptive");

	const rightLongWidths = await rightLongCard
		.locator("[data-social-card-row]")
		.first()
		.evaluate((node) => {
			const actor = node.querySelector<HTMLElement>(
				'[data-social-card-segment="actor"]',
			);
			const target = node.querySelector<HTMLElement>(
				'[data-social-card-segment="target"]',
			);
			const actorGroup =
				actor?.querySelector<HTMLElement>(
					'[data-social-card-entity-group="actor"]',
				) ?? actor;
			const targetGroup =
				target?.querySelector<HTMLElement>(
					'[data-social-card-entity-group="target"]',
				) ?? target;
			const actorLabel = actorGroup?.querySelector<HTMLElement>(
				"[data-social-card-primary]",
			);
			const targetLabel = targetGroup?.querySelector<HTMLElement>(
				"[data-social-card-primary]",
			);
			const action = node.querySelector<HTMLElement>(
				'[data-social-card-segment="action"]',
			);
			return {
				actorWidth: actorGroup?.getBoundingClientRect().width ?? 0,
				targetWidth: targetGroup?.getBoundingClientRect().width ?? 0,
				actorOverflow:
					(actorLabel?.scrollWidth ?? 0) - (actorLabel?.clientWidth ?? 0),
				targetOverflow:
					(targetLabel?.scrollWidth ?? 0) - (targetLabel?.clientWidth ?? 0),
				gapLeft:
					action && actorGroup
						? action.getBoundingClientRect().left -
							actorGroup.getBoundingClientRect().right
						: 0,
				gapRight:
					action && targetGroup
						? targetGroup.getBoundingClientRect().left -
							action.getBoundingClientRect().right
						: 0,
				balanceMode: node.dataset.socialCardBalanceMode ?? "centered",
			};
		});
	expect(["centered", "adaptive"]).toContain(rightLongWidths.balanceMode);
	if (rightLongWidths.balanceMode === "adaptive") {
		expect(
			Math.abs(rightLongWidths.gapLeft - rightLongWidths.gapRight),
		).toBeLessThanOrEqual(2);
	} else {
		expect(rightLongWidths.targetOverflow).toBeGreaterThan(1);
	}

	const bilateralLongWidths = await bilateralLongCard
		.locator("[data-social-card-row]")
		.first()
		.evaluate((node) => {
			const actor = node.querySelector<HTMLElement>(
				'[data-social-card-segment="actor"]',
			);
			const target = node.querySelector<HTMLElement>(
				'[data-social-card-segment="target"]',
			);
			const actorGroup =
				actor?.querySelector<HTMLElement>(
					'[data-social-card-entity-group="actor"]',
				) ?? actor;
			const targetGroup =
				target?.querySelector<HTMLElement>(
					'[data-social-card-entity-group="target"]',
				) ?? target;
			const actorLabel = actorGroup?.querySelector<HTMLElement>(
				"[data-social-card-primary]",
			);
			const targetLabel = targetGroup?.querySelector<HTMLElement>(
				"[data-social-card-primary]",
			);
			return {
				actorWidth: actorGroup?.getBoundingClientRect().width ?? 0,
				targetWidth: targetGroup?.getBoundingClientRect().width ?? 0,
				actorOverflow:
					(actorLabel?.scrollWidth ?? 0) - (actorLabel?.clientWidth ?? 0),
				targetOverflow:
					(targetLabel?.scrollWidth ?? 0) - (targetLabel?.clientWidth ?? 0),
				balanceMode: node.dataset.socialCardBalanceMode ?? "centered",
			};
		});
	expect(
		Math.abs(bilateralLongWidths.actorWidth - bilateralLongWidths.targetWidth),
	).toBeLessThanOrEqual(18);
	expect(bilateralLongWidths.balanceMode).toBe("centered");
	expect(
		Math.max(
			bilateralLongWidths.actorOverflow,
			bilateralLongWidths.targetOverflow,
		),
	).toBeGreaterThan(1);

	await expectInlineSocialCard(bilateralLongCard, {
		maxActionCenterOffsetX: 18,
	});

	await expect(
		page.locator(
			'[data-social-card-kind="follower_received"] [data-social-card-segment="target"] [data-social-card-primary-mobile-label]',
		),
	).toHaveJSProperty("textContent", "IvanLi-CN");
});

test("dashboard keeps short follower rows right-trimmed on mobile", async ({
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
					id: "viewer-short-followers",
					github_user_id: 10,
					login: "IvanLi-CN",
					name: "Octo Owner",
					avatar_url: null,
					email: null,
					is_admin: false,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			return json(route, {
				items: [
					{ login: "brutany", expectCentered: true },
					{ login: "zhenyuanwang46-droid", expectCentered: false },
					{ login: "pseudocodes", expectCentered: true },
					{ login: "zyou9724-creator", expectCentered: false },
					{ login: "mrlrk82", expectCentered: true },
				].map(({ login }, index) => ({
					kind: "follower_received",
					ts: `2026-04-10T11:${`${50 - index}`.padStart(2, "0")}:00Z`,
					id: `follow-short-${index + 1}`,
					repo_full_name: null,
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
				})),
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

	await page.setViewportSize({ width: 390, height: 844 });
	await page.goto("/?tab=followers");

	const cards = page.locator('[data-social-card-kind="follower_received"]');
	await expect(cards).toHaveCount(5);

	const expectedCenteredIds = new Set([
		"follow-short-1",
		"follow-short-3",
		"follow-short-5",
	]);

	for (const card of await cards.all()) {
		await expectInlineSocialCard(card);
		const metrics = await card
			.locator("[data-social-card-row]")
			.first()
			.evaluate((node) => {
				const targetGroup = node.querySelector<HTMLElement>(
					'[data-social-card-entity-group="target"]',
				);
				const rowRect = node.getBoundingClientRect();
				const targetRect = targetGroup?.getBoundingClientRect();
				return {
					balanceMode: node.dataset.socialCardBalanceMode ?? "centered",
					targetRightGap: targetRect
						? rowRect.right - targetRect.right
						: Number.POSITIVE_INFINITY,
				};
			});
		expect(metrics.targetRightGap).toBeLessThanOrEqual(14);

		const cardId = await card.getAttribute("data-feed-item-id");
		if (cardId && expectedCenteredIds.has(cardId)) {
			expect(metrics.balanceMode).toBe("centered");
		}
	}
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
		page.locator('[data-social-avatar-fallback="true"]:visible').first(),
	).toBeVisible();
});

test("switching social tabs clears stale feed items before the next dataset resolves", async ({
	page,
}) => {
	let releaseStarsResponse!: () => void;
	const starsResponseReady = new Promise<void>((resolve) => {
		releaseStarsResponse = resolve;
	});
	let _notificationsCalls = 0;
	let _briefsCalls = 0;
	let _reactionTokenStatusCalls = 0;
	let starsFeedCalls = 0;

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
				starsFeedCalls += 1;
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
			_notificationsCalls += 1;
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			_briefsCalls += 1;
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/reaction-token/status") {
			_reactionTokenStatusCalls += 1;
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

	await expect(socialPrimaryDesktop(page, "octocat-old")).toBeVisible();
	await expect.poll(() => _briefsCalls).toBe(1);
	await expect.poll(() => _reactionTokenStatusCalls).toBe(1);
	await page.getByRole("tab", { name: "加星" }).click();
	await expect(socialPrimaryDesktop(page, "octocat-old")).toHaveCount(0);
	await expect(page).toHaveURL(/\/\?tab=stars$/);
	await expect(page.locator("[data-dashboard-secondary-controls]")).toHaveCount(
		1,
	);
	await expect(
		page.locator('[data-feed-loading-skeleton="true"]'),
	).toBeVisible();
	await expect(page.locator("[data-dashboard-boot-header]")).toHaveCount(0);
	await expect(page.locator("[data-app-boot]")).toHaveCount(0);
	expect(starsFeedCalls).toBe(1);
	expect(_briefsCalls).toBe(1);
	expect(_reactionTokenStatusCalls).toBe(1);

	releaseStarsResponse();

	await expect(socialPrimaryDesktop(page, "octocat-new")).toBeVisible();
	await expect(page.locator('[data-feed-loading-skeleton="true"]')).toHaveCount(
		0,
	);
	await expect.poll(() => _briefsCalls).toBe(1);
	await expect.poll(() => _reactionTokenStatusCalls).toBe(1);
});
