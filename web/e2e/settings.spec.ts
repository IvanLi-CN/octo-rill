import { type Route, expect, test } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";
import { installPasskeyBrowserMock } from "./passkeyHelpers";

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

async function getPartialAccessibilityTreeSnapshot(
	page: Parameters<typeof test>[0]["page"],
	selector: string,
) {
	const client = await page.context().newCDPSession(page);
	const { root } = await client.send("DOM.getDocument", {
		depth: -1,
		pierce: true,
	});
	const { nodeId } = await client.send("DOM.querySelector", {
		nodeId: root.nodeId,
		selector,
	});
	if (!nodeId) {
		throw new Error(`selector not found for AX snapshot: ${selector}`);
	}
	const { node } = await client.send("DOM.describeNode", { nodeId });
	const { nodes } = await client.send("Accessibility.getPartialAXTree", {
		backendNodeId: node.backendNodeId,
		fetchRelatives: true,
	});
	return JSON.stringify(nodes);
}

const defaultGitHubConnections = [
	{
		id: "ghconn_primary",
		github_user_id: 42,
		login: "storybook-user",
		name: "Storybook User",
		avatar_url: svgAvatarDataUrl("GH", "#111827"),
		email: "storybook-user@example.com",
		scopes: "read:user, user:email, notifications, public_repo",
		linked_at: "2026-04-16T10:00:00+08:00",
		updated_at: "2026-04-18T09:00:00+08:00",
	},
	{
		id: "ghconn_secondary",
		github_user_id: 84,
		login: "storybook-ops",
		name: "Storybook Ops",
		avatar_url: svgAvatarDataUrl("OP", "#0f766e"),
		email: "ops@example.com",
		scopes: "read:user, user:email, notifications, public_repo",
		linked_at: "2026-04-17T10:00:00+08:00",
		updated_at: "2026-04-18T09:05:00+08:00",
	},
];

function buildReactionReadyFeedItem(id: string) {
	return {
		kind: "release",
		ts: "2026-04-18T08:00:00+08:00",
		id,
		repo_full_name: "owner/repo",
		title: `Release ${id}`,
		body: "- settings page migration\n- linuxdo oauth snapshot\n- pat fallback guide",
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `https://github.com/owner/repo/releases/tag/v${id}`,
		unread: null,
		translated: null,
		smart: null,
		reactions: {
			counts: {
				plus1: 0,
				laugh: 0,
				heart: 0,
				hooray: 0,
				rocket: 0,
				eyes: 0,
			},
			viewer: {
				plus1: false,
				laugh: false,
				heart: false,
				hooray: false,
				rocket: false,
				eyes: false,
			},
			status: "ready",
		},
	};
}

