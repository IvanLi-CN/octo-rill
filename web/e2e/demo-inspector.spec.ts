import { expect, test } from "@playwright/test";

test("demo auth affordances stay inside mock runtime", async ({ page }) => {
	await page.goto("/?demo=landing-welcome");

	const loginLink = page.getByRole("link", { name: "使用 GitHub 登录" });
	await expect(loginLink).toHaveAttribute("href", /demo=landing-welcome/);
	const href = await loginLink.getAttribute("href");
	expect(href).not.toBeNull();

	await page.goto(href!);

	await expect(page).toHaveURL(/demo=landing-welcome/);
	await expect(
		page.locator('[data-dashboard-brand-heading="true"]'),
	).toBeVisible();
});

test("switching demo persona reseeds the auth surface", async ({ page }) => {
	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);

	const inspector = page.locator('[data-demo-inspector-chrome="desktop"]');
	await expect(
		page.locator('[data-dashboard-brand-heading="true"]'),
	).toBeVisible();

	await inspector.getByRole("combobox").nth(1).click();
	await page.getByRole("option", { name: "Guest", exact: true }).click();

	await expect(page).toHaveURL(/d_persona=guest/);
	await expect(page.locator("[data-landing-login-cta]")).toBeVisible();
	await expect(
		page.locator('[data-dashboard-brand-heading="true"]'),
	).toHaveCount(0);
});

test("settings scene share url preserves existing route query", async ({
	page,
}) => {
	await page.goto("/settings?section=my-releases&demo=settings-my-releases");

	await expect(page).toHaveURL(/section=my-releases/);
	await expect(
		page.locator('[data-demo-inspector-chrome="desktop"]'),
	).toContainText("/settings?section=my-releases&demo=settings-my-releases");
	await expect(
		page.locator('[data-demo-inspector-chrome="desktop"]'),
	).not.toContainText("/settings?section=my-releases?demo=");
});

test("demo inspector stays fully usable when toast feedback appears", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1366, height: 768 });

	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);

	const inspector = page.locator('[data-demo-inspector-chrome="desktop"]');
	await expect(inspector).toBeVisible();
	await expect(
		page.locator('[data-demo-inspector-scroll-cue="bottom"]'),
	).toBeVisible();

	await page.getByRole("button", { name: "发布公开页" }).click();

	const toast = page.locator('[data-slot="toast"][data-state="open"]').first();
	await expect(toast).toBeVisible();

	const geometry = await page.evaluate(() => {
		const panel = document.querySelector<HTMLElement>(
			'[data-demo-inspector-chrome="desktop"]',
		);
		const surface = document.querySelector<HTMLElement>(
			'[data-demo-inspector-surface="true"]',
		);
		const scroller = document.querySelector<HTMLElement>(
			'[data-demo-inspector-scroller="true"]',
		);
		const title = document.querySelector<HTMLElement>(
			'[data-demo-inspector-title="true"]',
		);
		const toastElement = document.querySelector<HTMLElement>(
			'[data-slot="toast"][data-state="open"]',
		);
		if (!panel || !surface || !scroller || !title) {
			throw new Error("demo inspector geometry nodes are missing");
		}
		const panelRect = panel.getBoundingClientRect();
		const surfaceRect = surface.getBoundingClientRect();
		const scrollerRect = scroller.getBoundingClientRect();
		const titleRect = title.getBoundingClientRect();
		const toastRect = toastElement?.getBoundingClientRect() ?? null;
		return {
			viewportHeight: window.innerHeight,
			panelTop: panelRect.top,
			panelBottom: panelRect.bottom,
			surfaceTop: surfaceRect.top,
			surfaceBottom: surfaceRect.bottom,
			scrollerBottom: scrollerRect.bottom,
			scrollerCanScroll: scroller.scrollHeight > scroller.clientHeight,
			titleTop: titleRect.top,
			toastBottom: toastRect?.bottom ?? 0,
			toastLeft: toastRect?.left ?? 0,
			toastRight: toastRect?.right ?? 0,
			panelLeft: panelRect.left,
			panelRight: panelRect.right,
		};
	});

	expect(geometry.panelBottom).toBeLessThanOrEqual(geometry.viewportHeight - 1);
	expect(geometry.surfaceBottom).toBeLessThanOrEqual(
		geometry.viewportHeight - 1,
	);
	expect(geometry.scrollerBottom).toBeLessThanOrEqual(
		geometry.viewportHeight - 1,
	);
	expect(geometry.titleTop).toBeGreaterThanOrEqual(geometry.panelTop);
	expect(geometry.surfaceTop).toBeGreaterThanOrEqual(geometry.panelTop);
	expect(geometry.scrollerCanScroll).toBe(true);

	const overlapsToastHorizontally =
		geometry.panelRight > geometry.toastLeft &&
		geometry.panelLeft < geometry.toastRight;
	if (overlapsToastHorizontally) {
		expect(geometry.titleTop).toBeGreaterThanOrEqual(geometry.toastBottom + 4);
	}

	await page
		.locator('[data-demo-inspector-scroller="true"]')
		.evaluate((element) => {
			element.scrollTo({
				top: element.scrollHeight,
				behavior: "instant",
			});
		});

	await expect(
		page.locator('[data-demo-inspector-scroll-cue="top"]'),
	).toBeVisible();
	await expect(
		page.locator('[data-demo-inspector-scroll-cue="bottom"]'),
	).toBeHidden();
});

