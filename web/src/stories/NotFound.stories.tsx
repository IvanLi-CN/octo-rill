import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import { NotFoundPage } from "@/pages/NotFound";

const meta = {
	title: "Pages/Not Found",
	component: NotFoundPage,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"全局 404 页面用于承接服务端 SPA document fallback 后的未知路由。它必须在应用壳层成功启动后，明确显示“页面不存在”，而不是静默回到首页或空白页。",
			},
		},
	},
	args: {
		isAuthenticated: true,
		pathname: "/does-not-exist",
	},
} satisfies Meta<typeof NotFoundPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Authenticated: Story = {
	parameters: {
		docs: {
			description: {
				story:
					"登录后的未知路由应保留应用壳层，并提供返回工作台与进入设置页的稳定 CTA。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "页面不存在" }),
		).toBeVisible();
		await expect(canvas.getByText("/does-not-exist")).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: "返回工作台" }),
		).toBeVisible();
		await expect(canvas.getByRole("link", { name: "打开设置" })).toBeVisible();
	},
};

export const Guest: Story = {
	args: {
		isAuthenticated: false,
		pathname: "/missing-public-page",
	},
	parameters: {
		docs: {
			description: {
				story:
					"未登录状态下，404 页面应引导用户回到 Landing 或直接发起 GitHub 登录，而不是显示站内受限入口。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "页面不存在" }),
		).toBeVisible();
		await expect(canvas.getByText("/missing-public-page")).toBeVisible();
		await expect(canvas.getByRole("link", { name: "回到首页" })).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: "连接到 GitHub" }),
		).toBeVisible();
	},
};
