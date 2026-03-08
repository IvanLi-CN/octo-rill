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
		docs: {
			description: {
				component:
					"OctoRill 中最常用的交互按钮。这里集中验证不同 variant、尺寸与链接模式，确保基础 CTA 在业务页面里表现一致。",
			},
		},
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
	parameters: {
		docs: {
			description: {
				story:
					"展示 asChild + link variant 组合，验证按钮退化为文本链接时的样式。",
			},
		},
	},
};