test("demo inspector grows to fit the full control stack on tall desktops", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1798, height: 1360 });
	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);

	await page.getByRole("button", { name: "发布公开页" }).click();

	const inspector = page.locator('[data-demo-inspector-chrome="desktop"]');
	await expect(inspector).toBeVisible();
	await expect(inspector.getByText("Actions & Share")).toBeVisible();
	await expect(inspector.getByText("Advanced")).toBeVisible();
	await expect(
		page.locator('[data-demo-inspector-scroll-cue="bottom"]'),
	).toBeHidden();

	const geometry = await page.evaluate(() => {
		const panel = document.querySelector<HTMLElement>(
			'[data-demo-inspector-chrome="desktop"]',
		);
		const scroller = document.querySelector<HTMLElement>(
			'[data-demo-inspector-scroller="true"]',
		);
		if (!panel || !scroller) {
			throw new Error("demo inspector geometry nodes are missing");
		}
		const panelRect = panel.getBoundingClientRect();
		return {
			viewportHeight: window.innerHeight,
			panelBottom: panelRect.bottom,
			bottomGap: window.innerHeight - panelRect.bottom,
			scrollHeight: scroller.scrollHeight,
			clientHeight: scroller.clientHeight,
		};
	});

	expect(geometry.scrollHeight).toBeLessThanOrEqual(geometry.clientHeight + 2);
	expect(geometry.panelBottom).toBeLessThanOrEqual(geometry.viewportHeight - 1);
	expect(geometry.bottomGap).toBeGreaterThan(0);
});

test("reset scene restores the default publication share state", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1366, height: 768 });
	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);

	const inspector = page.locator('[data-demo-inspector-chrome="desktop"]');
	await inspector.getByRole("combobox").nth(3).click();
	await page.getByRole("option", { name: "Published", exact: true }).click();

	await expect(page).toHaveURL(/d_pub=published/);
	await expect(page.getByRole("button", { name: "发布公开页" })).toHaveCount(0);
	await expect(page.getByRole("button", { name: "取消发布" })).toBeVisible();

	await page.getByRole("button", { name: "Reset Scene" }).click();

	await expect(page).not.toHaveURL(/d_pub=published/);
	await expect(page.getByRole("button", { name: "发布公开页" })).toBeVisible();
	await expect(page.getByRole("button", { name: "取消发布" })).toHaveCount(0);
});
