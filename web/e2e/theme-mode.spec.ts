import { expect, test, type Page } from "@playwright/test";

async function mockLandingApis(page: Page) {
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

		if (req.method() === "GET" && url.pathname === "/api/version") {
			return route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					ok: true,
					version: "2.4.6",
					source: "APP_EFFECTIVE_VERSION",
				}),
			});
		}

		if (req.method() === "GET" && url.pathname === "/api/health") {
			return route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({ ok: true, version: "2.4.6" }),
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
}

async function chooseTheme(page: Page, label: "浅色" | "深色" | "跟随系统") {
	await page
		.locator("[data-theme-toggle]")
		.getByRole("button", { name: label })
		.click();
}

test("initial load follows dark system theme before any explicit preference", async ({
	page,
}) => {
	await page.emulateMedia({ colorScheme: "dark" });
	await mockLandingApis(page);
	await page.goto("/");

	await expect(
		page.locator("[data-theme-toggle]").getByRole("button", {
			name: "跟随系统",
		}),
	).toHaveAttribute("aria-pressed", "true");
	await expect
		.poll(() =>
			page.evaluate(() => ({
				preference: window.localStorage.getItem("octo-rill.theme-preference"),
				resolvedTheme: document.documentElement.dataset.theme,
				isDark: document.documentElement.classList.contains("dark"),
			})),
		)
		.toEqual({
			preference: "system",
			resolvedTheme: "dark",
			isDark: true,
		});
});

test("manual dark preference persists after reload and ignores system changes", async ({
	page,
}) => {
	await page.emulateMedia({ colorScheme: "light" });
	await mockLandingApis(page);
	await page.goto("/");

	await chooseTheme(page, "深色");
	await expect(
		page.locator("[data-theme-toggle]").getByRole("button", { name: "深色" }),
	).toHaveAttribute("aria-pressed", "true");

	await expect
		.poll(() =>
			page.evaluate(() => ({
				preference: window.localStorage.getItem("octo-rill.theme-preference"),
				resolvedTheme: document.documentElement.dataset.theme,
				isDark: document.documentElement.classList.contains("dark"),
			})),
		)
		.toEqual({
			preference: "dark",
			resolvedTheme: "dark",
			isDark: true,
		});

	await page.reload();
	await page.emulateMedia({ colorScheme: "light" });
	await expect
		.poll(() =>
			page.evaluate(() => ({
				preference: window.localStorage.getItem("octo-rill.theme-preference"),
				resolvedTheme: document.documentElement.dataset.theme,
				isDark: document.documentElement.classList.contains("dark"),
			})),
		)
		.toEqual({
			preference: "dark",
			resolvedTheme: "dark",
			isDark: true,
		});
});

test("system preference responds to media changes after the user reselects it", async ({
	page,
}) => {
	await page.emulateMedia({ colorScheme: "light" });
	await mockLandingApis(page);
	await page.goto("/");

	await chooseTheme(page, "深色");
	await chooseTheme(page, "跟随系统");

	await expect(
		page
			.locator("[data-theme-toggle]")
			.getByRole("button", { name: "跟随系统" }),
	).toHaveAttribute("aria-pressed", "true");

	await page.emulateMedia({ colorScheme: "dark" });
	await expect
		.poll(() =>
			page.evaluate(() => ({
				preference: window.localStorage.getItem("octo-rill.theme-preference"),
				resolvedTheme: document.documentElement.dataset.theme,
				isDark: document.documentElement.classList.contains("dark"),
			})),
		)
		.toEqual({
			preference: "system",
			resolvedTheme: "dark",
			isDark: true,
		});

	await page.emulateMedia({ colorScheme: "light" });
	await expect
		.poll(() =>
			page.evaluate(() => ({
				preference: window.localStorage.getItem("octo-rill.theme-preference"),
				resolvedTheme: document.documentElement.dataset.theme,
				isDark: document.documentElement.classList.contains("dark"),
			})),
		)
		.toEqual({
			preference: "system",
			resolvedTheme: "light",
			isDark: false,
		});
});
