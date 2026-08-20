import { expect, test, type Page } from "@playwright/test";

import {
	installPasskeyBrowserMock,
	installPasskeyUnsupportedBrowser,
} from "./passkeyHelpers";

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

async function installPendingAuthRedirect(
	page: Page,
	provider: "github" | "linuxdo",
) {
	let requestCount = 0;
	let releaseNavigation: (() => void) | undefined;
	const navigationReleased = new Promise<void>((resolve) => {
		releaseNavigation = resolve;
	});
	const routePattern = `**/auth/${provider}/login`;

	await page.route(routePattern, async (route) => {
		requestCount += 1;
		await navigationReleased;
		await route.abort();
	});

	return {
		count: () => requestCount,
		release: async () => {
			releaseNavigation?.();
			await page.unroute(routePattern).catch(() => undefined);
		},
	};
}

async function installPendingPasskeyOptions(
	page: Page,
	action: "authenticate" | "register",
) {
	let requestCount = 0;
	let releaseOptions: (() => void) | undefined;
	const optionsReleased = new Promise<void>((resolve) => {
		releaseOptions = resolve;
	});
	const routePattern = `**/api/auth/passkeys/${action}/options`;

	await page.route(routePattern, async (route) => {
		requestCount += 1;
		await optionsReleased;
		await route.abort();
	});

	return {
		count: () => requestCount,
		release: async () => {
			releaseOptions?.();
			await page.unroute(routePattern).catch(() => undefined);
		},
	};
}

