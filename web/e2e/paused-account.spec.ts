import { expect, test, type Route } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

test("paused session enters account recovery from GET /api/me", async ({
	page,
}) => {
	const apiPaths: string[] = [];

	await page.route("**/api/**", async (route) => {
		const request = route.request();
		const pathname = new URL(request.url()).pathname;
		apiPaths.push(pathname);

		if (request.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse(
					{
						id: "2f4k7m9p3x6c8v2a",
						github_user_id: 10,
						login: "octo-member",
						name: "Octo Member",
						avatar_url: null,
						email: "member@example.com",
						is_admin: false,
						account_status: "paused",
						paused_at: "2026-08-06T03:15:00+08:00",
					},
					{
						access_sync: {
							task_id: null,
							task_type: null,
							event_path: null,
							reason: "none",
						},
					},
				),
			);
		}

		return json(route, { error: { code: "unexpected_api" } }, 404);
	});

	await page.goto("/");

	await expect(page).toHaveURL(/\/account\/paused$/);
	await expect(page.getByRole("heading", { name: "账号已暂停" })).toBeVisible();
	await expect(page.getByRole("button", { name: "恢复账号" })).toBeVisible();
	expect(apiPaths).toContain("/api/me");
	expect(apiPaths).not.toContain("/api/account/status");
	expect(apiPaths).not.toContain("/api/feed");
});
