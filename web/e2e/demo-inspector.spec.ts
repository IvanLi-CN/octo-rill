import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test, type Locator } from "@playwright/test";

const DEMO_INSPECTOR_EVIDENCE_DIR = process.env.DEMO_INSPECTOR_EVIDENCE_DIR;

async function openInspectorCombobox(inspector: Locator, index: number) {
	const combobox = inspector.getByRole("combobox").nth(index);
	await combobox.evaluate((element) => {
		if (!(element instanceof HTMLButtonElement)) {
			throw new Error("demo inspector combobox is missing");
		}
		element.click();
	});
}

async function captureDemoInspectorEvidence(target: Locator, filename: string) {
	if (!DEMO_INSPECTOR_EVIDENCE_DIR) return;
	mkdirSync(DEMO_INSPECTOR_EVIDENCE_DIR, { recursive: true });
	await target.screenshot({
		path: resolve(DEMO_INSPECTOR_EVIDENCE_DIR, filename),
		animations: "disabled",
	});
}

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

test("demo mode skips live warm auth seed on first paint", async ({ page }) => {
	await page.addInitScript(() => {
		localStorage.setItem(
			"octo-rill.auth-bootstrap.v3",
			JSON.stringify({
				savedAt: Date.now(),
				me: {
					user: {
						id: "live-admin",
						github_user_id: 1,
						login: "live-admin",
						name: "Live Admin",
						avatar_url: null,
						email: "live-admin@example.com",
						is_admin: true,
					},
					access_sync: {
						task_id: null,
						task_type: null,
						event_path: null,
						reason: "none",
					},
					dashboard: {
						daily_boundary_local: "08:00",
						daily_boundary_time_zone: "Asia/Shanghai",
						daily_boundary_utc_offset_minutes: 480,
						include_own_releases: true,
					},
				},
			}),
		);

		let leaked = false;
		const markLeak = () => {
			if (document.querySelector('[data-dashboard-brand-heading="true"]')) {
				leaked = true;
			}
			(
				window as typeof window & { __demoWarmSeedLeakSeen?: boolean }
			).__demoWarmSeedLeakSeen = leaked;
		};

		markLeak();
		new MutationObserver(markLeak).observe(document.documentElement, {
			subtree: true,
			childList: true,
			attributes: true,
		});
	});

	await page.goto("/?demo=landing-welcome");

	await expect(page.locator("[data-landing-login-cta]")).toBeVisible();
	await expect(
		page.locator('[data-dashboard-brand-heading="true"]'),
	).toHaveCount(0);
	expect(
		await page.evaluate(() =>
			Boolean(
				(window as typeof window & { __demoWarmSeedLeakSeen?: boolean })
					.__demoWarmSeedLeakSeen,
			),
		),
	).toBe(false);
});

test("demo worker ignores unmarked live requests in regular dev builds", async ({
	page,
}) => {
	await page.goto("/?demo=landing-welcome");

	const result = await page.evaluate(async () => {
		try {
			const response = await fetch("/api/version", {
				credentials: "include",
			});
			const text = await response.text();
			try {
				return {
					status: response.status,
					payload: JSON.parse(text) as { source?: string } | null,
				};
			} catch {
				return {
					status: response.status,
					payload: null,
				};
			}
		} catch {
			return {
				status: null,
				payload: null,
			};
		}
	});

	expect(result.payload?.source).not.toBe("DEMO_RUNTIME");
});

test("leaving demo mode does not keep the demo auth persona cached", async ({
	page,
}) => {
	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);

	await expect(
		page.locator('[data-dashboard-brand-heading="true"]'),
	).toBeVisible();

	await page.goto("/");

	await expect(page.locator("[data-landing-login-cta]")).toBeVisible();
	await expect(
		page.locator('[data-dashboard-brand-heading="true"]'),
	).toHaveCount(0);
});

test("demo boot failure shows a safe error surface instead of a blank page", async ({
	page,
}) => {
	await page.addInitScript(() => {
		const container = window.ServiceWorkerContainer?.prototype;
		if (!container) return;
		container.register = async () => {
			throw new Error("mock register blocked");
		};
	});

	await page.goto("/?demo=landing-welcome");

	await expect(
		page.getByRole("heading", { name: "Web Demo 启动失败" }),
	).toBeVisible();
	await expect(
		page.getByText("应用没有继续渲染，以避免误触真实接口或真实认证链路。"),
	).toBeVisible();
	await expect(page.getByRole("button", { name: "重新尝试" })).toBeVisible();
	await expect(
		page.locator('[data-dashboard-brand-heading="true"]'),
	).toHaveCount(0);
});

