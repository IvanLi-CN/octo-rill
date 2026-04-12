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
					"OctoRill 的公开入口页，用来说明产品定位并承接 GitHub OAuth 登录。适合在这里确认未登录态的首屏文案、登录按钮与错误反馈。\n\n相关公开文档：[快速开始](../quick-start.html) · [产品说明](../product.html)",
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
		await expect(
			canvas.getByRole("heading", {
				name: "把和你有关的 GitHub 动态放到一个首页里",
			}),
		).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: "连接到 GitHub" }),
		).toBeVisible();
		await expect(
			canvas.getByText(
				"这里集中看 Releases、被加星、被关注和 Inbox；Release 默认提供中文翻译。",
			),
		).toBeVisible();
		await expect(
			canvasElement.ownerDocument.body.querySelector("[data-theme-toggle]"),
		).not.toBeNull();
	},
	parameters: {
		docs: {
			description: {
				story:
					"默认的未登录首屏状态：桌面保留品牌说明区 + 独立登录卡，首屏只剩标题、短说明、三条卖点和一个主 CTA。",
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
		await expect(canvas.getByText("Example error: unauthorized")).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: "连接到 GitHub" }),
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
		await expect(
			canvas.getByRole("link", { name: "连接到 GitHub" }),
		).toBeVisible();
		await expect(canvas.getByText("Releases 信息流")).toBeVisible();
	},
	parameters: {
		docs: {
			description: {
				story:
					"移动端审阅入口：把登录卡提到品牌说明前，优先保证首屏即可看到唯一主 CTA。",
			},
		},
	},
};