async function installSettingsMocks(
	page: Parameters<typeof test>[0]["page"],
	options?: {
		linuxdoAvailable?: boolean;
		linuxdoConnection?: Record<string, unknown> | null;
		githubConnections?: typeof defaultGitHubConnections;
		passkeys?: Array<{
			id: string;
			label: string;
			created_at: string;
			last_used_at: string | null;
		}>;
		reactionTokenConfigured?: boolean;
		reactionTokenMasked?: string | null;
		reactionTokenState?: "idle" | "valid" | "invalid" | "error";
		reactionTokenMessage?: string | null;
		reactionTokenOwnerLogin?: string | null;
		includeOwnReleases?: boolean;
		withReactionFeed?: boolean;
	},
) {
	let includeOwnReleases = options?.includeOwnReleases ?? false;
	let githubConnections =
		options?.githubConnections ?? defaultGitHubConnections;
	let passkeys = options?.passkeys ?? [];
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: "storybook-user",
					github_user_id: 42,
					login: "storybook-user",
					name: "Storybook User",
					avatar_url: svgAvatarDataUrl("SU", "#4f6a98"),
					email: "storybook-user@example.com",
					is_admin: true,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			return json(route, {
				items: options?.withReactionFeed
					? [buildReactionReadyFeedItem("70001")]
					: [buildReactionReadyFeedItem("70001")],
				next_cursor: null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/me/github-connections") {
			return json(route, { items: githubConnections });
		}

		if (req.method() === "GET" && pathname === "/api/me/passkeys") {
			return json(route, { items: passkeys });
		}

		if (
			req.method() === "DELETE" &&
			pathname.startsWith("/api/me/github-connections/")
		) {
			const connectionId = pathname.split("/").at(-1);
			githubConnections = githubConnections.filter(
				(connection) => connection.id !== connectionId,
			);
			return json(route, { items: githubConnections });
		}

		if (req.method() === "DELETE" && pathname.startsWith("/api/me/passkeys/")) {
			const passkeyId = pathname.split("/").at(-1);
			passkeys = passkeys.filter((passkey) => passkey.id !== passkeyId);
			return json(route, { items: passkeys });
		}

		if (req.method() === "GET" && pathname === "/api/reaction-token/status") {
			const owner = options?.reactionTokenOwnerLogin
				? githubConnections.find(
						(connection) =>
							connection.login === options.reactionTokenOwnerLogin,
					)
				: githubConnections[0];
			return json(route, {
				configured: options?.reactionTokenConfigured ?? false,
				masked_token: options?.reactionTokenMasked ?? null,
				check: {
					state: options?.reactionTokenState ?? "idle",
					message: options?.reactionTokenMessage ?? null,
					checked_at: "2026-04-18T08:00:00+08:00",
				},
				owner: owner
					? {
							github_connection_id: owner.id,
							github_user_id: owner.github_user_id,
							login: owner.login,
						}
					: null,
			});
		}

		if (req.method() === "POST" && pathname === "/api/reaction-token/check") {
			const owner = githubConnections[0];
			return json(route, {
				state: "valid",
				message: `token is valid for @${owner.login}`,
				owner: {
					github_connection_id: owner.id,
					github_user_id: owner.github_user_id,
					login: owner.login,
				},
			});
		}

		if (req.method() === "PUT" && pathname === "/api/reaction-token") {
			const owner = githubConnections[0];
			return json(route, {
				configured: true,
				masked_token: options?.reactionTokenMasked ?? "ghp_****_saved",
				check: {
					state: "valid",
					message: `token is valid for @${owner.login}`,
					checked_at: "2026-04-18T08:00:00+08:00",
				},
				owner: {
					github_connection_id: owner.id,
					github_user_id: owner.github_user_id,
					login: owner.login,
				},
			});
		}

		if (req.method() === "GET" && pathname === "/api/me/linuxdo") {
			return json(route, {
				available: options?.linuxdoAvailable ?? true,
				connection: options?.linuxdoConnection ?? null,
			});
		}

		if (req.method() === "GET" && pathname === "/api/me/profile") {
			return json(route, {
				user_id: "storybook-user",
				daily_brief_local_time: "08:00",
				daily_brief_time_zone: "Asia/Shanghai",
				last_active_at: "2026-04-18T08:00:00+08:00",
				include_own_releases: includeOwnReleases,
			});
		}

		if (req.method() === "PATCH" && pathname === "/api/me/profile") {
			const payload = req.postDataJSON() as
				| { include_own_releases?: boolean }
				| undefined;
			includeOwnReleases = payload?.include_own_releases ?? includeOwnReleases;
			return json(route, {
				user_id: "storybook-user",
				daily_brief_local_time: "08:00",
				daily_brief_time_zone: "Asia/Shanghai",
				last_active_at: "2026-04-18T08:00:00+08:00",
				include_own_releases: includeOwnReleases,
			});
		}

		if (req.method() === "GET" && pathname === "/api/version") {
			return json(route, { ok: true, version: "1.2.3", source: "test" });
		}

		if (req.method() === "GET" && pathname === "/api/health") {
			return json(route, { ok: true, version: "1.2.3" });
		}

		return json(
			route,
			{
				error: {
					code: "not_found",
					message: `unhandled ${req.method()} ${pathname}`,
				},
			},
			404,
		);
	});
}

test("dashboard account menu exposes settings entry and opens settings page", async ({
	page,
}) => {
	await installPasskeyBrowserMock(page);
	await installSettingsMocks(page);

	await page.goto("/");
	await page.getByRole("button", { name: "查看账号信息" }).click();
	await expect(page.locator("[data-dashboard-settings-entry]")).toBeVisible();
	await page.getByRole("link", { name: "设置" }).click();

	await expect(page).toHaveURL(/\/settings$/);
	await expect(page.locator('[data-settings-section="linuxdo"]')).toContainText(
		"LinuxDO 绑定",
	);
});

