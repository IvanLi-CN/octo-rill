import { type Route, expect, test } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function svgAvatarDataUrl(
	label: string,
	background: string,
	foreground = "#ffffff",
) {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240"><rect width="240" height="240" rx="120" fill="${background}"/><text x="120" y="132" font-family="Inter,Arial,sans-serif" font-size="44" font-weight="700" text-anchor="middle" fill="${foreground}">${label}</text></svg>`,
	)}`;
}

function buildReactionReadyFeedItem(id: string) {
	return {
		kind: "release",
		ts: "2026-04-18T08:00:00+08:00",
		id,
		repo_full_name: "owner/repo",
		title: `Release ${id}`,
		body: "- settings page migration\n- linuxdo oauth snapshot\n- pat fallback guide",
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `https://github.com/owner/repo/releases/tag/v${id}`,
		unread: null,
		translated: null,
		smart: null,
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
	};
}

async function installSettingsMocks(
	page: Parameters<typeof test>[0]["page"],
	options?: {
		linuxdoAvailable?: boolean;
		linuxdoConnection?: Record<string, unknown> | null;
		reactionTokenConfigured?: boolean;
		reactionTokenMasked?: string | null;
		reactionTokenState?: "idle" | "valid" | "invalid" | "error";
		reactionTokenMessage?: string | null;
		includeOwnReleases?: boolean;
		withReactionFeed?: boolean;
	},
) {
	let includeOwnReleases = options?.includeOwnReleases ?? false;
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: "storybook-user",
					github_user_id: 42,
					login: "storybook-user",
					name: "Storybook User",
					avatar_url: svgAvatarDataUrl("SU", "#4f6a98"),
					email: "storybook-user@example.com",
					is_admin: true,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			return json(route, {
				items: options?.withReactionFeed
					? [buildReactionReadyFeedItem("70001")]
					: [buildReactionReadyFeedItem("70001")],
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
				configured: options?.reactionTokenConfigured ?? false,
				masked_token: options?.reactionTokenMasked ?? null,
				check: {
					state: options?.reactionTokenState ?? "idle",
					message: options?.reactionTokenMessage ?? null,
					checked_at: "2026-04-18T08:00:00+08:00",
				},
			});
		}

		if (req.method() === "GET" && pathname === "/api/me/linuxdo") {
			return json(route, {
				available: options?.linuxdoAvailable ?? true,
				connection: options?.linuxdoConnection ?? null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/me/profile") {
			return json(route, {
				user_id: "storybook-user",
				daily_brief_local_time: "08:00",
				daily_brief_time_zone: "Asia/Shanghai",
				last_active_at: "2026-04-18T08:00:00+08:00",
				include_own_releases: includeOwnReleases,
			});
		}

		if (req.method() === "PATCH" && pathname === "/api/me/profile") {
			const payload = req.postDataJSON() as
				| { include_own_releases?: boolean }
				| undefined;
			includeOwnReleases = payload?.include_own_releases ?? includeOwnReleases;
			return json(route, {
				user_id: "storybook-user",
				daily_brief_local_time: "08:00",
				daily_brief_time_zone: "Asia/Shanghai",
				last_active_at: "2026-04-18T08:00:00+08:00",
				include_own_releases: includeOwnReleases,
			});
		}

		if (req.method() === "GET" && pathname === "/api/version") {
			return json(route, { ok: true, version: "1.2.3", source: "test" });
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
}

test("dashboard account menu exposes settings entry and opens settings page", async ({
	page,
}) => {
	await installSettingsMocks(page);

	await page.goto("/");
	await page.getByRole("button", { name: "查看账号信息" }).click();
	await expect(page.locator("[data-dashboard-settings-entry]")).toBeVisible();
	await page.getByRole("link", { name: "设置" }).click();

	await expect(page).toHaveURL(/\/settings$/);
	await expect(page.locator('[data-settings-section="linuxdo"]')).toContainText(
		"LinuxDO 绑定",
	);
});

test("settings deep link focuses github pat section", async ({ page }) => {
	await installSettingsMocks(page, {
		reactionTokenConfigured: true,
		reactionTokenMasked: "ghp_****_saved",
		reactionTokenState: "valid",
		reactionTokenMessage: "token is valid",
	});

	await page.goto("/settings?section=github-pat");

	await expect(page).toHaveURL(/section=github-pat/);
	await expect(
		page.locator('[data-settings-section="github-pat"]'),
	).toContainText("ghp_****_saved");
	await expect(page.getByLabel("GitHub PAT")).toHaveAttribute(
		"autocomplete",
		"new-password",
	);
});

test("settings shows bound linuxdo snapshot", async ({ page }) => {
	await installSettingsMocks(page, {
		linuxdoAvailable: true,
		linuxdoConnection: {
			linuxdo_user_id: 2048,
			username: "linuxdo-bound",
			name: "LinuxDO Bound",
			avatar_url: svgAvatarDataUrl("LD", "#0ea5e9"),
			trust_level: 4,
			active: true,
			silenced: false,
			linked_at: "2026-04-16T10:00:00+08:00",
			updated_at: "2026-04-18T09:30:00+08:00",
		},
	});

	await page.goto("/settings");

	const linuxdoSection = page.locator('[data-settings-section="linuxdo"]');
	await expect(linuxdoSection.getByText("@linuxdo-bound")).toBeVisible();
	await expect(linuxdoSection).toContainText(/Trust level\s*4/);
	await expect(
		linuxdoSection.getByRole("button", { name: "解绑 LinuxDO" }),
	).toBeVisible();
});

test("settings deep link saves my releases opt-in", async ({ page }) => {
	await installSettingsMocks(page, {
		includeOwnReleases: false,
	});

	await page.goto("/settings?section=my-releases");

	await expect(page).toHaveURL(/section=my-releases/);
	const myReleasesSection = page.locator(
		'[data-settings-section="my-releases"]',
	);
	await expect(myReleasesSection).toContainText("仅显示已加星仓库");

	const switchControl = myReleasesSection.getByRole("switch", {
		name: "我的发布",
	});
	await expect(switchControl).toHaveAttribute("aria-checked", "false");
	await switchControl.click();
	await myReleasesSection
		.getByRole("button", { name: "保存“我的发布”" })
		.click();

	await expect(switchControl).toHaveAttribute("aria-checked", "true");
	await expect(myReleasesSection).toContainText("已纳入我的发布");
});

test("unknown app route shows not-found page after app shell boot", async ({
	page,
}) => {
	await installSettingsMocks(page);

	await page.goto("/does-not-exist");

	await expect(page.getByRole("heading", { name: "页面不存在" })).toBeVisible();
	await expect(page.locator("[data-not-found-surface]")).toContainText(
		"/does-not-exist",
	);
	await expect(page.getByRole("link", { name: "返回工作台" })).toBeVisible();
	await expect(page.getByRole("link", { name: "打开设置" })).toBeVisible();
});
