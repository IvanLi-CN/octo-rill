import { Landing } from "@/pages/Landing";
import type { Meta, StoryObj } from "@storybook/react-vite";

const meta = {
	title: "Pages/Landing",
	component: Landing,
	parameters: {
		layout: "fullscreen",
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
	parameters: {
		docs: {
			description: {
				story:
					"默认的未登录首屏状态：左侧是品牌与产品价值的 hero 区，右侧保留独立登录卡片，避免品牌与 CTA 互相挤压。",
			},
		},
	},
};

export const WithError: Story = {
	args: {
		bootError: "Example error: unauthorized",
	},
	parameters: {
		docs: {
			description: {
				story: "模拟 OAuth 或引导阶段失败时的错误提示文案。",
			},
		},
	},
};
