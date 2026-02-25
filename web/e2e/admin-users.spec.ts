import { type Page, type Route, expect, test } from "@playwright/test";

type MockUser = {
	id: number;
	github_user_id: number;
	login: string;
	name: string | null;
	avatar_url: string | null;
	email: string | null;
	is_admin: boolean;
	is_disabled: boolean;
	created_at: string;
	updated_at: string;
};

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

async function installBaseMocks(
	page: Page,
	options: { isAdmin: boolean; adminApiForbidden?: boolean },
) {
	const users: MockUser[] = [
		{
			id: 1,
			github_user_id: 10,
			login: "octo-admin",
			name: "Octo Admin",
			avatar_url: null,
			email: "admin@example.com",
			is_admin: true,
			is_disabled: false,
			created_at: "2026-02-24T08:00:00Z",
			updated_at: "2026-02-24T08:00:00Z",
		},
		{
			id: 2,
			github_user_id: 20,
			login: "octo-user",
			name: "Octo User",
			avatar_url: null,
			email: "user@example.com",
			is_admin: false,
			is_disabled: false,
			created_at: "2026-02-25T08:00:00Z",
			updated_at: "2026-02-25T08:00:00Z",
		},
	];

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(route, {
				user: {
					id: 1,
					github_user_id: 10,
					login: "octo-admin",
					name: "Octo Admin",
					avatar_url: null,
					email: "admin@example.com",
					is_admin: options.isAdmin,
				},
			});
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

		if (pathname === "/api/admin/users") {
			if (options.adminApiForbidden) {
				return json(
					route,
					{
						ok: false,
						error: { code: "forbidden_admin_only", message: "forbidden" },
					},
					403,
				);
			}
			if (req.method() === "GET") {
				const role = url.searchParams.get("role") ?? "all";
				const status = url.searchParams.get("status") ?? "all";
				const query = (url.searchParams.get("query") ?? "").toLowerCase();
				const filtered = users.filter((u) => {
					if (role === "admin" && !u.is_admin) return false;
					if (role === "user" && u.is_admin) return false;
					if (status === "enabled" && u.is_disabled) return false;
					if (status === "disabled" && !u.is_disabled) return false;
					if (!query) return true;
					return (
						u.login.toLowerCase().includes(query) ||
						(u.name ?? "").toLowerCase().includes(query) ||
						(u.email ?? "").toLowerCase().includes(query)
					);
				});
				return json(route, {
					items: filtered,
					page: 1,
					page_size: 20,
					total: filtered.length,
				});
			}
		}

		if (req.method() === "PATCH" && pathname.startsWith("/api/admin/users/")) {
			const id = Number(pathname.split("/").at(-1));
			const body = req.postDataJSON() as {
				is_admin?: boolean;
				is_disabled?: boolean;
			};
			const target = users.find((u) => u.id === id);
			if (!target) {
				return json(
					route,
					{ ok: false, error: { code: "not_found", message: "not found" } },
					404,
				);
			}
			if (typeof body.is_admin === "boolean") {
				target.is_admin = body.is_admin;
			}
			if (typeof body.is_disabled === "boolean") {
				target.is_disabled = body.is_disabled;
			}
			target.updated_at = "2026-02-25T10:00:00Z";
			return json(route, target);
		}

		return json(
			route,
			{ error: { message: `unhandled ${req.method()} ${pathname}` } },
			404,
		);
	});
}

test("admin user can manage users in admin panel", async ({ page }) => {
	await installBaseMocks(page, { isAdmin: true });
	await page.goto("/");

	await page.getByRole("link", { name: "管理员面板" }).click();
	await expect(page).toHaveURL(/\/admin$/);
	await expect(page.getByRole("heading", { name: "管理员面板" })).toBeVisible();
	await expect(page.getByRole("heading", { name: "用户管理" })).toBeVisible();

	const userRow = page
		.getByText("octo-user", { exact: false })
		.first()
		.locator("xpath=ancestor::div[contains(@class,'bg-card')][1]");
	await expect(userRow).toContainText("普通用户");
	await userRow.getByRole("button", { name: "设为管理员" }).click();
	await expect(
		page.getByRole("heading", { name: "确认管理员变更" }),
	).toBeVisible();
	await page.getByRole("button", { name: "确认更改" }).click();
	await expect(userRow).toContainText("管理员");

	await userRow.getByRole("button", { name: "禁用" }).click();
	await expect(userRow).toContainText("已禁用");
});

test("non-admin user cannot stay on admin route", async ({ page }) => {
	await installBaseMocks(page, { isAdmin: false, adminApiForbidden: true });
	await page.goto("/admin");

	await expect(page).toHaveURL("/");
	await expect(page.getByRole("link", { name: "管理员面板" })).toHaveCount(0);
	await expect(page.getByRole("heading", { name: "用户管理" })).toHaveCount(0);
});
