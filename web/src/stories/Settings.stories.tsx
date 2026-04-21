import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useRef, useState } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, userEvent, within } from "storybook/test";

import type {
	LinuxDoConnectionResponse,
	MeProfileResponse,
	ReactionTokenStatusResponse,
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
			daily_boundary_local: "08:00",
			daily_boundary_time_zone: "Asia/Shanghai",
			daily_boundary_utc_offset_minutes: 480,
		},
	};
}

function buildMockProfile(): MeProfileResponse {
	return {
		user_id: "storybook-user",
		daily_brief_local_time: "08:00",
		daily_brief_time_zone: "Asia/Shanghai",
		last_active_at: "2026-04-18T08:00:00+08:00",
		include_own_releases: false,
	};
}

type SettingsStoryArgs = {
	section: SettingsSection;
	linuxdoStatus?: string;
	linuxdoAvailable: boolean;
	linuxdoConnection: LinuxDoConnectionResponse | null;
	reactionTokenStatus: ReactionTokenStatusResponse;
	profile: MeProfileResponse;
	themePreference: ThemePreference;
};

function SettingsStoryScene(args: SettingsStoryArgs) {
	const me = buildMockMeResponse();
	const originalFetchRef = useRef(globalThis.fetch);
	const [profile, setProfile] = useState(args.profile);
	const [section, setSection] = useState(args.section);

	useEffect(() => {
		setProfile(args.profile);
	}, [args.profile]);

	useEffect(() => {
		setSection(args.section);
	}, [args.section]);

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

		if (request.pathname === "/api/me/linuxdo" && method === "GET") {
			return jsonResponse({
				available: args.linuxdoAvailable,
				connection: args.linuxdoConnection,
			});
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
			return jsonResponse(args.reactionTokenStatus);
		}
		if (request.pathname === "/api/reaction-token/check" && method === "POST") {
			return jsonResponse({
				state: "valid",
				message: "token is valid",
			});
		}
		if (request.pathname === "/api/reaction-token" && method === "PUT") {
			return jsonResponse({
				configured: true,
				masked_token: "ghp_****_storybook_saved",
				check: {
					state: "valid",
					message: "token is valid",
					checked_at: "2026-04-18T08:01:00+08:00",
				},
			});
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
				if (!(target instanceof HTMLElement)) return;
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
					refreshPage: () => {},
				}}
			>
				<ThemeProvider defaultPreference={args.themePreference} persist={false}>
					<SettingsPage
						me={me}
						section={section}
						linuxdoStatus={args.linuxdoStatus}
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
					"Settings 页面统一承载 LinuxDO Connect 绑定、我的发布开关、GitHub PAT 配置与日报设置。它是普通用户设置的唯一主入口，并支持 `section` 深链定位。",
			},
		},
	},
	args: {
		section: "linuxdo",
		linuxdoAvailable: true,
		linuxdoConnection: null,
		reactionTokenStatus: {
			configured: false,
			masked_token: null,
			check: {
				state: "idle",
				message: null,
				checked_at: null,
			},
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
		await expect(canvas.getByText("LinuxDO 绑定")).toBeVisible();
		await expect(canvas.getByText("我的发布")).toBeVisible();
		await expect(canvas.getByText("GitHub PAT")).toBeVisible();
		await expect(canvas.getByText("日报设置")).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "Connect LinuxDO" }),
		).toBeVisible();

		await userEvent.click(canvas.getByRole("link", { name: "GitHub PAT" }));
		await expect(canvas.getByTestId("github-pat-guide-card")).toBeVisible();

		await userEvent.click(canvas.getByRole("link", { name: "我的发布" }));
		await expect(
			canvas.getByRole("switch", { name: "我的发布" }),
		).toBeVisible();

		await userEvent.click(canvas.getByRole("link", { name: "日报设置" }));
		await expect(canvas.getByText("日报时间")).toBeVisible();
	},
};

export const SwitchableSections: Story = {
	name: "Switchable Sections",
	parameters: {
		docs: {
			description: {
				story:
					"用于手动点击四个设置分区的交互式 Story。点击顶部导航即可在 LinuxDO、我的发布、GitHub PAT、日报设置之间切换。",
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
				message: "token is valid",
				checked_at: "2026-04-18T08:00:00+08:00",
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
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"深链到 `section=github-pat` 时，页面展示 GitHub classic PAT 的高仿 DOM mock，并直接预填建议值供用户照抄。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.queryByRole("heading", { name: "GitHub PAT 可用" }),
		).not.toBeInTheDocument();
		await expect(canvas.getByText("GitHub PAT 无效")).toBeVisible();
		const input = canvas.getByLabelText("GitHub PAT", { selector: "input" });
		await expect(input).toHaveAttribute("type", "password");
		await expect(input).toHaveAttribute("autocomplete", "new-password");
		await expect(input).toHaveAttribute("data-1p-ignore", "true");
		await expect(input).toHaveAttribute("data-form-type", "other");
		await expect(input).toHaveAttribute("data-secret-visible", "false");
		await userEvent.click(
			canvas.getByRole("button", { name: "显示 GitHub PAT" }),
		);
		await expect(input).toHaveAttribute("type", "text");
		await expect(input).toHaveAttribute("data-secret-visible", "true");
		const guide = canvas.getByTestId("github-pat-guide-card");
		await expect(guide).toBeVisible();
		await expect(
			within(guide).getByRole("textbox", { name: "Note" }),
		).toHaveValue("OctoRill release feedback");
		await expect(
			within(guide).getByRole("button", { name: "No expiration" }),
		).toBeVisible();
		await expect(
			canvas.getByText("当前环境未配置 LinuxDO Connect，按钮已禁用。"),
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
	args: {
		section: "my-releases",
		profile: {
			...buildMockProfile(),
			include_own_releases: false,
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
	},
};