test("switching demo persona reseeds the auth surface", async ({ page }) => {
	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);

	const inspector = page.locator('[data-demo-inspector-chrome="desktop"]');
	await expect(
		page.locator('[data-dashboard-brand-heading="true"]'),
	).toBeVisible();

	await openInspectorCombobox(inspector, 1);
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

test("demo inspector share url follows in-scene route changes", async ({
	page,
}) => {
	await page.goto("/settings?section=my-releases&demo=settings-my-releases");

	await page.getByRole("link", { name: "GitHub PAT" }).first().click();

	await expect(page).toHaveURL(
		/settings\?(?=.*section=github-pat)(?=.*demo=settings-my-releases)/,
	);
	await expect(
		page.locator('[data-demo-inspector-chrome="desktop"]'),
	).toContainText(
		/settings\?(?=.*section=github-pat)(?=.*demo=settings-my-releases)/,
	);
});

test("network changes preserve the current in-scene route query", async ({
	page,
}) => {
	await page.goto("/settings?section=my-releases&demo=settings-my-releases");

	await page.getByRole("link", { name: "GitHub PAT" }).first().click();
	await expect(page).toHaveURL(
		/settings\?(?=.*section=github-pat)(?=.*demo=settings-my-releases)/,
	);

	const inspector = page.locator('[data-demo-inspector-chrome="desktop"]');
	await openInspectorCombobox(inspector, 2);
	await page.getByRole("option", { name: "Slow", exact: true }).click();

	await expect(page).toHaveURL(
		/settings\?(?=.*section=github-pat)(?=.*demo=settings-my-releases)(?=.*d_net=slow)/,
	);
	await expect(inspector).toContainText(
		/settings\?(?=.*section=github-pat)(?=.*demo=settings-my-releases)(?=.*d_net=slow)/,
	);
});

test("desktop inspector does not block settings simulated writes", async ({
	page,
}) => {
	await page.goto("/settings?section=api-keys&demo=settings-my-releases");
	await expect(page.locator("[data-settings-layout]")).toBeVisible();
	await expect(
		page.locator('[data-settings-section="api-keys"]'),
	).toBeVisible();

	const items = page.locator("[data-api-key-item]");
	await expect(items.first()).toBeVisible();
	const before = await items.count();
	await captureDemoInspectorEvidence(
		page.locator("body"),
		"settings-api-keys-floating-overlay.png",
	);

	await page.getByRole("button", { name: "创建 API Key" }).click();

	await expect(page.locator("[data-api-key-created]")).toBeVisible();
	await expect(items).toHaveCount(before + 1);

	await page.getByRole("link", { name: "GitHub PAT" }).first().click();
	await expect(page).toHaveURL(/section=github-pat/);

	await page.getByRole("link", { name: "API Key" }).first().click();
	await expect(page).toHaveURL(/section=api-keys/);
	await expect(items).toHaveCount(before + 1);
});

test("desktop inspector floats over dashboard content instead of reserving layout width", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1366, height: 768 });
	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);
	await expect(
		page.locator('[data-dashboard-brand-heading="true"]'),
	).toBeVisible();
	await expect(
		page.locator('[data-demo-inspector-chrome="desktop"]'),
	).toBeVisible();
	await expect(
		page.locator(
			'[data-dashboard-scope-summary="repo"][data-dashboard-scope-summary-layout="desktop"]',
		),
	).toBeVisible();

	const geometry = await page.evaluate(() => {
		const inspector = document.querySelector<HTMLElement>(
			'[data-demo-inspector-chrome="desktop"]',
		);
		const scopeSummary = document.querySelector<HTMLElement>(
			'[data-dashboard-scope-summary="repo"][data-dashboard-scope-summary-layout="desktop"]',
		);
		if (!inspector || !scopeSummary) {
			throw new Error("dashboard demo inspector geometry is unavailable");
		}
		const inspectorRect = inspector.getBoundingClientRect();
		const summaryRect = scopeSummary.getBoundingClientRect();
		return {
			inspectorPosition: window.getComputedStyle(inspector).position,
			panelLeft: inspectorRect.left,
			summaryRight: summaryRect.right,
		};
	});

	expect(geometry.inspectorPosition).toBe("fixed");
	expect(geometry.summaryRight).toBeGreaterThan(geometry.panelLeft + 24);
	await captureDemoInspectorEvidence(
		page.locator("body"),
		"dashboard-desktop-floating-overlay.png",
	);
});

