import { type Page, type Route, expect, test } from "@playwright/test";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

async function installAdminJobsMocks(page: Page) {
	const tasks = [
		{
			id: "task-failed-1",
			task_type: "brief.daily_slot",
			status: "failed",
			source: "scheduler",
			requested_by: null,
			parent_task_id: null,
			cancel_requested: false,
			error_message: "mock failed",
			created_at: "2026-02-26T00:00:00Z",
			started_at: "2026-02-26T00:00:10Z",
			finished_at: "2026-02-26T00:01:00Z",
			updated_at: "2026-02-26T00:01:00Z",
		},
		{
			id: "task-running-1",
			task_type: "sync.releases",
			status: "running",
			source: "api.sync_releases",
			requested_by: 1,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-02-26T01:00:00Z",
			started_at: "2026-02-26T01:00:05Z",
			finished_at: null,
			updated_at: "2026-02-26T01:00:20Z",
		},
	];

	const slots = Array.from({ length: 24 }, (_, hour) => ({
		hour_utc: hour,
		enabled: hour % 2 === 0,
		last_dispatch_at: "2026-02-26T00:00:00Z",
		updated_at: "2026-02-26T00:00:00Z",
	}));

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(route, {
				user: {
					id: 1,
					github_user_id: 10,
					login: "octo-admin",
					name: "Octo Admin",
					avatar_url: null,
					email: "admin@example.com",
					is_admin: true,
				},
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/overview") {
			return json(route, {
				queued: 2,
				running: 1,
				failed_24h: 1,
				succeeded_24h: 3,
				enabled_scheduled_slots: slots.filter((slot) => slot.enabled).length,
				total_scheduled_slots: slots.length,
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/realtime") {
			return json(route, {
				items: tasks,
				page: 1,
				page_size: 20,
				total: tasks.length,
			});
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/jobs/realtime/")
		) {
			const taskId = pathname.split("/").at(-1) ?? "";
			const task = tasks.find((item) => item.id === taskId);
			if (!task) {
				return json(
					route,
					{
						ok: false,
						error: { code: "not_found", message: "task not found" },
					},
					404,
				);
			}
			return json(route, {
				task,
				events: [
					{
						id: 1,
						event_type: "task.created",
						payload_json: JSON.stringify({
							task_id: task.id,
							status: task.status,
						}),
						created_at: "2026-02-26T00:00:00Z",
					},
				],
			});
		}

		if (
			req.method() === "POST" &&
			pathname.startsWith("/api/admin/jobs/realtime/") &&
			pathname.endsWith("/retry")
		) {
			return json(route, { task_id: "retry-1", status: "queued" });
		}

		if (
			req.method() === "POST" &&
			pathname.startsWith("/api/admin/jobs/realtime/") &&
			pathname.endsWith("/cancel")
		) {
			return json(route, {
				task_id: pathname.split("/").at(-2),
				status: "running",
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/scheduled") {
			return json(route, { items: slots });
		}

		if (
			req.method() === "PATCH" &&
			pathname.startsWith("/api/admin/jobs/scheduled/")
		) {
			const hour = Number(pathname.split("/").at(-1));
			const body = req.postDataJSON() as { enabled: boolean };
			const target = slots.find((slot) => slot.hour_utc === hour);
			if (!target) {
				return json(
					route,
					{
						ok: false,
						error: { code: "not_found", message: "slot not found" },
					},
					404,
				);
			}
			target.enabled = body.enabled;
			return json(route, target);
		}

		return json(
			route,
			{ error: { message: `unhandled ${req.method()} ${pathname}` } },
			404,
		);
	});
}

test("admin can manage jobs center", async ({ page }) => {
	await installAdminJobsMocks(page);
	await page.goto("/admin/jobs");

	await expect(page).toHaveURL(/\/admin\/jobs$/);
	await expect(page.getByRole("heading", { name: "任务中心" })).toBeVisible();
	await expect(page.getByRole("heading", { name: "任务总览" })).toBeVisible();

	await expect(page.getByText("brief.daily_slot")).toBeVisible();
	await page.getByRole("button", { name: "详情" }).first().click();
	await expect(page.getByRole("heading", { name: "任务详情" })).toBeVisible();
	await page.getByRole("button", { name: "关闭" }).click();

	await page.getByRole("button", { name: "定时任务" }).click();
	await expect(page.getByText("UTC 00:00")).toBeVisible();
	await page.getByRole("button", { name: "停用" }).first().click();
	await expect(
		page.getByRole("button", { name: "启用" }).first(),
	).toBeVisible();
});