test("landing page shows concise login copy for unauthenticated users", async ({
	page,
}) => {
	await installPasskeyBrowserMock(page);
	await installLandingApiMocks(page, 401, "unauthorized");

	await page.goto("/");

	const loginButton = page.getByRole("link", { name: "使用 GitHub 登录" });
	const linuxDoButton = page.getByRole("link", { name: "使用 LinuxDO 登录" });
	const passkeyLoginButton = page.getByRole("button", {
		name: "使用 Passkey 登录",
	});
	const passkeyRegisterButton = page.getByRole("button", {
		name: "首次使用？创建 Passkey 并继续绑定 GitHub",
	});
	await expect(loginButton).toBeVisible();
	await expect(linuxDoButton).toBeVisible();
	await expect(passkeyLoginButton).toBeVisible();
	await expect(passkeyRegisterButton).toBeVisible();
	await expect(passkeyLoginButton).toBeEnabled();
	await expect(passkeyRegisterButton).toBeEnabled();
	await expect(
		loginButton.locator('[data-auth-provider-icon="github"]'),
	).toBeVisible();
	await expect(
		linuxDoButton.locator('[data-auth-provider-icon="linuxdo"]'),
	).toBeVisible();
	await expect(
		passkeyLoginButton.locator('[data-auth-provider-icon="passkey"]'),
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
	await installPasskeyBrowserMock(page);
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
	await installPasskeyBrowserMock(page);
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

test("landing page disables passkey actions when browser support is unavailable", async ({
	page,
}) => {
	await installPasskeyUnsupportedBrowser(page);
	await installLandingApiMocks(page, 401, "unauthorized");

	await page.goto("/");

	const passkeyLoginButton = page.getByRole("button", {
		name: "使用 Passkey 登录",
	});
	const passkeyRegisterButton = page.getByRole("button", {
		name: "首次使用？创建 Passkey 并继续绑定 GitHub",
	});

	await expect(passkeyLoginButton).toBeDisabled();
	await expect(passkeyRegisterButton).toBeDisabled();
	await expect(
		page.getByText(
			"当前浏览器不支持 Passkey；你仍然可以继续使用 GitHub / LinuxDO 登录。",
		),
	).toBeVisible();
});

test("landing page preserves native modified and middle-click OAuth semantics", async ({
	page,
}) => {
	await installPasskeyBrowserMock(page);
	await installLandingApiMocks(page, 401, "unauthorized");
	await page
		.context()
		.route("**/auth/github/login", (route) => route.fulfill({ status: 204 }));

	await page.goto("/");
	await expect(page.locator("[data-landing-login-cta]")).toBeVisible({
		timeout: 15_000,
	});

	const interaction = await page.evaluate(() => {
		const link = document.querySelector<HTMLAnchorElement>(
			"[data-landing-login-cta]",
		);
		if (!link) throw new Error("GitHub login link not found");

		const modifiedClick = new MouseEvent("click", {
			bubbles: true,
			cancelable: true,
			button: 0,
			ctrlKey: true,
		});
		link.dispatchEvent(modifiedClick);

		const middleClick = new MouseEvent("click", {
			bubbles: true,
			cancelable: true,
			button: 1,
		});
		link.dispatchEvent(middleClick);

		return {
			href: link.getAttribute("href"),
			modifiedDefaultPrevented: modifiedClick.defaultPrevented,
			middleDefaultPrevented: middleClick.defaultPrevented,
			ariaDisabled: link.getAttribute("aria-disabled"),
		};
	});

	expect(interaction).toEqual({
		href: "/auth/github/login",
		modifiedDefaultPrevented: false,
		middleDefaultPrevented: false,
		ariaDisabled: null,
	});
});

test("landing page locks every login action while GitHub OAuth is starting", async ({
	page,
}) => {
	await installPasskeyBrowserMock(page);
	await installLandingApiMocks(page, 401, "unauthorized");
	const pendingAuth = await installPendingAuthRedirect(page, "github");

	try {
		await page.goto("/");
		await expect(page.locator("[data-landing-login-cta]")).toBeVisible({
			timeout: 15_000,
		});

		const pendingState = await page.evaluate(
			() =>
				new Promise((resolve) => {
					const link = document.querySelector<HTMLAnchorElement>(
						"[data-landing-login-cta]",
					);
					if (!link) throw new Error("GitHub login link not found");
					for (let index = 0; index < 2; index += 1) {
						link.dispatchEvent(
							new MouseEvent("click", {
								bubbles: true,
								cancelable: true,
								button: 0,
							}),
						);
					}
					requestAnimationFrame(() => {
						resolve({
							githubName: link.textContent?.trim(),
							githubDisabled: link.getAttribute("aria-disabled"),
							linuxdoDisabled: document
								.querySelector("[data-landing-linuxdo-cta]")
								?.getAttribute("aria-disabled"),
							passkeyLoginDisabled: document.querySelector<HTMLButtonElement>(
								"[data-landing-passkey-login-cta]",
							)?.disabled,
							passkeyRegisterDisabled:
								document.querySelector<HTMLButtonElement>(
									"[data-landing-passkey-register-cta]",
								)?.disabled,
							cardBusy: document
								.querySelector("[data-landing-login-card]")
								?.getAttribute("aria-busy"),
						});
					});
				}),
		);
		expect(pendingState).toEqual({
			githubName: "正在跳转到 GitHub…",
			githubDisabled: "true",
			linuxdoDisabled: "true",
			passkeyLoginDisabled: true,
			passkeyRegisterDisabled: true,
			cardBusy: "true",
		});
		await expect.poll(() => pendingAuth.count()).toBe(1);
	} finally {
		await pendingAuth.release();
	}
});

test("landing page shows the LinuxDO redirect state without a second OAuth request", async ({
	page,
}) => {
	await installPasskeyBrowserMock(page);
	await installLandingApiMocks(page, 401, "unauthorized");
	const pendingAuth = await installPendingAuthRedirect(page, "linuxdo");

	try {
		await page.goto("/");
		await expect(page.locator("[data-landing-linuxdo-cta]")).toBeVisible({
			timeout: 15_000,
		});

		const pendingState = await page.evaluate(
			() =>
				new Promise((resolve) => {
					const link = document.querySelector<HTMLAnchorElement>(
						"[data-landing-linuxdo-cta]",
					);
					if (!link) throw new Error("LinuxDO login link not found");
					for (let index = 0; index < 2; index += 1) {
						link.dispatchEvent(
							new MouseEvent("click", {
								bubbles: true,
								cancelable: true,
								button: 0,
							}),
						);
					}
					requestAnimationFrame(() => {
						resolve({
							linuxdoName: link.textContent?.trim(),
							githubDisabled: document
								.querySelector("[data-landing-login-cta]")
								?.getAttribute("aria-disabled"),
							linuxdoDisabled: link.getAttribute("aria-disabled"),
							passkeyLoginDisabled: document.querySelector<HTMLButtonElement>(
								"[data-landing-passkey-login-cta]",
							)?.disabled,
							passkeyRegisterDisabled:
								document.querySelector<HTMLButtonElement>(
									"[data-landing-passkey-register-cta]",
								)?.disabled,
							cardBusy: document
								.querySelector("[data-landing-login-card]")
								?.getAttribute("aria-busy"),
						});
					});
				}),
		);
		expect(pendingState).toEqual({
			linuxdoName: "正在跳转到 LinuxDO…",
			githubDisabled: "true",
			linuxdoDisabled: "true",
			passkeyLoginDisabled: true,
			passkeyRegisterDisabled: true,
			cardBusy: "true",
		});
		await expect.poll(() => pendingAuth.count()).toBe(1);
	} finally {
		await pendingAuth.release();
	}
});

for (const pendingPasskey of [
	{
		action: "authenticate" as const,
		ctaSelector: "[data-landing-passkey-login-cta]",
		pendingCopy: "正在验证 Passkey…",
	},
	{
		action: "register" as const,
		ctaSelector: "[data-landing-passkey-register-cta]",
		pendingCopy: "正在创建 Passkey…",
	},
]) {
	test(`landing page locks every login action while Passkey ${pendingPasskey.action} is starting`, async ({
		page,
	}) => {
		await installPasskeyBrowserMock(page);
		await installLandingApiMocks(page, 401, "unauthorized");
		const pendingOptions = await installPendingPasskeyOptions(
			page,
			pendingPasskey.action,
		);

		try {
			await page.goto("/");
			const activePasskeyButton = page.locator(pendingPasskey.ctaSelector);
			await expect(activePasskeyButton).toBeVisible({ timeout: 15_000 });
			await activePasskeyButton.click();
			await expect.poll(() => pendingOptions.count()).toBe(1);

			await expect(activePasskeyButton).toHaveText(pendingPasskey.pendingCopy);
			await expect(activePasskeyButton).toBeDisabled();
			await expect(page.locator("[data-landing-login-cta]")).toHaveAttribute(
				"aria-disabled",
				"true",
			);
			await expect(page.locator("[data-landing-linuxdo-cta]")).toHaveAttribute(
				"aria-disabled",
				"true",
			);
			await expect(
				page.locator("[data-landing-passkey-login-cta]"),
			).toBeDisabled();
			await expect(
				page.locator("[data-landing-passkey-register-cta]"),
			).toBeDisabled();
			await expect(page.locator("[data-landing-login-card]")).toHaveAttribute(
				"aria-busy",
				"true",
			);

			const disabledOAuthClick = await page.evaluate(() => {
				const link = document.querySelector<HTMLAnchorElement>(
					"[data-landing-login-cta]",
				);
				if (!link) throw new Error("GitHub login link not found");
				const click = new MouseEvent("click", {
					bubbles: true,
					cancelable: true,
					button: 0,
				});
				link.dispatchEvent(click);
				return click.defaultPrevented;
			});
			expect(disabledOAuthClick).toBe(true);
			expect(pendingOptions.count()).toBe(1);
		} finally {
			await pendingOptions.release();
		}
	});
}
