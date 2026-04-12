import { expect, test, type Page } from "@playwright/test";

const TECH_HINT_PATTERN =
	/(?:dev 环境|Vite|\/api 和 \/auth proxy|proxy 到 Rust 后端)/i;
const LEGACY_COPY_PATTERN =
	/(?:Start here|为 GitHub Release 阅读而生|使用 GitHub 登录)/i;

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

	const loginButton = page.getByRole("link", { name: "连接到 GitHub" });
	await expect(loginButton).toBeVisible();
	await expect(loginButton).toHaveAttribute("href", "/auth/github/login");
	await expect(
		page.getByRole("heading", {
			name: "把和你有关的 GitHub 动态放到一个首页里",
		}),
	).toBeVisible();
	await expect(
		page.getByText(
			"这里集中看 Releases、被加星、被关注和 Inbox；Release 默认提供中文翻译。",
		),
	).toBeVisible();
	await expect(page.getByText("Releases 信息流")).toBeVisible();
	await expect(page.getByText("被加星 / 被关注")).toBeVisible();
	await expect(page.getByText("日报 + Inbox 入口")).toBeVisible();
	await expect(page.getByText(TECH_HINT_PATTERN)).toHaveCount(0);
	await expect(page.getByText(LEGACY_COPY_PATTERN)).toHaveCount(0);
});

test("landing page keeps boot error visible while dev proxy tip stays hidden", async ({
	page,
}) => {
	await installLandingApiMocks(page, 500, "boot exploded");

	await page.goto("/");

	const loginButton = page.getByRole("link", { name: "连接到 GitHub" });
	await expect(loginButton).toBeVisible();
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

	const loginButton = page.getByRole("link", { name: "连接到 GitHub" });
	await expect(loginButton).toBeVisible();

	const viewport = await page.evaluate(() => ({
		scrollY: window.scrollY,
		height: window.innerHeight,
	}));
	expect(viewport.scrollY).toBe(0);

	const buttonRect = await loginButton.evaluate((element) => {
		const rect = element.getBoundingClientRect();
		return {
			top: rect.top,
			bottom: rect.bottom,
			height: rect.height,
		};
	});

	expect(buttonRect.top).toBeGreaterThanOrEqual(0);
	expect(buttonRect.height).toBeGreaterThan(0);
	expect(buttonRect.bottom).toBeLessThanOrEqual(viewport.height);
});
