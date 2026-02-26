import { expect, test } from "@playwright/test";

const TECH_HINT_PATTERN =
	/(?:dev 环境|Vite|\/api 和 \/auth proxy|proxy 到 Rust 后端)/i;

test("landing page hides dev proxy tip for unauthenticated users", async ({
	page,
}) => {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());

		if (req.method() === "GET" && url.pathname === "/api/me") {
			return route.fulfill({
				status: 401,
				contentType: "application/json",
				body: JSON.stringify({
					error: {
						code: "unauthorized",
						message: "unauthorized",
					},
				}),
			});
		}

		if (req.method() === "GET" && url.pathname === "/api/health") {
			return route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({ ok: true, version: "1.2.3" }),
			});
		}

		return route.fulfill({
			status: 404,
			contentType: "application/json",
			body: JSON.stringify({
				error: {
					code: "not_found",
					message: `unhandled ${req.method()} ${url.pathname}`,
				},
			}),
		});
	});

	await page.goto("/");

	const loginButton = page.getByRole("link", { name: "使用 GitHub 登录" });
	await expect(loginButton).toBeVisible();
	await expect(loginButton).toHaveAttribute("href", "/auth/github/login");
	await expect(page.getByText(TECH_HINT_PATTERN)).toHaveCount(0);
});

test("landing page keeps boot error visible while dev proxy tip stays hidden", async ({
	page,
}) => {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());

		if (req.method() === "GET" && url.pathname === "/api/me") {
			return route.fulfill({
				status: 500,
				contentType: "application/json",
				body: JSON.stringify({
					error: {
						code: "boot_failed",
						message: "boot exploded",
					},
				}),
			});
		}

		if (req.method() === "GET" && url.pathname === "/api/health") {
			return route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({ ok: true, version: "1.2.3" }),
			});
		}

		return route.fulfill({
			status: 404,
			contentType: "application/json",
			body: JSON.stringify({
				error: {
					code: "not_found",
					message: `unhandled ${req.method()} ${url.pathname}`,
				},
			}),
		});
	});

	await page.goto("/");

	const loginButton = page.getByRole("link", { name: "使用 GitHub 登录" });
	await expect(loginButton).toBeVisible();
	await expect(page.getByText("boot exploded")).toBeVisible();
	await expect(page.getByText(TECH_HINT_PATTERN)).toHaveCount(0);
});
