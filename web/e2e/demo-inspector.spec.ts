import { expect, test } from "@playwright/test";

test("demo inspector stays fully usable when toast feedback appears", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1366, height: 768 });

	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);

	const inspector = page.locator('[data-demo-inspector-chrome="desktop"]');
	await expect(inspector).toBeVisible();

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
});
