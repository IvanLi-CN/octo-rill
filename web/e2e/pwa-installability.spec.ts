import { expect, test, type Page } from "@playwright/test";

async function installAnonymousApiMocks(page: Page) {
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
					version: "0.1.0",
					source: "APP_EFFECTIVE_VERSION",
				}),
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

test("app exposes installable PWA metadata without blocking anonymous login", async ({
	page,
}) => {
	await installAnonymousApiMocks(page);
	await page.goto("/");

	await expect(page.locator('link[rel="manifest"]')).toHaveAttribute(
		"href",
		"/manifest.webmanifest",
	);
	await expect(page.locator('link[rel="apple-touch-icon"]')).toHaveAttribute(
		"href",
		"/pwa/apple-touch-icon.png",
	);
	await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute(
		"content",
		"#0f172a",
	);
	await expect(
		page.getByRole("link", { name: "使用 GitHub 登录" }),
	).toBeVisible();

	const manifestResponse = await page.request.get("/manifest.webmanifest");
	expect(manifestResponse.ok()).toBe(true);
	const manifest = (await manifestResponse.json()) as {
		name?: string;
		display?: string;
		icons?: Array<{ src?: string; purpose?: string }>;
	};
	expect(manifest.name).toBe("OctoRill");
	expect(manifest.display).toBe("standalone");
	expect(
		manifest.icons?.some((icon) => icon.purpose?.includes("maskable")),
	).toBe(true);
});
