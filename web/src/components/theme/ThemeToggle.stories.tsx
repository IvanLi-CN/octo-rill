import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { ThemeToggle } from "@/components/theme/ThemeToggle";

const meta = {
	title: "UI/Theme Toggle",
	component: ThemeToggle,
	tags: ["autodocs"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"全局三态主题切换入口。真实页面中挂在页头 action 区，使用更紧凑的图标胶囊在 `浅色 / 深色 / 跟随系统` 之间切换当前应用主题。",
			},
		},
	},
} satisfies Meta<typeof ThemeToggle>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const rootDocument = canvasElement.ownerDocument;
		const canvas = within(rootDocument.body);
		const toggle = canvas.getByRole("group", { name: "主题模式" });
		const lightButton = within(toggle).getByRole("button", { name: "浅色" });
		const darkButton = within(toggle).getByRole("button", { name: "深色" });
		const systemButton = within(toggle).getByRole("button", {
			name: "跟随系统",
		});

		await expect(lightButton).toHaveAttribute("aria-pressed", "true");

		await userEvent.click(darkButton);
		await expect(darkButton).toHaveAttribute("aria-pressed", "true");
		expect(rootDocument.documentElement.classList.contains("dark")).toBe(true);

		await userEvent.click(systemButton);
		await expect(systemButton).toHaveAttribute("aria-pressed", "true");
	},
};

export const Compact: Story = {
	args: {
		compact: true,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const button = canvas.getByRole("button", {
			name: /主题模式：浅色/,
		});

		await expect(button).toHaveAttribute("data-theme-toggle-compact", "true");
		await userEvent.click(button);
		await expect(button).toHaveAttribute(
			"aria-label",
			expect.stringContaining("深色"),
		);
	},
	parameters: {
		docs: {
			description: {
				story:
					"紧凑模式供移动端页头使用，折叠成单按钮循环切换 `浅色 / 深色 / 跟随系统`，避免主题控件独占一整行。",
			},
		},
	},
};
