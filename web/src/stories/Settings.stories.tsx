import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useRef, useState } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, screen, userEvent, within } from "storybook/test";

import type {
	ApiKeySummary,
	GitHubConnectionResponse,
	LinuxDoConnectionResponse,
	MeProfileResponse,
	PasskeySummary,
	ReactionTokenStatusResponse,
	WebhookPushSettingsResponse,
} from "@/api";
import { SettingsPage } from "@/pages/Settings";
import type { SettingsSection } from "@/settings/routeState";
import { ThemeProvider } from "@/theme/ThemeProvider";
import type { ThemePreference } from "@/theme/theme";
import { VersionMonitorStateProvider } from "@/version/versionMonitor";

const SETTINGS_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	settingsGithubPatMobile390: {
		name: "Settings PAT mobile 390x844",
		styles: {
			height: "844px",
			width: "390px",
		},
		type: "mobile",
	},
	settingsGithubPatDesktop1280: {
		name: "Settings PAT desktop 1280x1000",
		styles: {
			height: "1000px",
			width: "1280px",
		},
		type: "desktop",
	},
} as const;

function svgAvatarDataUrl(
	label: string,
	background: string,
	foreground = "#ffffff",
) {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240"><rect width="240" height="240" rx="120" fill="${background}"/><text x="120" y="132" font-family="Inter,Arial,sans-serif" font-size="44" font-weight="700" text-anchor="middle" fill="${foreground}">${label}</text></svg>`,
	)}`;
}

function jsonResponse(payload: unknown, status = 200) {
	return new Response(JSON.stringify(payload), {
		status,
		headers: { "Content-Type": "application/json" },
	});
}

function buildMockMeResponse() {
	return {
		user: {
			id: "storybook-user",
			github_user_id: 42,
			login: "storybook-user",
			name: "Storybook User",
			avatar_url: svgAvatarDataUrl("SU", "#4f6a98"),
			email: "storybook-user@example.com",
			is_admin: false,
		},
		dashboard: {
			daily_boundary_local: "00:00",
			daily_boundary_time_zone: "Asia/Shanghai",
			daily_boundary_utc_offset_minutes: 480,
			include_own_releases: false,
		},
	};
}

function buildMockProfile(): MeProfileResponse {
	return {
		user_id: "storybook-user",
		daily_brief_schedule_local_time: "06:00",
		daily_brief_time_zone: "Asia/Shanghai",
		last_active_at: "2026-04-18T08:00:00+08:00",
		include_own_releases: false,
	};
}

const baseGitHubConnections: GitHubConnectionResponse[] = [
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

type SettingsStoryArgs = {
	section: SettingsSection;
	githubStatus?: string;
	linuxdoStatus?: string;
	passkeyStatus?: string;
	linuxdoAvailable: boolean;
	linuxdoConnection: LinuxDoConnectionResponse | null;
	githubConnections: GitHubConnectionResponse[];
	passkeys: PasskeySummary[];
	apiKeys: ApiKeySummary[];
	apiKeyDeleteError: boolean;
	passkeySupportOverride: boolean;
	reactionTokenStatus: ReactionTokenStatusResponse;
	profile: MeProfileResponse;
	themePreference: ThemePreference;
	webhookPush?: WebhookPushSettingsResponse;
};

function SettingsStoryScene(args: SettingsStoryArgs) {
	const me = buildMockMeResponse();
	const originalFetchRef = useRef(globalThis.fetch);
	const [profile, setProfile] = useState(args.profile);
	const [section, setSection] = useState(args.section);
	const [githubConnections, setGitHubConnections] = useState(
		args.githubConnections,
	);
	const [passkeys, setPasskeys] = useState(args.passkeys);
	const [apiKeys, setApiKeys] = useState(args.apiKeys);
	const [reactionTokenStatus, setReactionTokenStatus] = useState(
		args.reactionTokenStatus,
	);

	useEffect(() => {
		setProfile(args.profile);
	}, [args.profile]);

	useEffect(() => {
		setSection(args.section);
	}, [args.section]);

	useEffect(() => {
		setGitHubConnections(args.githubConnections);
	}, [args.githubConnections]);

	useEffect(() => {
		setPasskeys(args.passkeys);
	}, [args.passkeys]);

	useEffect(() => {
		setApiKeys(args.apiKeys);
	}, [args.apiKeys]);

	useEffect(() => {
		setReactionTokenStatus(args.reactionTokenStatus);
	}, [args.reactionTokenStatus]);

	globalThis.fetch = async (input, init) => {
		const requestUrl =
			typeof input === "string"
				? input
				: input instanceof URL
					? input.toString()
					: input.url;
		const request = new URL(requestUrl, window.location.origin);
		const method =
			init?.method ??
			(typeof input === "object" && "method" in input ? input.method : "GET");

		if (request.pathname === "/api/me/github-connections" && method === "GET") {
			return jsonResponse({ items: githubConnections });
		}
		if (
			request.pathname.startsWith("/api/me/github-connections/") &&
			method === "DELETE"
		) {
			const connectionId = request.pathname.split("/").at(-1);
			const nextConnections = githubConnections.filter(
				(connection) => connection.id !== connectionId,
			);
			setGitHubConnections(nextConnections);
			return jsonResponse({ items: nextConnections });
		}
		if (request.pathname === "/api/me/linuxdo" && method === "GET") {
			return jsonResponse({
				available: args.linuxdoAvailable,
				connection: args.linuxdoConnection,
			});
		}
		if (request.pathname === "/api/me/passkeys" && method === "GET") {
			return jsonResponse({ items: passkeys });
		}
		if (request.pathname === "/api/me/api-keys" && method === "GET") {
			return jsonResponse({ items: apiKeys });
		}
		if (request.pathname === "/api/me/api-keys" && method === "POST") {
			const body = init?.body ? JSON.parse(String(init.body)) : {};
			const createdAt = "2026-04-18T08:05:00+08:00";
			const item: ApiKeySummary = {
				id: `api_key_${apiKeys.length + 1}`,
				name: String(body.name || "API Key"),
				api_key: "orill_ak_storybook_created_plaintext_A1b2C3",
				masked_key: "orill_ak_storybook...A1b2C3",
				created_at: createdAt,
				last_used_at: null,
			};
			const nextApiKeys = [item, ...apiKeys];
			setApiKeys(nextApiKeys);
			return jsonResponse({
				item,
				api_key: "orill_ak_storybook_created_plaintext_A1b2C3",
			});
		}
		if (
			request.pathname.startsWith("/api/me/api-keys/") &&
			method === "DELETE"
		) {
			if (args.apiKeyDeleteError) {
				return jsonResponse(
					{
						error: {
							code: "api_key_revoke_failed",
							message: "撤销 API Key 失败，请稍后重试。",
						},
					},
					500,
				);
			}
			const apiKeyId = request.pathname.split("/").at(-1);
			const nextApiKeys = apiKeys.filter((apiKey) => apiKey.id !== apiKeyId);
			setApiKeys(nextApiKeys);
			return jsonResponse({ items: nextApiKeys });
		}
		if (
			request.pathname.startsWith("/api/me/passkeys/") &&
			method === "DELETE"
		) {
			const passkeyId = request.pathname.split("/").at(-1);
			const nextPasskeys = passkeys.filter(
				(passkey) => passkey.id !== passkeyId,
			);
			setPasskeys(nextPasskeys);
			return jsonResponse({ items: nextPasskeys });
		}
		if (request.pathname === "/api/me/linuxdo" && method === "DELETE") {
			return jsonResponse({
				available: args.linuxdoAvailable,
				connection: null,
			});
		}
		if (request.pathname === "/api/me/profile" && method === "GET") {
			return jsonResponse(profile);
		}
		if (request.pathname === "/api/me/webhook-push" && method === "GET") {
			if (args.webhookPush) return jsonResponse(args.webhookPush);
			return jsonResponse({
				enabled: false,
				include_own_releases: profile.include_own_releases,
				callback_ready: true,
				pat: {
					configured: reactionTokenStatus.configured,
					valid: true,
					owner_login: "storybook-user",
				},
				summary: {
					total: 2,
					registered: 1,
					missing: 1,
					permission_paused: 0,
					errors: 0,
					removable: 1,
				},
				schedule: {
					audit_interval_days: 7,
					last_started_at: null,
					next_started_at: null,
				},
				repos: [
					{
						repo_id: 1,
						owner_login: "storybook-user",
						repo_name: "octo-rill",
						repo_full_name: "storybook-user/octo-rill",
						is_private: false,
						hook_id: 10,
						status: "registered",
						error_kind: null,
						error_message: null,
						permission_paused: false,
						last_checked_at: null,
						last_registered_at: null,
					},
					{
						repo_id: 2,
						owner_login: "storybook-user",
						repo_name: "notes",
						repo_full_name: "storybook-user/notes",
						is_private: false,
						hook_id: null,
						status: "missing",
						error_kind: null,
						error_message: null,
						permission_paused: false,
						last_checked_at: null,
						last_registered_at: null,
					},
				],
			});
		}
		if (request.pathname === "/api/me/webhook-push" && method === "PATCH") {
			return jsonResponse({
				enabled: JSON.parse(String(init?.body ?? "{}")).enabled,
				task_id: "storybook-webhook-task",
				status: "queued",
				reused: false,
			});
		}
		if (
			request.pathname.startsWith("/api/me/webhook-push/") &&
			(method === "POST" || method === "DELETE")
		) {
			return jsonResponse({
				task_id: "storybook-webhook-task",
				status: "queued",
				reused: false,
			});
		}
		if (request.pathname === "/api/me/profile" && method === "PATCH") {
			const patch = init?.body ? JSON.parse(String(init.body)) : {};
			const nextProfile = {
				...profile,
				...patch,
			};
			setProfile(nextProfile);
			return jsonResponse(nextProfile);
		}
		if (request.pathname === "/api/reaction-token/status" && method === "GET") {
			return jsonResponse(reactionTokenStatus);
		}
		if (request.pathname === "/api/reaction-token/check" && method === "POST") {
			const owner = githubConnections.at(0)
				? {
						github_connection_id: githubConnections[0].id,
						github_user_id: githubConnections[0].github_user_id,
						login: githubConnections[0].login,
					}
				: null;
			return jsonResponse({
				state: "valid",
				message: owner
					? `token is valid for @${owner.login}`
					: "token is valid",
				owner,
			});
		}
		if (request.pathname === "/api/reaction-token" && method === "PUT") {
			const owner = githubConnections.at(0)
				? {
						github_connection_id: githubConnections[0].id,
						github_user_id: githubConnections[0].github_user_id,
						login: githubConnections[0].login,
					}
				: null;
			const nextStatus: ReactionTokenStatusResponse = {
				configured: true,
				masked_token: "ghp_****_storybook_saved",
				check: {
					state: "valid",
					message: owner
						? `token is valid for @${owner.login}`
						: "token is valid",
					checked_at: "2026-04-18T08:01:00+08:00",
				},
				owner,
			};
			setReactionTokenStatus(nextStatus);
			return jsonResponse(nextStatus);
		}

		return jsonResponse(
			{
				error: {
					code: "not_found",
					message: `unhandled ${method} ${request.pathname}`,
				},
			},
			404,
		);
	};

	useEffect(() => {
		return () => {
			globalThis.fetch = originalFetchRef.current;
		};
	}, []);

	return (
		<div
			onClickCapture={(event) => {
				const target = event.target;
				if (!(target instanceof Element)) return;
				const anchor = target.closest<HTMLAnchorElement>(
					"[data-settings-nav] a[href*='/settings']",
				);
				if (!anchor) return;
				event.preventDefault();
			}}
		>
			<VersionMonitorStateProvider
				value={{
					loadedVersion: "v2.4.6",
					availableVersion: null,
					hasUpdate: false,
					hasServiceWorkerUpdate: false,
					refreshPage: () => {},
				}}
			>
				<ThemeProvider defaultPreference={args.themePreference} persist={false}>
					<SettingsPage
						me={me}
						section={section}
						githubStatus={args.githubStatus}
						linuxdoStatus={args.linuxdoStatus}
						passkeyStatus={args.passkeyStatus}
						passkeySupportOverride={args.passkeySupportOverride}
						onSectionChange={setSection}
						onProfileSaved={() => {}}
					/>
				</ThemeProvider>
			</VersionMonitorStateProvider>
		</div>
	);
}

const connectedLinuxDo: LinuxDoConnectionResponse = {
	linuxdo_user_id: 114514,
	username: "linuxdo-storybook",
	name: "LinuxDO Storybook",
	avatar_url: svgAvatarDataUrl("LD", "#0ea5e9"),
	trust_level: 3,
	active: true,
	silenced: false,
	linked_at: "2026-04-16T10:00:00+08:00",
	updated_at: "2026-04-18T09:00:00+08:00",
};

const meta = {
	title: "Pages/Settings",
	component: SettingsStoryScene,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		viewport: {
			options: SETTINGS_VIEWPORTS,
		},
		docs: {
			description: {
				component:
					"Settings 页面统一承载 Passkey、GitHub 多账号绑定、LinuxDO Connect 绑定、我的发布开关、GitHub PAT 配置与日报设置。它是普通用户设置的唯一主入口，并支持 `section` 深链定位。",
			},
		},
	},
	args: {
		section: "github-accounts",
		githubStatus: "connected",
		passkeyStatus: undefined,
		linuxdoAvailable: true,
		linuxdoConnection: null,
		githubConnections: baseGitHubConnections,
		passkeys: [],
		apiKeys: [],
		apiKeyDeleteError: false,
		passkeySupportOverride: true,
		reactionTokenStatus: {
			configured: false,
			masked_token: null,
			check: {
				state: "idle",
				message: null,
				checked_at: null,
			},
			owner: null,
		},
		profile: buildMockProfile(),
		themePreference: "light",
	},
} satisfies Meta<typeof SettingsStoryScene>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "账号与偏好" }),
		).toBeVisible();
		await expect(canvas.getByText("GitHub 账号")).toBeVisible();
		await expect(canvas.getByText("日报设置")).toBeVisible();
		await expect(canvas.getByText("我的发布")).toBeVisible();
		await expect(canvas.getByText("GitHub PAT")).toBeVisible();
		await expect(canvas.getByText("API Key")).toBeVisible();
		await expect(canvas.getByText("LinuxDO 绑定")).toBeVisible();
		await expect(canvas.getByAltText("storybook-user avatar")).toBeVisible();
		await expect(canvas.getByAltText("storybook-ops avatar")).toBeVisible();

		await userEvent.click(canvas.getByRole("link", { name: "GitHub PAT" }));
		await expect(canvas.getByTestId("github-pat-guide-card")).toBeVisible();

		await userEvent.click(canvas.getByRole("link", { name: "API Key" }));
		await expect(canvas.getByText("还没有 API Key。")).toBeVisible();

		await userEvent.click(canvas.getByRole("link", { name: "我的发布" }));
		await expect(
			canvas.getByRole("switch", { name: "我的发布" }),
		).toBeVisible();

		await userEvent.click(canvas.getByRole("link", { name: "日报设置" }));
		await expect(canvas.getByLabelText("IANA 时区")).toBeVisible();
	},
};

export const GitHubAccounts: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("GitHub 账号已绑定")).toBeVisible();
		await expect(canvas.getByText("@storybook-user")).toBeVisible();
		await expect(canvas.getByText("@storybook-ops")).toBeVisible();
		await expect(canvas.getByAltText("storybook-user avatar")).toBeVisible();
		await expect(canvas.getByAltText("storybook-ops avatar")).toBeVisible();
		await expect(canvas.getByRole("button", { name: "解绑" })).toBeVisible();
	},
	parameters: {
		docs: {
			description: {
				story:
					"深链到 `section=github-accounts` 时，应展示多 GitHub 绑定列表，以及追加绑定 / 解绑入口；所有绑定账号都参与登录、同步与 PAT 校验。",
			},
		},
	},
};

export const PasskeysEmpty: Story = {
	args: {
		section: "passkeys",
		passkeySupportOverride: true,
		passkeys: [],
	},
	parameters: {
		docs: {
			description: {
				story:
					"深链到 `section=passkeys` 的空态：用于展示首次添加 Passkey 的说明、CTA 和 GitHub 仍然是恢复路径的约束。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("Passkeys")).toBeVisible();
		await expect(
			canvas.getByText("还没有可直接登录的 Passkey。"),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "添加 Passkey" }),
		).toBeVisible();
	},
};

export const PasskeysMultipleDevices: Story = {
	args: {
		section: "passkeys",
		passkeySupportOverride: true,
		passkeyStatus: "registered",
		passkeys: [
			{
				id: "passkey_phone",
				label: "Passkey · 2026-04-20 09:00 UTC",
				created_at: "2026-04-20T09:00:00Z",
				last_used_at: "2026-04-22T08:30:00Z",
			},
			{
				id: "passkey_laptop",
				label: "Passkey · 2026-04-21 11:15 UTC",
				created_at: "2026-04-21T11:15:00Z",
				last_used_at: null,
			},
		],
	},
	parameters: {
		docs: {
			description: {
				story:
					"多设备态：同时展示 flash 状态、两把 Passkey 列表与最近使用时间，作为设置页视觉证据的稳定入口。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("Passkey 已添加")).toBeVisible();
		await expect(
			canvas.getByText("Passkey · 2026-04-20 09:00 UTC"),
		).toBeVisible();
		await expect(
			canvas.getByText("Passkey · 2026-04-21 11:15 UTC"),
		).toBeVisible();
		await expect(canvas.getAllByRole("button", { name: "移除" })).toHaveLength(
			2,
		);
	},
};

export const ApiKeysEmpty: Story = {
	args: {
		section: "api-keys",
		apiKeys: [],
	},
	parameters: {
		docs: {
			description: {
				story: "API Key 空态：展示业务接口权限边界、命名输入与创建入口。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("API Key")).toBeVisible();
		await expect(canvas.getByText("还没有 API Key。")).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "创建 API Key" }),
		).toBeVisible();
	},
};

export const ApiKeysCreatedPersisted: Story = {
	args: {
		section: "api-keys",
		apiKeys: [],
	},
	parameters: {
		docs: {
			description: {
				story: "创建成功态：完整 Key 会加入列表，用户之后仍可回显和复制。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.type(canvas.getByLabelText("名称"), "local sync script");
		await userEvent.click(canvas.getByRole("button", { name: "创建 API Key" }));
		await expect(await canvas.findByText("API Key 已创建")).toBeVisible();
		await expect(
			canvas.getByText("orill_ak_storybook_created_plaintext_A1b2C3"),
		).toBeVisible();
		await expect(
			canvas.getByText("之后仍可在本页查看和复制完整 Key。"),
		).toBeVisible();
	},
};

export const ApiKeysRevokeConfirm: Story = {
	args: {
		section: "api-keys",
		apiKeys: [
			{
				id: "api_key_cli",
				name: "CLI sync",
				api_key: "orill_ak_cli_prod_full_plaintext_x9Y8z7",
				masked_key: "orill_ak_cli_prod...x9Y8z7",
				created_at: "2026-04-12T08:00:00+08:00",
				last_used_at: "2026-04-18T07:40:00+08:00",
			},
		],
	},
	parameters: {
		docs: {
			description: {
				story: "撤销确认态：点击撤销后必须二次确认，确认前不会删除 Key。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("orill_ak_cli_prod_full_plaintext_x9Y8z7"),
		).toBeVisible();
		await expect(canvas.getByRole("button", { name: "撤销" })).toBeVisible();
		await userEvent.click(canvas.getByRole("button", { name: "撤销" }));
		await expect(await screen.findByText("确认撤销 API Key")).toBeVisible();
		await expect(screen.getByRole("button", { name: "取消" })).toBeVisible();
		await expect(
			screen.getByRole("button", { name: "确认撤销" }),
		).toBeVisible();
	},
};

export const ApiKeysListAndRevokeError: Story = {
	args: {
		section: "api-keys",
		apiKeyDeleteError: true,
		apiKeys: [
			{
				id: "api_key_cli",
				name: "CLI sync",
				api_key: "orill_ak_cli_prod_full_plaintext_x9Y8z7",
				masked_key: "orill_ak_cli_prod...x9Y8z7",
				created_at: "2026-04-12T08:00:00+08:00",
				last_used_at: "2026-04-18T07:40:00+08:00",
			},
			{
				id: "api_key_worker",
				name: "brief worker",
				api_key: "orill_ak_worker_full_plaintext_k3LmN4",
				masked_key: "orill_ak_worker...k3LmN4",
				created_at: "2026-04-14T12:20:00+08:00",
				last_used_at: null,
			},
		],
	},
	parameters: {
		docs: {
			description: {
				story:
					"API Key 列表与撤销错误态：验证完整 Key 回显、metadata、二次确认和失败反馈。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("CLI sync")).toBeVisible();
		await expect(canvas.getByText("brief worker")).toBeVisible();
		await expect(
			canvas.getByText("orill_ak_cli_prod_full_plaintext_x9Y8z7"),
		).toBeVisible();
		await userEvent.click(canvas.getAllByRole("button", { name: "撤销" })[0]);
		await expect(await screen.findByText("确认撤销 API Key")).toBeVisible();
		await userEvent.click(screen.getByRole("button", { name: "确认撤销" }));
		await expect(
			await canvas.findByText("撤销 API Key 失败，请稍后重试。"),
		).toBeVisible();
	},
};

export const SwitchableSections: Story = {
	name: "Switchable Sections",
	parameters: {
		docs: {
			description: {
				story:
					"用于手动点击设置分区的交互式 Story。点击顶部导航即可在 Passkeys、GitHub 账号、LinuxDO、我的发布、GitHub PAT 与日报设置之间切换。",
			},
		},
	},
};

export const ConnectedAndConfigured: Story = {
	args: {
		section: "linuxdo",
		linuxdoStatus: "connected",
		linuxdoConnection: connectedLinuxDo,
		reactionTokenStatus: {
			configured: true,
			masked_token: "ghp_****_storybook",
			check: {
				state: "valid",
				message: "token is valid for @storybook-user",
				checked_at: "2026-04-18T08:00:00+08:00",
			},
			owner: {
				github_connection_id: "ghconn_primary",
				github_user_id: 42,
				login: "storybook-user",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("LinuxDO 已绑定")).toBeVisible();
		await expect(canvas.getByText("@linuxdo-storybook")).toBeVisible();
		await expect(canvas.getByText("3")).toBeVisible();
		await expect(canvas.getByText("ghp_****_storybook")).toBeVisible();
	},
};

export const DeepLinkedGitHubPat: Story = {
	args: {
		section: "github-pat",
		linuxdoAvailable: false,
		reactionTokenStatus: {
			configured: true,
			masked_token: "ghp_****_invalid",
			check: {
				state: "invalid",
				message: "PAT 无效或已过期，请重新填写并校验。",
				checked_at: "2026-04-18T08:02:00+08:00",
			},
			owner: {
				github_connection_id: "ghconn_secondary",
				github_user_id: 84,
				login: "storybook-ops",
			},
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"深链到 `section=github-pat` 时，GitHub PAT 区域会被高亮；这里同时覆盖 LinuxDO 未配置、PAT 失效与 PAT owner 展示。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.queryByRole("heading", { name: "GitHub PAT 可用" }),
		).not.toBeInTheDocument();
		await expect(await canvas.findByText("已保存 GitHub PAT")).toBeVisible();
		await expect(
			await canvas.findByText("PAT 无效或已过期，请重新填写并校验。"),
		).toBeVisible();
		const input = canvasElement.querySelector(
			"#settings-reaction-pat",
		) as HTMLInputElement | null;
		if (!input) {
			throw new Error("expected settings GitHub PAT input");
		}
		await expect(input).toHaveAttribute("type", "password");
		await expect(input).toHaveAttribute("autocomplete", "new-password");
		await expect(input).toHaveAttribute(
			"aria-describedby",
			"settings-reaction-pat-hidden-hint",
		);
		await expect(input).toHaveAttribute("data-1p-ignore", "true");
		await expect(input).toHaveAttribute("data-form-type", "other");
		await expect(input).toHaveAttribute("data-secret-visible", "false");
		await expect(input).toHaveAttribute(
			"data-secret-mask-mode",
			"native-password",
		);
		await userEvent.click(
			canvas.getByRole("button", { name: "显示 GitHub PAT" }),
		);
		await expect(input).toHaveAttribute("type", "text");
		await expect(input).toHaveAttribute("data-secret-visible", "true");
		await expect(input).toHaveAttribute("data-secret-mask-mode", "plain-text");
		await expect(input).not.toHaveAttribute("aria-describedby");
		const guide = canvas.getByTestId("github-pat-guide-card");
		await expect(guide).toBeVisible();
		await expect(
			within(guide).getByRole("textbox", { name: "Note" }),
		).toHaveValue("OctoRill release feedback");
		await expect(
			within(guide).getByRole("button", { name: "No expiration" }),
		).toBeVisible();
		await expect(
			canvas.getByText("@storybook-ops", { exact: true }),
		).toBeVisible();
	},
};

export const GitHubPatDesktopLight: Story = {
	...DeepLinkedGitHubPat,
	name: "GitHub PAT / Desktop / Light",
	globals: {
		viewport: {
			value: "settingsGithubPatDesktop1280",
			isRotated: false,
		},
	},
	args: {
		...DeepLinkedGitHubPat.args,
		themePreference: "light",
	},
};

export const GitHubPatDesktopDark: Story = {
	...DeepLinkedGitHubPat,
	name: "GitHub PAT / Desktop / Dark",
	globals: {
		viewport: {
			value: "settingsGithubPatDesktop1280",
			isRotated: false,
		},
	},
	args: {
		...DeepLinkedGitHubPat.args,
		themePreference: "dark",
	},
};

export const GitHubPatMobileLight: Story = {
	...DeepLinkedGitHubPat,
	name: "GitHub PAT / Mobile / Light",
	globals: {
		viewport: {
			value: "settingsGithubPatMobile390",
			isRotated: false,
		},
	},
	args: {
		...DeepLinkedGitHubPat.args,
		themePreference: "light",
	},
};

export const GitHubPatMobileDark: Story = {
	...DeepLinkedGitHubPat,
	name: "GitHub PAT / Mobile / Dark",
	globals: {
		viewport: {
			value: "settingsGithubPatMobile390",
			isRotated: false,
		},
	},
	args: {
		...DeepLinkedGitHubPat.args,
		themePreference: "dark",
	},
};

export const DeepLinkedMyReleases: Story = {
	globals: {
		viewport: { value: "settingsGithubPatDesktop1280", isRotated: false },
	},
	args: {
		section: "my-releases",
		profile: {
			...buildMockProfile(),
			include_own_releases: false,
		},
		reactionTokenStatus: {
			configured: true,
			masked_token: "ghp_****_storybook",
			check: {
				state: "valid",
				message: "token is valid for @storybook-user",
				checked_at: "2026-04-18T08:00:00+08:00",
			},
			owner: {
				github_connection_id: "ghconn_primary",
				github_user_id: 42,
				login: "storybook-user",
			},
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"深链到 `section=my-releases` 时，应展示独立的“我的发布”开关，并允许用户保存后立即回显开启状态。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const switchControl = canvas.getByRole("switch", { name: "我的发布" });
		await expect(switchControl).toHaveAttribute("aria-checked", "false");
		await expect(canvas.getByText("仅显示已加星仓库")).toBeVisible();

		await userEvent.click(switchControl);
		await userEvent.click(
			canvas.getByRole("button", { name: "保存“我的发布”" }),
		);

		await expect(switchControl).toHaveAttribute("aria-checked", "true");
		await expect(canvas.getByText("已纳入我的发布")).toBeVisible();

		const webhookSwitch = await canvas.findByRole("switch", {
			name: "Webhook 推送",
		});
		await userEvent.click(webhookSwitch);
		await expect(
			await screen.findByRole("alertdialog", { name: "开启 Webhook 推送？" }),
		).toBeVisible();
		await expect(
			screen.getByRole("button", { name: "确认开启并注册" }),
		).toBeVisible();
	},
};

export const WebhookPermissionPaused: Story = {
	globals: {
		viewport: { value: "settingsGithubPatDesktop1280", isRotated: false },
	},
	args: {
		...DeepLinkedMyReleases.args,
		profile: { ...buildMockProfile(), include_own_releases: true },
		webhookPush: {
			enabled: true,
			include_own_releases: true,
			callback_ready: true,
			pat: { configured: true, valid: true, owner_login: "storybook-user" },
			summary: {
				total: 3,
				registered: 1,
				missing: 1,
				permission_paused: 1,
				errors: 0,
				removable: 2,
			},
			schedule: {
				audit_interval_days: 7,
				last_started_at: "2026-08-08T02:00:00Z",
				next_started_at: "2026-08-15T02:00:00Z",
			},
			repos: [
				{
					repo_id: 1,
					owner_login: "storybook-user",
					repo_name: "octo-rill",
					repo_full_name: "storybook-user/octo-rill",
					is_private: false,
					hook_id: 10,
					status: "registered",
					error_kind: null,
					error_message: null,
					permission_paused: false,
					last_checked_at: "2026-08-08T02:00:00Z",
					last_registered_at: "2026-08-08T02:00:00Z",
				},
				{
					repo_id: 2,
					owner_login: "storybook-user",
					repo_name: "private-service",
					repo_full_name: "storybook-user/private-service",
					is_private: true,
					hook_id: null,
					status: "permission_paused",
					error_kind: "permission",
					error_message:
						"GitHub 拒绝管理该仓库的 webhook。请更新 PAT 权限后手动注册。",
					permission_paused: true,
					last_checked_at: "2026-08-08T02:00:00Z",
					last_registered_at: null,
				},
				{
					repo_id: 3,
					owner_login: "storybook-user",
					repo_name: "notes",
					repo_full_name: "storybook-user/notes",
					is_private: false,
					hook_id: null,
					status: "missing",
					error_kind: null,
					error_message: null,
					permission_paused: false,
					last_checked_at: "2026-08-08T02:00:00Z",
					last_registered_at: null,
				},
			],
		},
	},
};

export const WebhookDeleteReady: Story = {
	globals: {
		viewport: { value: "settingsGithubPatMobile390", isRotated: false },
	},
	args: {
		...WebhookPermissionPaused.args,
		webhookPush: {
			...(WebhookPermissionPaused.args
				?.webhookPush as WebhookPushSettingsResponse),
			enabled: false,
			summary: {
				total: 3,
				registered: 1,
				missing: 1,
				permission_paused: 1,
				errors: 0,
				removable: 2,
			},
			repos: (
				WebhookPermissionPaused.args?.webhookPush as WebhookPushSettingsResponse
			).repos.map((repo, index) =>
				index === 0 ? { ...repo, status: "delete_pending" } : repo,
			),
		},
	},
};
