import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useRef } from "react";
import { expect, within } from "storybook/test";

import { BindGitHubPage } from "@/pages/BindGitHub";
import { VersionMonitorStateProvider } from "@/version/versionMonitor";

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

type BindGitHubStoryArgs = {
	linuxdoStatus?: string;
	passkeyStatus?: string;
	pendingLinuxDo: {
		linuxdo_user_id: number;
		username: string;
		name: string | null;
		avatar_url: string;
		trust_level: number;
		active: boolean;
		silenced: boolean;
	} | null;
	pendingPasskey: {
		label: string;
		created_at: string;
	} | null;
	linuxdoAvailable: boolean;
};

function BindGitHubStoryScene(args: BindGitHubStoryArgs) {
	const originalFetchRef = useRef(globalThis.fetch);

	globalThis.fetch = async (input) => {
		const requestUrl =
			typeof input === "string"
				? input
				: input instanceof URL
					? input.toString()
					: input.url;
		const request = new URL(requestUrl, window.location.origin);
		if (request.pathname === "/api/auth/bind-context") {
			return jsonResponse({
				linuxdo_available: args.linuxdoAvailable,
				pending_linuxdo: args.pendingLinuxDo,
				pending_passkey: args.pendingPasskey,
			});
		}
		return jsonResponse(
			{
				error: {
					code: "not_found",
					message: `unhandled GET ${request.pathname}`,
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
		<VersionMonitorStateProvider
			value={{
				loadedVersion: "v2.4.6",
				availableVersion: null,
				hasUpdate: false,
				refreshPage: () => {},
			}}
		>
			<BindGitHubPage
				linuxdoStatus={args.linuxdoStatus}
				passkeyStatus={args.passkeyStatus}
			/>
		</VersionMonitorStateProvider>
	);
}

const meta = {
	title: "Pages/BindGitHub",
	component: BindGitHubStoryScene,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"LinuxDO 首登后没有现成 GitHub 绑定时，会进入这个补绑页。这里需要稳定展示 LinuxDO 待落地快照、GitHub CTA 与冲突提示。",
			},
		},
	},
	args: {
		linuxdoStatus: "connected",
		passkeyStatus: undefined,
		linuxdoAvailable: true,
		pendingLinuxDo: {
			linuxdo_user_id: 9527,
			username: "linuxdo-first-login",
			name: "LinuxDO First Login",
			avatar_url: svgAvatarDataUrl("LD", "#0ea5e9"),
			trust_level: 2,
			active: true,
			silenced: false,
		},
		pendingPasskey: null,
	},
} satisfies Meta<typeof BindGitHubStoryScene>;

export default meta;
type Story = StoryObj<typeof meta>;

export const PendingLinuxDo: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("可以继续绑定 GitHub")).toBeVisible();
		await expect(canvas.getByText("LinuxDO First Login")).toBeVisible();
		const githubLink = canvas.getByRole("link", {
			name: "绑定 GitHub 并继续",
		});
		await expect(githubLink).toBeVisible();
		expect(
			githubLink.querySelector('[data-auth-provider-icon="github"]'),
		).not.toBeNull();
	},
	parameters: {
		docs: {
			description: {
				story:
					"默认补绑态：带着 LinuxDO 快照进入，等待用户补一个 GitHub 账号后完成内部账号落地。",
			},
		},
	},
};

export const ConflictState: Story = {
	args: {
		linuxdoStatus: "github_already_bound",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("GitHub 账号已被占用")).toBeVisible();
		const githubLink = canvas.getByRole("link", {
			name: "绑定 GitHub 并继续",
		});
		await expect(githubLink).toBeVisible();
		expect(
			githubLink.querySelector('[data-auth-provider-icon="github"]'),
		).not.toBeNull();
	},
};

export const PendingPasskey: Story = {
	args: {
		linuxdoStatus: undefined,
		passkeyStatus: "created",
		pendingLinuxDo: null,
		pendingPasskey: {
			label: "Passkey · 2026-04-22 10:12 UTC",
			created_at: "2026-04-22T10:12:00Z",
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"Passkey-first onboarding：匿名用户先创建 Passkey，再来到 `/bind/github` 继续补 GitHub 绑定。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("Passkey 已暂存")).toBeVisible();
		await expect(canvas.getByText("待挂接的 Passkey")).toBeVisible();
		await expect(
			canvas.getByText("Passkey · 2026-04-22 10:12 UTC"),
		).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: "绑定 GitHub 并继续" }),
		).toBeVisible();
	},
};

export const MissingPendingContext: Story = {
	args: {
		linuxdoStatus: undefined,
		passkeyStatus: undefined,
		pendingLinuxDo: null,
		pendingPasskey: null,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("当前没有待完成的 LinuxDO 绑定。"),
		).toBeVisible();
		const linuxDoLink = canvas.getByRole("link", {
			name: "重新使用 LinuxDO 登录",
		});
		await expect(linuxDoLink).toBeVisible();
		expect(
			linuxDoLink.querySelector('[data-auth-provider-icon="linuxdo"]'),
		).not.toBeNull();
	},
};
