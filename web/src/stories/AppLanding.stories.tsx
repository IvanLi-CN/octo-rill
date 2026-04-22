import { Landing } from "@/pages/Landing";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, within } from "storybook/test";

const LANDING_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	landingMobileFold: {
		name: "Landing mobile 375x667",
		styles: {
			height: "667px",
			width: "375px",
		},
		type: "mobile",
	},
} as const;

const meta = {
	title: "Pages/Landing",
	component: Landing,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		viewport: {
			options: LANDING_VIEWPORTS,
		},
		docs: {
			description: {
				component:
					"OctoRill 的公开入口页，用来说明产品定位并承接 GitHub / LinuxDO / Passkey 三条登录入口。适合在这里确认未登录态的首屏文案、Passkey fallback 与错误反馈。\n\n相关公开文档：[快速开始](../quick-start.html) · [产品说明](../product.html)",
			},
		},
	},
} satisfies Meta<typeof Landing>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	args: {
		bootError: null,
		passkeySupportOverride: true,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const githubLink = canvas.getByRole("link", { name: "使用 GitHub 登录" });
		const linuxDoLink = canvas.getByRole("link", { name: "使用 LinuxDO 登录" });
		await expect(
			canvas.getByRole("heading", {
				name: "集中查看与你相关的 GitHub 动态",
			}),
		).toBeVisible();
		await expect(githubLink).toBeVisible();
		await expect(linuxDoLink).toBeVisible();
		expect(
			githubLink.querySelector('[data-auth-provider-icon="github"]'),
		).not.toBeNull();
		expect(
			linuxDoLink.querySelector('[data-auth-provider-icon="linuxdo"]'),
		).not.toBeNull();
		await expect(
			canvas.getByRole("button", { name: "使用 Passkey 登录" }),
		).toBeVisible();
		expect(
			canvas
				.getByRole("button", { name: "使用 Passkey 登录" })
				.querySelector('[data-auth-provider-icon="passkey"]'),
		).not.toBeNull();
		await expect(
			canvas.getByRole("button", {
				name: "首次使用？创建 Passkey 并继续绑定 GitHub",
			}),
		).toBeVisible();
		await expect(
			canvas.getByText(
				"登录后可在同一页面查看发布更新、获星与关注动态，并使用日报与通知入口；发布内容支持中文翻译与要点整理。",
			),
		).toBeVisible();
		await expect(canvas.getByText("查看获星与关注变化")).toBeVisible();
		await expect(canvas.getByText("查看发布译文与要点")).toBeVisible();
		await expect(
			canvasElement.ownerDocument.body.querySelector("[data-theme-toggle]"),
		).not.toBeNull();
	},
	parameters: {
		docs: {
			description: {
				story:
					"默认未登录首屏：桌面保留品牌说明区 + 独立登录卡，同时暴露 GitHub / LinuxDO / Passkey 入口，其中 Passkey 支持返回用户直登与首登先建后绑。",
			},
		},
	},
};

export const WithError: Story = {
	args: {
		bootError: "Example error: unauthorized",
		passkeySupportOverride: true,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("Example error: unauthorized")).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "使用 Passkey 登录" }),
		).toBeVisible();
	},
	parameters: {
		docs: {
			description: {
				story: "模拟 OAuth 或引导阶段失败时的错误提示文案。",
			},
		},
	},
};

export const PasskeyUnsupported: Story = {
	args: {
		bootError: null,
		passkeySupportOverride: false,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText(
				"当前浏览器不支持 Passkey；你仍然可以继续使用 GitHub / LinuxDO 登录。",
			),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "使用 Passkey 登录" }),
		).toBeDisabled();
	},
	parameters: {
		docs: {
			description: {
				story: "浏览器不支持 Passkey 时，CTA 会被禁用并显示明确 fallback。",
			},
		},
	},
};

export const MobilePriority: Story = {
	name: "Mobile priority CTA",
	args: {
		bootError: null,
		passkeySupportOverride: true,
	},
	globals: {
		viewport: {
			value: "landingMobileFold",
			isRotated: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("link", { name: "使用 GitHub 登录" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: "使用 LinuxDO 登录" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "使用 Passkey 登录" }),
		).toBeVisible();
		await expect(canvas.getByText("发布更新", { exact: true })).toBeVisible();
		await expect(canvas.getByText("查看日报与通知入口")).toBeVisible();
	},
	parameters: {
		docs: {
			description: {
				story:
					"移动端审阅入口：登录卡仍然排在品牌说明前，优先保证首屏能看到 GitHub / LinuxDO / Passkey 三个主要入口。",
			},
		},
	},
};
