import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "./button";

const meta = {
	title: "UI/Button",
	component: Button,
	args: {
		children: "Click me",
	},
	parameters: {
		layout: "centered",
	},
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Secondary: Story = {
	args: {
		variant: "secondary",
		children: "Secondary",
	},
};

export const Outline: Story = {
	args: {
		variant: "outline",
		children: "Outline",
	},
};

export const Disabled: Story = {
	args: {
		disabled: true,
		children: "Disabled",
	},
};

export const Large: Story = {
	args: {
		size: "lg",
		children: "Large",
	},
};

export const AsLink: Story = {
	render: (args) => (
		<Button asChild {...args}>
			<a href="#storybook">Open docs</a>
		</Button>
	),
	args: {
		variant: "link",
	},
};
