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
