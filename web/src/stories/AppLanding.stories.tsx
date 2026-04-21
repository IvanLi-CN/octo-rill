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
					"OctoRill 的公开入口页，用来说明产品定位并承接 GitHub / LinuxDO 双登录入口。适合在这里确认未登录态的首屏文案、登录按钮与错误反馈。\n\n相关公开文档：[快速开始](../quick-start.html) · [产品说明](../product.html)",
			},
		},
	},
} satisfies Meta<typeof Landing>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	args: {
		bootError: null,
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
					"默认的未登录首屏状态：桌面保留品牌说明区 + 独立登录卡，首屏明确说明 OctoRill 面向前台用户提供发布更新阅读、社交动态、日报与通知入口，并同时暴露 GitHub / LinuxDO 两个入口。",
			},
		},
	},
};

export const WithError: Story = {
	args: {
		bootError: "Example error: unauthorized",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const githubLink = canvas.getByRole("link", { name: "使用 GitHub 登录" });
		const linuxDoLink = canvas.getByRole("link", { name: "使用 LinuxDO 登录" });
		await expect(canvas.getByText("Example error: unauthorized")).toBeVisible();
		await expect(githubLink).toBeVisible();
		await expect(linuxDoLink).toBeVisible();
		expect(
			githubLink.querySelector('[data-auth-provider-icon="github"]'),
		).not.toBeNull();
		expect(
			linuxDoLink.querySelector('[data-auth-provider-icon="linuxdo"]'),
		).not.toBeNull();
	},
	parameters: {
		docs: {
			description: {
				story: "模拟 OAuth 或引导阶段失败时的错误提示文案。",
			},
		},
	},
};

export const MobilePriority: Story = {
	name: "Mobile priority CTA",
	args: {
		bootError: null,
	},
	globals: {
		viewport: {
			value: "landingMobileFold",
			isRotated: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const githubLink = canvas.getByRole("link", { name: "使用 GitHub 登录" });
		const linuxDoLink = canvas.getByRole("link", { name: "使用 LinuxDO 登录" });
		await expect(githubLink).toBeVisible();
		await expect(linuxDoLink).toBeVisible();
		expect(
			githubLink.querySelector('[data-auth-provider-icon="github"]'),
		).not.toBeNull();
		expect(
			linuxDoLink.querySelector('[data-auth-provider-icon="linuxdo"]'),
		).not.toBeNull();
		await expect(canvas.getByText("发布更新", { exact: true })).toBeVisible();
		await expect(canvas.getByText("查看日报与通知入口")).toBeVisible();
	},
	parameters: {
		docs: {
			description: {
				story:
					"移动端审阅入口：把登录卡提到品牌说明前，优先保证首屏即可看到 GitHub / LinuxDO 双 CTA。",
			},
		},
	},
};
