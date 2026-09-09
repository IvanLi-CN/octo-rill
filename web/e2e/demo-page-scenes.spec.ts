import { expect, test } from "@playwright/test";

test.describe("mock-only page demo scenes", () => {
	test("admin dashboard and repo governance scenes are deep-linkable", async ({
		page,
	}) => {
		await page.goto("/admin/?demo=admin-dashboard-overview&d_controls=hidden");
		await expect(
			page.locator('[data-admin-dashboard-shell="true"]'),
		).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByText("运营总览")).toBeVisible();

		await page.goto("/admin/repos?demo=admin-repos-overview&d_controls=hidden");
		await expect(page.getByText("仓库刷新治理")).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByText("octo-demo/release-lab")).toBeVisible();
		await expect(page.getByText("octo-demo/docs-hub")).toBeVisible();
	});

	test("bind github and announcement detail scenes use deterministic payloads", async ({
		page,
	}) => {
		await page.goto(
			"/bind/github?demo=bind-github-pending&linuxdo=connected&passkey=created&d_controls=hidden",
		);
		await expect(page.getByText("可以继续绑定 GitHub")).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByText("Passkey 已暂存")).toBeVisible();

		await page.goto(
			"/octo-demo/release-lab/discussions/42?demo=announcement-detail&from=feed&d_controls=hidden",
		);
		await expect(page.getByRole("tab", { name: "润色" })).toHaveAttribute(
			"aria-selected",
			"true",
			{ timeout: 15_000 },
		);
		await expect(
			page.getByRole("heading", { name: "公告：站内阅读流已对齐" }),
		).toBeVisible();
	});

	test("not found and app boot scenes remain explicit", async ({ page }) => {
		await page.goto("/demo-missing-route?demo=not-found&d_controls=hidden");
		await expect(page.locator("[data-not-found-surface]")).toBeVisible({
			timeout: 15_000,
		});
		await expect(
			page.getByRole("heading", { name: "页面不存在" }),
		).toBeVisible();

		await page.goto("/?demo=app-boot&d_controls=hidden");
		await expect(page.locator("[data-app-boot]")).toBeVisible({
			timeout: 15_000,
		});
		await expect(
			page.getByText("应用正在完成初始化，请稍候片刻。"),
		).toBeVisible();
	});

	test("app shell version states are addressable without a real service worker", async ({
		page,
	}) => {
		await page.goto(
			"/focus/repo/octo-demo/release-lab?demo=app-shell&d_shell=update-install&d_controls=hidden",
		);
		await expect(page.locator("[data-version-update-notice]")).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByText(/新版本|更新/).first()).toBeVisible();
		await expect(page.locator("[data-pwa-install-action]")).toBeVisible();

		await page.goto(
			"/focus/repo/octo-demo/release-lab?demo=app-shell&d_shell=unknown&d_controls=hidden",
		);
		await expect(page.getByText("Version unknown")).toBeVisible({
			timeout: 15_000,
		});
	});
});