test("settings deep link focuses github accounts section", async ({ page }) => {
	await installPasskeyBrowserMock(page);
	await installSettingsMocks(page);

	await page.goto("/settings?section=github-accounts&github=connected");

	await expect(page).toHaveURL(/section=github-accounts/);
	const section = page.locator('[data-settings-section="github-accounts"]');
	await expect(section).toContainText("GitHub 账号");
	await expect(section).toContainText("@storybook-user");
	await expect(section).toContainText("@storybook-ops");
	await expect(page.getByAltText("storybook-user avatar")).toBeVisible();
	await expect(page.getByAltText("storybook-ops avatar")).toBeVisible();
	await expect(page.getByText("GitHub 账号已绑定")).toBeVisible();
});

test("settings deep link focuses github pat section", async ({ page }) => {
	await installPasskeyBrowserMock(page);
	await installSettingsMocks(page, {
		reactionTokenConfigured: true,
		reactionTokenMasked: "ghp_****_saved",
		reactionTokenState: "valid",
		reactionTokenMessage: "token is valid for @storybook-ops",
		reactionTokenOwnerLogin: "storybook-ops",
	});

	await page.goto("/settings?section=github-pat");

	await expect(page).toHaveURL(/section=github-pat/);
	const githubPatSection = page.locator('[data-settings-section="github-pat"]');
	await expect(githubPatSection).toBeVisible({ timeout: 10_000 });
	await expect(githubPatSection).toContainText("ghp_****_saved", {
		timeout: 10_000,
	});
	const input = page.locator("#settings-reaction-pat");
	await expect(input).toHaveAttribute("type", "password");
	await expect(input).toHaveAttribute("autocomplete", "new-password");
	await expect(input).toHaveAttribute("data-1p-ignore", "true");
	await expect(input).toHaveAttribute("data-form-type", "other");
	await expect(input).toHaveAttribute("data-secret-visible", "false");
	await expect(input).toHaveAttribute(
		"data-secret-mask-mode",
		"native-password",
	);
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(input).toHaveAttribute("type", "text");
	await expect(input).toHaveAttribute("data-secret-visible", "true");
	await expect(input).toHaveAttribute("data-secret-mask-mode", "plain-text");
	const guide = page.getByTestId("github-pat-guide-card");
	await expect(guide).toBeVisible();
	await expect(guide.getByRole("textbox", { name: "Note" })).toHaveValue(
		"OctoRill release feedback",
	);
	await expect(
		guide.getByRole("button", { name: "No expiration" }),
	).toBeVisible();
	await expect(page.getByText("@storybook-ops", { exact: true })).toBeVisible();
});

test("settings github pat hidden mode preserves undo history after visible edits", async ({
	page,
}) => {
	await installSettingsMocks(page);

	await page.goto("/settings?section=github-pat");

	const input = page.locator("#settings-reaction-pat");
	const toggleButton = page.locator(
		'button[aria-controls="settings-reaction-pat"]',
	);
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await input.fill("ghp_visible_fix");
	await toggleButton.click();
	await expect(toggleButton).toBeFocused();
	await expect(toggleButton).toHaveAttribute("aria-label", "显示 GitHub PAT");
	await input.focus();
	await page.keyboard.press(
		process.platform === "darwin" ? "Meta+Z" : "Control+Z",
	);
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(input).toHaveValue("");
});

test("settings github pat hidden mode stays editable in accessibility tree", async ({
	page,
}) => {
	await installSettingsMocks(page);

	await page.goto("/settings?section=github-pat");

	const input = page.locator("#settings-reaction-pat");
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await input.fill("ghp_secret_demo");
	await page.getByRole("button", { name: "隐藏 GitHub PAT" }).click();

	const inputAccessibilityTree = await getPartialAccessibilityTreeSnapshot(
		page,
		"#settings-reaction-pat",
	);

	expect(inputAccessibilityTree).toContain("GitHub PAT");
	expect(inputAccessibilityTree).toContain("当前内容已隐藏");
	expect(inputAccessibilityTree).not.toContain("ghp_secret_demo");
	expect(inputAccessibilityTree).toContain("•••••••••••••••");
	expect(inputAccessibilityTree).not.toContain("ariaHiddenElement");
});

