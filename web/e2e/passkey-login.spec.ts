import { expect, test, type Route } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";
import { installPasskeyBrowserMock } from "./passkeyHelpers";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function buildPasskeyCreationOptions() {
	return {
		publicKey: {
			rp: {
				name: "OctoRill",
				id: "localhost",
			},
			user: {
				id: "AQIDBA",
				name: "passkey-user",
				displayName: "Passkey User",
			},
			challenge: "BQYHCAkK",
			pubKeyCredParams: [{ type: "public-key", alg: -7 }],
			timeout: 60000,
			excludeCredentials: [],
			attestation: "none",
			extensions: {},
		},
	};
}

function buildPasskeyRequestOptions() {
	return {
		publicKey: {
			challenge: "CgsMDQ4P",
			timeout: 60000,
			rpId: "localhost",
			allowCredentials: [],
			userVerification: "required",
			extensions: {},
		},
		mediation: "conditional",
	};
}

async function installLandingPasskeyMocks(
	page: Parameters<typeof test>[0]["page"],
) {
	let authenticated = false;

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			if (!authenticated) {
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

			return json(
				route,
				buildMockMeResponse({
					id: "passkey-user",
					github_user_id: 42,
					login: "passkey-user",
					name: "Passkey User",
					avatar_url: null,
					email: "passkey@example.com",
					is_admin: false,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			return json(route, { items: [], next_cursor: null });
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/version") {
			return json(route, { ok: true, version: "1.2.3", source: "test" });
		}

		if (req.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "1.2.3" });
		}

		if (
			req.method() === "POST" &&
			pathname === "/api/auth/passkeys/register/options"
		) {
			return json(route, buildPasskeyCreationOptions());
		}

		if (
			req.method() === "POST" &&
			pathname === "/api/auth/passkeys/register/verify"
		) {
			return json(route, {
				status: "pending_github_bind",
				next_path: "/bind/github?passkey=created",
			});
		}

		if (
			req.method() === "POST" &&
			pathname === "/api/auth/passkeys/authenticate/options"
		) {
			return json(route, buildPasskeyRequestOptions());
		}

		if (
			req.method() === "POST" &&
			pathname === "/api/auth/passkeys/authenticate/verify"
		) {
			authenticated = true;
			return json(route, {
				status: "authenticated",
				next_path: "/",
			});
		}

		if (req.method() === "GET" && pathname === "/api/auth/bind-context") {
			return json(route, {
				linuxdo_available: true,
				pending_linuxdo: null,
				pending_passkey: {
					label: "Passkey · 2026-04-22 10:12 UTC",
					created_at: "2026-04-22T10:12:00Z",
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

async function installSettingsPasskeyMocks(
	page: Parameters<typeof test>[0]["page"],
) {
	let passkeys: Array<{
		id: string;
		label: string;
		created_at: string;
		last_used_at: string | null;
	}> = [];

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: "passkey-user",
					github_user_id: 42,
					login: "passkey-user",
					name: "Passkey User",
					avatar_url: null,
					email: "passkey@example.com",
					is_admin: false,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			return json(route, { items: [], next_cursor: null });
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/version") {
			return json(route, { ok: true, version: "1.2.3", source: "test" });
		}

		if (req.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "1.2.3" });
		}

		if (req.method() === "GET" && pathname === "/api/me/github-connections") {
			return json(route, {
				items: [
					{
						id: "ghconn_primary",
						github_user_id: 42,
						login: "passkey-user",
						name: "Passkey User",
						avatar_url: null,
						email: "passkey@example.com",
						scopes: "read:user, user:email, notifications, public_repo",
						linked_at: "2026-04-16T10:00:00+08:00",
						updated_at: "2026-04-18T09:00:00+08:00",
					},
				],
			});
		}

		if (req.method() === "GET" && pathname === "/api/me/linuxdo") {
			return json(route, {
				available: true,
				connection: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/me/profile") {
			return json(route, {
				user_id: "passkey-user",
				daily_brief_local_time: "08:00",
				daily_brief_time_zone: "Asia/Shanghai",
				last_active_at: "2026-04-18T08:00:00+08:00",
				include_own_releases: false,
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

		if (req.method() === "GET" && pathname === "/api/me/passkeys") {
			return json(route, { items: passkeys });
		}

		if (
			req.method() === "POST" &&
			pathname === "/api/auth/passkeys/register/options"
		) {
			return json(route, buildPasskeyCreationOptions());
		}

		if (
			req.method() === "POST" &&
			pathname === "/api/auth/passkeys/register/verify"
		) {
			passkeys = [
				...passkeys,
				{
					id: "pk_new",
					label: "Passkey · 2026-04-22 10:12 UTC",
					created_at: "2026-04-22T10:12:00Z",
					last_used_at: null,
				},
			];
			return json(route, {
				status: "registered",
				next_path: "/settings?section=passkeys&passkey=registered",
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

test("landing onboarding can create a passkey and continue to GitHub bind", async ({
	page,
}) => {
	await installPasskeyBrowserMock(page);
	await installLandingPasskeyMocks(page);

	await page.goto("/");
	await page
		.getByRole("button", { name: "首次使用？创建 Passkey 并继续绑定 GitHub" })
		.click();

	await page.waitForURL("**/bind/github?passkey=created");
	await expect(page.getByText("Passkey 已暂存")).toBeVisible();
	await expect(page.getByText("待挂接的 Passkey")).toBeVisible();
});

test("returning user can sign in directly with a passkey", async ({ page }) => {
	await installPasskeyBrowserMock(page);
	await installLandingPasskeyMocks(page);

	await page.goto("/");
	await page.getByRole("button", { name: "使用 Passkey 登录" }).click();

	await page.waitForURL("**/");
	await expect(
		page.getByRole("button", { name: "查看账号信息" }),
	).toBeVisible();
});

test("settings can add a new passkey for the signed-in account", async ({
	page,
}) => {
	await installPasskeyBrowserMock(page);
	await installSettingsPasskeyMocks(page);

	await page.goto("/settings?section=passkeys");

	await page.getByRole("button", { name: "添加 Passkey" }).click();

	const passkeysSection = page.locator('[data-settings-section="passkeys"]');
	await expect(page.getByText("Passkey 已添加")).toBeVisible();
	await expect(
		passkeysSection.getByText("Passkey · 2026-04-22 10:12 UTC"),
	).toBeVisible();
});
