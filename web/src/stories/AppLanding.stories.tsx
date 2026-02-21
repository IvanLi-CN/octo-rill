import { Landing } from "@/pages/Landing";
import type { Meta, StoryObj } from "@storybook/react-vite";

const meta = {
	title: "Pages/Landing",
	component: Landing,
	parameters: {
		layout: "fullscreen",
	},
} satisfies Meta<typeof Landing>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	args: {
		bootError: null,
	},
};

export const WithError: Story = {
	args: {
		bootError: "Example error: unauthorized",
	},
};
