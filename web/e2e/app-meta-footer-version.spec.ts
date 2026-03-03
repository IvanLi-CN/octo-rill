import { expect, test, type Page } from "@playwright/test";

type FooterMockMode = "version-success" | "version-fallback" | "all-failed";

async function mockFooterVersionApis(page: Page, mode: FooterMockMode) {
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
			if (mode === "version-success") {
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

			return route.fulfill({
				status: 503,
				contentType: "application/json",
				body: JSON.stringify({
					error: { code: "unavailable", message: "version unavailable" },
				}),
			});
		}

		if (req.method() === "GET" && url.pathname === "/api/health") {
			if (mode === "version-fallback") {
				return route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify({ ok: true, version: "1.8.0" }),
				});
			}

			return route.fulfill({
				status: 503,
				contentType: "application/json",
				body: JSON.stringify({
					error: { code: "unavailable", message: "health unavailable" },
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

test("footer shows version from /api/version", async ({ page }) => {
	await mockFooterVersionApis(page, "version-success");
	await page.goto("/");
	await expect(page.getByText("Version v2.4.6")).toBeVisible();
});

test("footer falls back to /api/health when /api/version fails", async ({
	page,
}) => {
	await mockFooterVersionApis(page, "version-fallback");
	await page.goto("/");
	await expect(page.getByText("Version v1.8.0")).toBeVisible();
});

test("footer shows unknown when both version endpoints fail", async ({
	page,
}) => {
	await mockFooterVersionApis(page, "all-failed");
	await page.goto("/");
	await expect(page.getByText("Version unknown")).toBeVisible();
});