test("settings github pat hidden mode keeps word deletion shortcuts", async ({
	page,
}) => {
	await installSettingsMocks(page);

	await page.goto("/settings?section=github-pat");

	const input = page.locator("#settings-reaction-pat");
	const toggleButton = page.locator(
		'button[aria-controls="settings-reaction-pat"]',
	);
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await input.fill("ghp_visible_chunk");
	await page.getByRole("button", { name: "隐藏 GitHub PAT" }).click();
	await expect(toggleButton).toBeFocused();
	await input.focus();
	await input.evaluate((node) => {
		if (!(node instanceof HTMLInputElement)) {
			throw new Error("expected HTMLInputElement");
		}
		node.setSelectionRange(node.value.length, node.value.length);
	});
	await page.keyboard.press(
		process.platform === "darwin" ? "Alt+Backspace" : "Control+Backspace",
	);
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(input).toHaveValue("ghp_visible_");
});

test("settings github pat hidden mode keeps word deletion on the native undo stack", async ({
	page,
}) => {
	await installSettingsMocks(page);

	await page.goto("/settings?section=github-pat");

	const input = page.locator("#settings-reaction-pat");
	const toggleButton = page.locator(
		'button[aria-controls="settings-reaction-pat"]',
	);
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await input.fill("ghp_visible_chunk");
	await page.getByRole("button", { name: "隐藏 GitHub PAT" }).click();
	await expect(toggleButton).toBeFocused();
	await input.focus();
	await input.evaluate((node) => {
		if (!(node instanceof HTMLInputElement)) {
			throw new Error("expected HTMLInputElement");
		}
		node.setSelectionRange(node.value.length, node.value.length);
	});
	await page.keyboard.press(
		process.platform === "darwin" ? "Alt+Backspace" : "Control+Backspace",
	);
	await page.keyboard.press(
		process.platform === "darwin" ? "Meta+Z" : "Control+Z",
	);
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(input).toHaveValue("ghp_visible_chunk");
});

test("settings github pat hidden mode handles beforeinput word deletion", async ({
	page,
}) => {
	await installSettingsMocks(page);

	await page.goto("/settings?section=github-pat");

	const input = page.locator("#settings-reaction-pat");
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await input.fill("ghp_visible_chunk");
	await page.getByRole("button", { name: "隐藏 GitHub PAT" }).click();
	await input.focus();
	await input.evaluate((node) => {
		if (!(node instanceof HTMLInputElement)) {
			throw new Error("expected HTMLInputElement");
		}
		node.setSelectionRange(node.value.length, node.value.length);
		node.dispatchEvent(
			new InputEvent("beforeinput", {
				bubbles: true,
				cancelable: true,
				inputType: "deleteWordBackward",
			}),
		);
	});
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(input).toHaveValue("ghp_visible_");
});

test("settings github pat hidden mode accepts drop edits", async ({ page }) => {
	await installSettingsMocks(page);

	await page.goto("/settings?section=github-pat");

	const input = page.locator("#settings-reaction-pat");
	await input.evaluate((node, droppedValue) => {
		if (!(node instanceof HTMLInputElement)) {
			throw new Error("expected HTMLInputElement");
		}
		const dataTransfer = new DataTransfer();
		dataTransfer.setData("text", droppedValue);
		node.dispatchEvent(
			new DragEvent("drop", {
				bubbles: true,
				cancelable: true,
				dataTransfer,
			}),
		);
	}, "ghp_drop_token");
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(input).toHaveValue("ghp_drop_token");
});

test("settings github pat hidden mode keeps drop edits on the native undo stack", async ({
	page,
}) => {
	await installSettingsMocks(page);

	await page.goto("/settings?section=github-pat");

	const input = page.locator("#settings-reaction-pat");
	await input.evaluate((node, droppedValue) => {
		if (!(node instanceof HTMLInputElement)) {
			throw new Error("expected HTMLInputElement");
		}
		const dataTransfer = new DataTransfer();
		dataTransfer.setData("text/plain", droppedValue);
		node.dispatchEvent(
			new DragEvent("drop", {
				bubbles: true,
				cancelable: true,
				dataTransfer,
			}),
		);
	}, "ghp_drop_token");
	await page.keyboard.press(
		process.platform === "darwin" ? "Meta+Z" : "Control+Z",
	);
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(input).toHaveValue("");
});