test("ultra-wide desktop docks inspector into a dedicated left column", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1800, height: 1200 });
	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);
	await expect(
		page.getByRole("heading", { name: "octo-demo/release-lab" }),
	).toBeVisible();
	await expect(page.locator('[data-demo-root-frame="wide"]')).toBeVisible();
	await expect(
		page.locator('[data-demo-inspector-chrome="desktop"]'),
	).toBeVisible();
	await expect(
		page.locator(
			'[data-dashboard-scope-summary="repo"][data-dashboard-scope-summary-layout="desktop"]',
		),
	).toBeVisible();

	const geometry = await page.evaluate(() => {
		const frame = document.querySelector<HTMLElement>(
			'[data-demo-root-frame="wide"]',
		);
		const inspector = document.querySelector<HTMLElement>(
			'[data-demo-inspector-chrome="desktop"]',
		);
		const scopeSummary = document.querySelector<HTMLElement>(
			'[data-dashboard-scope-summary="repo"][data-dashboard-scope-summary-layout="desktop"]',
		);
		if (!frame || !inspector || !scopeSummary) {
			throw new Error("wide demo layout geometry is unavailable");
		}
		const frameRect = frame.getBoundingClientRect();
		const inspectorRect = inspector.getBoundingClientRect();
		const summaryRect = scopeSummary.getBoundingClientRect();
		return {
			frameLeft: frameRect.left,
			frameRight: frameRect.right,
			inspectorLeft: inspectorRect.left,
			inspectorRight: inspectorRect.right,
			inspectorMode: inspector.dataset.demoInspectorMode,
			inspectorPosition: window.getComputedStyle(inspector).position,
			summaryLeft: summaryRect.left,
			viewportWidth: window.innerWidth,
		};
	});

	expect(geometry.inspectorMode).toBe("docked");
	expect(geometry.inspectorPosition).toBe("fixed");
	expect(
		Math.abs(geometry.inspectorLeft - geometry.frameLeft),
	).toBeLessThanOrEqual(2);
	expect(geometry.frameRight).toBeLessThanOrEqual(geometry.viewportWidth + 2);
	expect(geometry.summaryLeft).toBeGreaterThan(geometry.inspectorRight + 16);
	await captureDemoInspectorEvidence(
		page.locator("body"),
		"dashboard-desktop-wide-docked-left.png",
	);
});

test("demo PAT saves never echo a user-entered secret prefix", async ({
	page,
}) => {
	await page.goto("/settings?section=github-pat&demo=settings-my-releases");

	const patInput = page.getByRole("textbox", { name: "GitHub PAT" });
	await patInput.fill("ghp_sensitive_demo_secret_12345678");
	await expect(
		page.getByText("GitHub PAT 可用", { exact: true }),
	).toBeVisible();

	await page.getByRole("button", { name: "保存 GitHub PAT" }).click();

	await expect(
		page.getByText("demo_pat_token_xxxxxxxx", { exact: true }),
	).toBeVisible();
	await expect(
		page.getByText("ghp_sensitive_demo_secret_12345678", { exact: true }),
	).toHaveCount(0);
	await expect(page.getByText("ghp_sens", { exact: true })).toHaveCount(0);
});

test("internal links preserve demo share state in native hrefs", async ({
	page,
}) => {
	await page.goto("/settings?demo=settings-my-releases");

	const githubPatLink = page.getByRole("link", { name: "GitHub PAT" }).first();
	await expect(githubPatLink).toHaveAttribute(
		"href",
		/\/settings\?section=github-pat&demo=settings-my-releases/,
	);

	await page.goto(
		"/public/octo-demo/release-lab/releases/tag/v2.31.0?demo=public-release-ready",
	);

	await expect(page.getByRole("link", { name: "OctoRill" })).toHaveAttribute(
		"href",
		/\/\?demo=public-release-ready/,
	);
});

