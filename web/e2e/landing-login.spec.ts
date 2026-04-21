import { expect, test, type Page } from "@playwright/test";

const TECH_HINT_PATTERN =
	/(?:dev 环境|Vite|\/api 和 \/auth proxy|proxy 到 Rust 后端)/i;
const LEGACY_COPY_PATTERN =
	/(?:Start here|为 GitHub Release 阅读而生|连接你的账号|连接到 GitHub)/i;

async function installLandingApiMocks(
	page: Page,
	meStatus: 401 | 500,
	message: string,
) {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());

		if (req.method() === "GET" && url.pathname === "/api/me") {
			return route.fulfill({
				status: meStatus,
				contentType: "application/json",
				body: JSON.stringify({
					error: {
						code: meStatus === 401 ? "unauthorized" : "boot_failed",
						message,
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
}

test("landing page shows concise login copy for unauthenticated users", async ({
	page,
}) => {
	await installLandingApiMocks(page, 401, "unauthorized");

	await page.goto("/");

	const loginButton = page.getByRole("link", { name: "使用 GitHub 登录" });
	const linuxDoButton = page.getByRole("link", { name: "使用 LinuxDO 登录" });
	await expect(loginButton).toBeVisible();
	await expect(linuxDoButton).toBeVisible();
	await expect(
		loginButton.locator('[data-auth-provider-icon="github"]'),
	).toBeVisible();
	await expect(
		linuxDoButton.locator('[data-auth-provider-icon="linuxdo"]'),
	).toBeVisible();
	await expect(loginButton).toHaveAttribute("href", "/auth/github/login");
	await expect(linuxDoButton).toHaveAttribute("href", "/auth/linuxdo/login");
	await expect(
		page.getByRole("heading", {
			name: "集中查看与你相关的 GitHub 动态",
		}),
	).toBeVisible();
	await expect(
		page.getByText(
			"登录后可在同一页面查看发布更新、获星与关注动态，并使用日报与通知入口；发布内容支持中文翻译与要点整理。",
		),
	).toBeVisible();
	await expect(page.getByText("发布更新", { exact: true })).toBeVisible();
	await expect(page.getByText("社交动态", { exact: true })).toBeVisible();
	await expect(page.getByText("日报通知", { exact: true })).toBeVisible();
	await expect(page.getByText("查看发布译文与要点")).toBeVisible();
	await expect(page.getByText("查看获星与关注变化")).toBeVisible();
	await expect(page.getByText(TECH_HINT_PATTERN)).toHaveCount(0);
	await expect(page.getByText(LEGACY_COPY_PATTERN)).toHaveCount(0);
});

test("landing page keeps boot error visible while dev proxy tip stays hidden", async ({
	page,
}) => {
	await installLandingApiMocks(page, 500, "boot exploded");

	await page.goto("/");

	const loginButton = page.getByRole("link", { name: "使用 GitHub 登录" });
	const linuxDoButton = page.getByRole("link", { name: "使用 LinuxDO 登录" });
	await expect(loginButton).toBeVisible();
	await expect(linuxDoButton).toBeVisible();
	await expect(
		loginButton.locator('[data-auth-provider-icon="github"]'),
	).toBeVisible();
	await expect(
		linuxDoButton.locator('[data-auth-provider-icon="linuxdo"]'),
	).toBeVisible();
	await expect(page.getByText("boot exploded")).toBeVisible();
	await expect(page.getByText(TECH_HINT_PATTERN)).toHaveCount(0);
	await expect(page.getByText(LEGACY_COPY_PATTERN)).toHaveCount(0);
});

test("landing page keeps the GitHub CTA above the fold on mobile", async ({
	page,
}) => {
	await page.setViewportSize({ width: 375, height: 667 });
	await installLandingApiMocks(page, 401, "unauthorized");

	await page.goto("/");

	const loginButton = page.getByRole("link", { name: "使用 GitHub 登录" });
	const linuxDoButton = page.getByRole("link", { name: "使用 LinuxDO 登录" });
	await expect(loginButton).toBeVisible();
	await expect(linuxDoButton).toBeVisible();
	await expect(
		loginButton.locator('[data-auth-provider-icon="github"]'),
	).toBeVisible();
	await expect(
		linuxDoButton.locator('[data-auth-provider-icon="linuxdo"]'),
	).toBeVisible();

	const viewport = await page.evaluate(() => ({
		scrollY: window.scrollY,
		height: window.innerHeight,
	}));
	expect(viewport.scrollY).toBe(0);

	const githubRect = await loginButton.evaluate((element) => {
		const rect = element.getBoundingClientRect();
		return {
			top: rect.top,
			bottom: rect.bottom,
			height: rect.height,
		};
	});
	const linuxDoRect = await linuxDoButton.evaluate((element) => {
		const rect = element.getBoundingClientRect();
		return {
			top: rect.top,
			bottom: rect.bottom,
			height: rect.height,
		};
	});

	expect(githubRect.top).toBeGreaterThanOrEqual(0);
	expect(githubRect.height).toBeGreaterThan(0);
	expect(githubRect.bottom).toBeLessThanOrEqual(viewport.height);
	expect(linuxDoRect.top).toBeGreaterThanOrEqual(0);
	expect(linuxDoRect.height).toBeGreaterThan(0);
	expect(linuxDoRect.bottom).toBeLessThanOrEqual(viewport.height);
});