test("settings github pat hidden mode supports menu-style undo and redo", async ({
	page,
}) => {
	await installSettingsMocks(page);

	await page.goto("/settings?section=github-pat");

	const input = page.locator("#settings-reaction-pat");
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await input.fill("ghp_visible_chunk");
	await page.getByRole("button", { name: "隐藏 GitHub PAT" }).click();
	await input.focus();
	await input.evaluate((node) => {
		if (!(node instanceof HTMLInputElement)) {
			throw new Error("expected HTMLInputElement");
		}
		node.setSelectionRange(node.value.length, node.value.length);
	});
	await page.keyboard.press(
		process.platform === "darwin" ? "Alt+Backspace" : "Control+Backspace",
	);
	await input.evaluate((node) => {
		if (!(node instanceof HTMLInputElement)) {
			throw new Error("expected HTMLInputElement");
		}
		node.dispatchEvent(
			new InputEvent("beforeinput", {
				bubbles: true,
				cancelable: true,
				inputType: "historyUndo",
			}),
		);
	});
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(input).toHaveValue("ghp_visible_chunk");
	await page.getByRole("button", { name: "隐藏 GitHub PAT" }).click();
	await input.focus();
	await input.evaluate((node) => {
		if (!(node instanceof HTMLInputElement)) {
			throw new Error("expected HTMLInputElement");
		}
		node.dispatchEvent(
			new InputEvent("beforeinput", {
				bubbles: true,
				cancelable: true,
				inputType: "historyRedo",
			}),
		);
	});
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(input).toHaveValue("ghp_visible_");
});

