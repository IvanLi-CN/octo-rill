import { expect, test, type Page } from "@playwright/test";

type VersionApiMockOptions = {
	versionMode: "success" | "failure";
	healthMode: "success" | "failure";
	getVersion?: () => string;
	getHealthVersion?: () => string;
};

async function accelerateVersionPolling(page: Page) {
	await page.addInitScript(() => {
		const originalSetInterval = window.setInterval.bind(window);
		window.setInterval = ((
			handler: TimerHandler,
			timeout?: number,
			...args
		) => {
			const effectiveTimeout =
				typeof timeout === "number" && timeout >= 60_000 ? 25 : timeout;
			return originalSetInterval(handler, effectiveTimeout, ...args);
		}) as typeof window.setInterval;
	});
}

async function mockVersionApis(page: Page, options: VersionApiMockOptions) {
	const {
		versionMode,
		healthMode,
		getVersion = () => "2.4.6",
		getHealthVersion = () => "1.8.0",
	} = options;

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
			if (versionMode === "success") {
				return route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify({
						ok: true,
						version: getVersion(),
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
			if (healthMode === "success") {
				return route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify({ ok: true, version: getHealthVersion() }),
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

test("footer shows version from /api/version and keeps shell notice hidden in steady state", async ({
	page,
}) => {
	await mockVersionApis(page, {
		versionMode: "success",
		healthMode: "failure",
		getVersion: () => "2.4.6",
	});
	await page.goto("/");
	await expect(page.getByText("Version v2.4.6")).toBeVisible();
	await expect(page.locator("[data-version-update-notice]")).toHaveCount(0);
});

test("footer falls back to /api/health when /api/version fails", async ({
	page,
}) => {
	await mockVersionApis(page, {
		versionMode: "failure",
		healthMode: "success",
		getHealthVersion: () => "1.8.0",
	});
	await page.goto("/");
	await expect(page.getByText("Version v1.8.0")).toBeVisible();
});

test("footer shows unknown when both version endpoints fail", async ({
	page,
}) => {
	await mockVersionApis(page, {
		versionMode: "failure",
		healthMode: "failure",
	});
	await page.goto("/");
	await expect(page.getByText("Version unknown")).toBeVisible();
	await expect(page.locator("[data-version-update-notice]")).toHaveCount(0);
});

test("app shell shows a subtle update notice when /api/version changes while footer keeps the loaded version", async ({
	page,
}) => {
	await accelerateVersionPolling(page);
	let currentVersion = "2.4.6";
	await mockVersionApis(page, {
		versionMode: "success",
		healthMode: "failure",
		getVersion: () => currentVersion,
	});
	await page.goto("/");
	await expect(page.getByText("Version v2.4.6")).toBeVisible();
	await expect(page.locator("[data-version-update-notice]")).toHaveCount(0);

	currentVersion = "2.5.0";

	await expect(page.locator("[data-version-update-notice]")).toContainText(
		"检测到新版本 v2.5.0",
	);
	await expect(page.getByText("Version v2.4.6")).toBeVisible();
});

test("app shell can detect updates via /api/health fallback polling", async ({
	page,
}) => {
	await accelerateVersionPolling(page);
	let healthVersion = "1.8.0";
	await mockVersionApis(page, {
		versionMode: "failure",
		healthMode: "success",
		getHealthVersion: () => healthVersion,
	});
	await page.goto("/");
	await expect(page.getByText("Version v1.8.0")).toBeVisible();

	healthVersion = "1.8.1";

	await expect(page.locator("[data-version-update-notice]")).toContainText(
		"检测到新版本 v1.8.1",
	);
	await expect(page.getByText("Version v1.8.0")).toBeVisible();
});

test("refreshing from the update notice reloads the page into the new version steady state", async ({
	page,
}) => {
	await accelerateVersionPolling(page);
	let currentVersion = "2.4.6";
	await mockVersionApis(page, {
		versionMode: "success",
		healthMode: "failure",
		getVersion: () => currentVersion,
	});
	await page.goto("/");
	await expect(page.getByText("Version v2.4.6")).toBeVisible();

	currentVersion = "2.5.0";

	await expect(page.locator("[data-version-update-notice]")).toContainText(
		"检测到新版本 v2.5.0",
	);
	await page.getByRole("button", { name: "刷新" }).click();

	await expect(page.getByText("Version v2.5.0")).toBeVisible();
	await expect(page.locator("[data-version-update-notice]")).toHaveCount(0);
});
