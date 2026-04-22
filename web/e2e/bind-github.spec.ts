import { expect, test, type Route } from "@playwright/test";

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function svgAvatarDataUrl(
	label: string,
	background: string,
	foreground = "#ffffff",
) {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240"><rect width="240" height="240" rx="120" fill="${background}"/><text x="120" y="132" font-family="Inter,Arial,sans-serif" font-size="44" font-weight="700" text-anchor="middle" fill="${foreground}">${label}</text></svg>`,
	)}`;
}

async function installBindMocks(
	page: Parameters<typeof test>[0]["page"],
	options?: {
		linuxdoAvailable?: boolean;
		pendingLinuxDo?: {
			linuxdo_user_id: number;
			username: string;
			name: string | null;
			avatar_url: string;
			trust_level: number;
			active: boolean;
			silenced: boolean;
		} | null;
		pendingPasskey?: {
			label: string;
			created_at: string;
		} | null;
	},
) {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());

		if (req.method() === "GET" && url.pathname === "/api/me") {
			return json(
				route,
				{
					error: {
						code: "unauthorized",
						message: "unauthorized",
					},
				},
				401,
			);
		}

		if (req.method() === "GET" && url.pathname === "/api/auth/bind-context") {
			return json(route, {
				linuxdo_available: options?.linuxdoAvailable ?? true,
				pending_linuxdo: options?.pendingLinuxDo ?? {
					linuxdo_user_id: 9527,
					username: "linuxdo-first-login",
					name: "LinuxDO First Login",
					avatar_url: svgAvatarDataUrl("LD", "#0ea5e9"),
					trust_level: 2,
					active: true,
					silenced: false,
				},
				pending_passkey: options?.pendingPasskey ?? null,
			});
		}

		if (req.method() === "GET" && url.pathname === "/api/health") {
			return json(route, { ok: true, version: "1.2.3" });
		}

		return json(
			route,
			{
				error: {
					code: "not_found",
					message: `unhandled ${req.method()} ${url.pathname}`,
				},
			},
			404,
		);
	});
}

test("bind github page shows pending linuxdo snapshot and github CTA", async ({
	page,
}) => {
	await installBindMocks(page);

	await page.goto("/bind/github?linuxdo=connected");

	await expect(page.getByText("可以继续绑定 GitHub")).toBeVisible();
	await expect(page.getByText("LinuxDO First Login")).toBeVisible();
	const githubLink = page.getByRole("link", { name: "绑定 GitHub 并继续" });
	await expect(githubLink).toHaveAttribute("href", "/auth/github/login");
	await expect(
		githubLink.locator('[data-auth-provider-icon="github"]'),
	).toBeVisible();
});

test("bind github page shows pending passkey state and github CTA", async ({
	page,
}) => {
	await installBindMocks(page, {
		pendingLinuxDo: null,
		pendingPasskey: {
			label: "Passkey · 2026-04-22 10:12 UTC",
			created_at: "2026-04-22T10:12:00Z",
		},
	});

	await page.goto("/bind/github?passkey=created");

	await expect(page.getByText("Passkey 已暂存")).toBeVisible();
	await expect(page.getByText("待挂接的 Passkey")).toBeVisible();
	await expect(page.getByText("Passkey · 2026-04-22 10:12 UTC")).toBeVisible();
	await expect(
		page.getByRole("link", { name: "绑定 GitHub 并继续" }),
	).toHaveAttribute("href", "/auth/github/login");
});