test("settings github pat hidden mode drops at the hovered caret", async ({
	page,
}) => {
	await installSettingsMocks(page);

	await page.goto("/settings?section=github-pat");

	const input = page.locator("#settings-reaction-pat");
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await input.fill("ghp_chunk");
	await page.getByRole("button", { name: "隐藏 GitHub PAT" }).click();
	await input.focus();
	await input.evaluate((node) => {
		if (!(node instanceof HTMLInputElement)) {
			throw new Error("expected HTMLInputElement");
		}
		node.setSelectionRange(node.value.length, node.value.length);
	});
	await input.evaluate(
		(node, payload) => {
			if (!(node instanceof HTMLInputElement)) {
				throw new Error("expected HTMLInputElement");
			}
			const { prefix, droppedValue } = payload;
			const rect = node.getBoundingClientRect();
			const style = getComputedStyle(node);
			const leftInset =
				parseFloat(style.borderLeftWidth || "0") +
				parseFloat(style.paddingLeft || "0");
			const context = document.createElement("canvas").getContext("2d");
			if (!context) {
				throw new Error("expected 2d canvas context");
			}
			context.font =
				style.font ||
				`${style.fontStyle} ${style.fontVariant} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
			const clientX =
				rect.left + leftInset + context.measureText(prefix).width + 1;
			const clientY = rect.top + rect.height / 2;
			node.dispatchEvent(
				new DragEvent("dragover", {
					bubbles: true,
					cancelable: true,
					clientX,
					clientY,
					dataTransfer: new DataTransfer(),
				}),
			);
			const dataTransfer = new DataTransfer();
			dataTransfer.setData("text/plain", droppedValue);
			node.dispatchEvent(
				new DragEvent("drop", {
					bubbles: true,
					cancelable: true,
					clientX,
					clientY,
					dataTransfer,
				}),
			);
		},
		{
			prefix: "ghp_",
			droppedValue: "drop_",
		},
	);
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(input).toHaveValue("ghp_drop_chunk");
});

test("settings github pat save clears hidden undo history", async ({
	page,
}) => {
	await installSettingsMocks(page);

	await page.goto("/settings?section=github-pat");

	const input = page.locator("#settings-reaction-pat");
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await input.fill("ghp_valid_token_1234");
	await expect(
		page.getByText("GitHub PAT 可用", { exact: true }),
	).toBeVisible();
	await page.getByRole("button", { name: "保存 GitHub PAT" }).click();
	await expect(input).toHaveValue("");
	await expect(
		page.getByText("token is valid for @storybook-user", { exact: true }),
	).toBeVisible();
	await input.focus();
	await page.keyboard.press(
		process.platform === "darwin" ? "Meta+Z" : "Control+Z",
	);
	await page.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(input).toHaveValue("");
});

test("settings shows bound linuxdo snapshot", async ({ page }) => {
	await installPasskeyBrowserMock(page);
	await installSettingsMocks(page, {
		linuxdoAvailable: true,
		linuxdoConnection: {
			linuxdo_user_id: 2048,
			username: "linuxdo-bound",
			name: "LinuxDO Bound",
			avatar_url: svgAvatarDataUrl("LD", "#0ea5e9"),
			trust_level: 4,
			active: true,
			silenced: false,
			linked_at: "2026-04-16T10:00:00+08:00",
			updated_at: "2026-04-18T09:30:00+08:00",
		},
	});

	await page.goto("/settings");

	const linuxdoSection = page.locator('[data-settings-section="linuxdo"]');
	await expect(linuxdoSection.getByText("@linuxdo-bound")).toBeVisible();
	await expect(linuxdoSection).toContainText(/Trust level\s*4/);
	await expect(
		linuxdoSection.getByRole("button", { name: "解绑 LinuxDO" }),
	).toBeVisible();
});

test("settings deep link saves my releases opt-in", async ({ page }) => {
	await installPasskeyBrowserMock(page);
	await installSettingsMocks(page, {
		includeOwnReleases: false,
	});

	await page.goto("/settings?section=my-releases");

	await expect(page).toHaveURL(/section=my-releases/);
	const myReleasesSection = page.locator(
		'[data-settings-section="my-releases"]',
	);
	await expect(myReleasesSection).toContainText("仅显示已加星仓库");

	const switchControl = myReleasesSection.getByRole("switch", {
		name: "我的发布",
	});
	await expect(switchControl).toHaveAttribute("aria-checked", "false");
	await switchControl.click();
	await myReleasesSection
		.getByRole("button", { name: "保存“我的发布”" })
		.click();

	await expect(switchControl).toHaveAttribute("aria-checked", "true");
	await expect(myReleasesSection).toContainText("已纳入我的发布");
});

test("unknown app route shows not-found page after app shell boot", async ({
	page,
}) => {
	await installPasskeyBrowserMock(page);
	await installSettingsMocks(page);

	await page.goto("/does-not-exist");

	await expect(page.getByRole("heading", { name: "页面不存在" })).toBeVisible();
	await expect(page.locator("[data-not-found-surface]")).toContainText(
		"/does-not-exist",
	);
	await expect(page.getByRole("link", { name: "返回工作台" })).toBeVisible();
	await expect(page.getByRole("link", { name: "打开设置" })).toBeVisible();
});

test("settings deep link shows passkey list and allows revoke", async ({
	page,
}) => {
	await installPasskeyBrowserMock(page);
	await installSettingsMocks(page, {
		passkeys: [
			{
				id: "pk_phone",
				label: "Passkey · 2026-04-20 09:00 UTC",
				created_at: "2026-04-20T09:00:00Z",
				last_used_at: "2026-04-22T08:30:00Z",
			},
			{
				id: "pk_laptop",
				label: "Passkey · 2026-04-21 11:15 UTC",
				created_at: "2026-04-21T11:15:00Z",
				last_used_at: null,
			},
		],
	});

	await page.goto("/settings?section=passkeys&passkey=registered");

	await expect(page).toHaveURL(/section=passkeys/);
	const passkeysSection = page.locator('[data-settings-section="passkeys"]');
	await expect(page.getByText("Passkey 已添加")).toBeVisible();
	await expect(passkeysSection).toContainText("Passkey · 2026-04-20 09:00 UTC");
	await expect(passkeysSection).toContainText("Passkey · 2026-04-21 11:15 UTC");
	await passkeysSection
		.locator('[data-passkey-item="pk_phone"]')
		.getByRole("button", { name: "移除" })
		.click();
	await expect(
		passkeysSection.locator('[data-passkey-item="pk_phone"]'),
	).toHaveCount(0);
	await expect(page.getByText("Passkey 已移除")).toBeVisible();
});

test("switching away from passkeys clears the passkey flash state", async ({
	page,
}) => {
	await installPasskeyBrowserMock(page);
	await installSettingsMocks(page, {
		passkeys: [
			{
				id: "pk_phone",
				label: "Passkey · 2026-04-20 09:00 UTC",
				created_at: "2026-04-20T09:00:00Z",
				last_used_at: "2026-04-22T08:30:00Z",
			},
		],
	});

	await page.goto("/settings?section=passkeys&passkey=registered");

	await expect(page.getByText("Passkey 已添加")).toBeVisible();
	await page
		.getByRole("link", { name: /GitHub 账号/ })
		.first()
		.click();

	await expect(page).toHaveURL(/section=github-accounts/);
	await expect(page).not.toHaveURL(/passkey=/);
	await expect(page.getByText("Passkey 已添加")).toHaveCount(0);
});