test("simulated publish writes published share state into the URL", async ({
	page,
}) => {
	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);

	await page.getByRole("button", { name: "发布公开页" }).evaluate((element) => {
		if (!(element instanceof HTMLButtonElement)) {
			throw new Error("publish button is missing");
		}
		element.click();
	});

	await expect(page).toHaveURL(/d_pub=published/);

	await page.reload();

	await expect(page.getByRole("button", { name: "取消发布" })).toBeVisible();
	await expect(page.getByRole("button", { name: "发布公开页" })).toHaveCount(0);
});

test("demo inspector stays fully usable when toast feedback appears", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1366, height: 768 });

	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);
	await expect(
		page.locator('[data-dashboard-brand-heading="true"]'),
	).toBeVisible();

	const inspector = page.locator('[data-demo-inspector-chrome="desktop"]');
	await expect(inspector).toBeVisible();
	await expect(inspector.getByText("Actions & Share")).toBeVisible();
	await expect(
		inspector.getByRole("button", { name: "Copy Share URL" }),
	).toBeVisible();
	await expect(inspector.getByText("Advanced")).toBeVisible();
	await expect(inspector).toContainText(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);

	await page.getByRole("button", { name: "发布公开页" }).click();

	const toast = page.locator('[data-slot="toast"][data-state="open"]').first();
	await expect(toast).toBeVisible();
	await expect(inspector.getByText("Actions & Share")).toBeVisible();
	await expect(
		inspector.getByRole("button", { name: "Copy Share URL" }),
	).toBeVisible();
	await expect(inspector.getByText("Advanced")).toBeVisible();
	await expect(
		page.locator('[data-demo-inspector-scroll-cue="bottom"]'),
	).toBeHidden();

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
			panelBottomGap: window.innerHeight - panelRect.bottom,
			surfaceTop: surfaceRect.top,
			surfaceBottom: surfaceRect.bottom,
			scrollerBottom: scrollerRect.bottom,
			scrollerCanScroll: scroller.scrollHeight - scroller.clientHeight > 12,
			titleTop: titleRect.top,
			toastBottom: toastRect?.bottom ?? 0,
			toastLeft: toastRect?.left ?? 0,
			toastRight: toastRect?.right ?? 0,
			panelLeft: panelRect.left,
			panelRight: panelRect.right,
		};
	});

	expect(geometry.panelBottom).toBeLessThanOrEqual(geometry.viewportHeight - 1);
	expect(geometry.panelBottomGap).toBeGreaterThanOrEqual(8);
	expect(geometry.surfaceBottom).toBeLessThanOrEqual(
		geometry.viewportHeight - 1,
	);
	expect(geometry.scrollerBottom).toBeLessThanOrEqual(
		geometry.viewportHeight - 1,
	);
	expect(geometry.titleTop).toBeGreaterThanOrEqual(geometry.panelTop);
	expect(geometry.surfaceTop).toBeGreaterThanOrEqual(geometry.panelTop);
	expect(geometry.scrollerCanScroll).toBe(false);

	const overlapsToastHorizontally =
		geometry.panelRight > geometry.toastLeft &&
		geometry.panelLeft < geometry.toastRight;
	if (overlapsToastHorizontally) {
		expect(geometry.titleTop).toBeGreaterThanOrEqual(geometry.toastBottom + 4);
	}

	await captureDemoInspectorEvidence(
		page.locator("body"),
		"dashboard-desktop-toast-clamped-fixed.png",
	);

	await expect(
		page.locator('[data-demo-inspector-scroll-cue="top"]'),
	).toBeHidden();
});

test("demo inspector grows to fit the full control stack on tall desktops", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1798, height: 1360 });
	await page.goto(
		"/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish",
	);
	await expect(
		page.locator('[data-dashboard-brand-heading="true"]'),
	).toBeVisible();

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
	await openInspectorCombobox(inspector, 3);
	await page.getByRole("option", { name: "Published", exact: true }).click();

	await expect(page).toHaveURL(/d_pub=published/);
	await expect(page.getByRole("button", { name: "发布公开页" })).toHaveCount(0);
	await expect(page.getByRole("button", { name: "取消发布" })).toBeVisible();

	await page.getByRole("button", { name: "Reset Scene" }).click();

	await expect(page).not.toHaveURL(/d_pub=published/);
	await expect(page.getByRole("button", { name: "发布公开页" })).toBeVisible();
	await expect(page.getByRole("button", { name: "取消发布" })).toHaveCount(0);
});
