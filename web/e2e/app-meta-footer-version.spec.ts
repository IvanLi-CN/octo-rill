import { expect, test, type Page } from "@playwright/test";

type VersionApiMockOptions = {
	versionMode: "success" | "failure";
	healthMode: "success" | "failure";
	getVersion?: () => string;
	getHealthVersion?: () => string;
};

const EMBEDDED_FRONTEND_VERSION = "v0.1.0";

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
		getVersion = () => "0.1.0",
		getHealthVersion = () => "0.1.0",
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

test("footer shows the embedded frontend build version in steady state", async ({
	page,
}) => {
	await mockVersionApis(page, {
		versionMode: "success",
		healthMode: "failure",
		getVersion: () => "0.1.0",
	});
	await page.goto("/");
	await expect(
		page.getByText(`Version ${EMBEDDED_FRONTEND_VERSION}`),
	).toBeVisible();
	await expect(page.locator("[data-version-update-notice]")).toHaveCount(0);
});

test("footer keeps the embedded version when /api/version fails and /api/health agrees", async ({
	page,
}) => {
	await mockVersionApis(page, {
		versionMode: "failure",
		healthMode: "success",
		getHealthVersion: () => "0.1.0",
	});
	await page.goto("/");
	await expect(
		page.getByText(`Version ${EMBEDDED_FRONTEND_VERSION}`),
	).toBeVisible();
	await expect(page.locator("[data-version-update-notice]")).toHaveCount(0);
});

test("footer keeps the embedded version when version polling endpoints fail", async ({
	page,
}) => {
	await mockVersionApis(page, {
		versionMode: "failure",
		healthMode: "failure",
	});
	await page.goto("/");
	await expect(
		page.getByText(`Version ${EMBEDDED_FRONTEND_VERSION}`),
	).toBeVisible();
	await expect(page.locator("[data-version-update-notice]")).toHaveCount(0);
});

test("app shell shows an update notice when backend version differs from the embedded frontend version", async ({
	page,
}) => {
	await accelerateVersionPolling(page);
	let currentVersion = "0.1.0";
	await mockVersionApis(page, {
		versionMode: "success",
		healthMode: "failure",
		getVersion: () => currentVersion,
	});
	await page.goto("/");
	await expect(
		page.getByText(`Version ${EMBEDDED_FRONTEND_VERSION}`),
	).toBeVisible();
	await expect(page.locator("[data-version-update-notice]")).toHaveCount(0);

	currentVersion = "0.1.1";

	await expect(page.locator("[data-version-update-notice]")).toContainText(
		"检测到新版本 v0.1.1",
	);
	await expect(
		page.getByText(`Version ${EMBEDDED_FRONTEND_VERSION}`),
	).toBeVisible();
});

test("app shell can detect updates via /api/health fallback polling", async ({
	page,
}) => {
	await accelerateVersionPolling(page);
	let healthVersion = "0.1.0";
	await mockVersionApis(page, {
		versionMode: "failure",
		healthMode: "success",
		getHealthVersion: () => healthVersion,
	});
	await page.goto("/");
	await expect(
		page.getByText(`Version ${EMBEDDED_FRONTEND_VERSION}`),
	).toBeVisible();

	healthVersion = "0.1.1";

	await expect(page.locator("[data-version-update-notice]")).toContainText(
		"检测到新版本 v0.1.1",
	);
	await expect(
		page.getByText(`Version ${EMBEDDED_FRONTEND_VERSION}`),
	).toBeVisible();
});
