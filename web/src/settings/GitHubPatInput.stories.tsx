import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { Label } from "@/components/ui/label";
import { GitHubPatInput } from "@/settings/GitHubPatInput";

const meta = {
	title: "Settings/GitHubPatInput",
	component: GitHubPatInput,
	tags: ["autodocs"],
	args: {
		id: "storybook-github-pat",
		value: "ghp_storybook_valid_token",
		readOnly: true,
		placeholder: "粘贴 classic PAT",
		autoCapitalize: "none",
		autoCorrect: "off",
		spellCheck: false,
		inputClassName: "h-10 font-mono text-sm",
	},
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"GitHub PAT 专用输入组件：默认保持原生 password 保护，保留 `new-password` 提示，并叠加密码管理器忽略属性；只有显式点亮后才切到明文。",
			},
		},
	},
	render: (args) => (
		<div className="w-[360px] space-y-2">
			<Label htmlFor={args.id}>GitHub PAT</Label>
			<GitHubPatInput {...args} />
		</div>
	),
} satisfies Meta<typeof GitHubPatInput>;

export default meta;
type Story = StoryObj<typeof meta>;

export const MaskedByDefault: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const input = canvas.getByLabelText("GitHub PAT", { selector: "input" });
		await expect(input).toHaveAttribute("type", "password");
		await expect(input).toHaveAttribute("autocomplete", "new-password");
		await expect(input).toHaveAttribute("data-1p-ignore", "true");
		await expect(input).toHaveAttribute("data-form-type", "other");
		await expect(input).toHaveAttribute("data-secret-visible", "false");

		await userEvent.click(
			canvas.getByRole("button", { name: "显示 GitHub PAT" }),
		);
		await expect(input).toHaveAttribute("type", "text");
		await expect(input).toHaveAttribute("data-secret-visible", "true");

		await userEvent.click(
			canvas.getByRole("button", { name: "隐藏 GitHub PAT" }),
		);
		await expect(input).toHaveAttribute("type", "password");
		await expect(input).toHaveAttribute("data-secret-visible", "false");
	},
};

export const Editable: Story = {
	args: {
		readOnly: false,
		value: "ghp_storybook_draft",
	},
	parameters: {
		docs: {
			description: {
				story:
					"可编辑态默认仍使用原生 password 保护；用户显式点亮后才切到明文。",
			},
		},
	},
};
