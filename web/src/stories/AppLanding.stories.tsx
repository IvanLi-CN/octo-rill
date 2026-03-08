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
					"OctoRill 的公开入口页，用来说明产品定位并承接 GitHub OAuth 登录。适合在这里确认未登录态的首屏文案、登录按钮与错误反馈。",
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
				story: "默认的未登录首屏状态，展示产品简介与 GitHub 登录入口。",
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
