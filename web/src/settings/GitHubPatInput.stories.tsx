import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useState } from "react";
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
					"GitHub PAT 专用输入组件：默认保持非 password 语义的文本输入，在隐藏态使用原生文本编辑配合视觉掩码，避免被当成站点登录密码框自动填充；隐藏态仍保持屏幕阅读器可操作，并通过额外提示说明当前是掩码编辑态。只有显式点亮后才切到明文。",
			},
		},
	},
	render: (args) => {
		const [value, setValue] = useState(String(args.value ?? ""));

		useEffect(() => {
			setValue(String(args.value ?? ""));
		}, [args.value]);

		return (
			<div className="w-[360px] space-y-2">
				<Label htmlFor={args.id}>GitHub PAT</Label>
				<GitHubPatInput
					{...args}
					value={value}
					onChange={(event) => {
						setValue(event.target.value);
						args.onChange?.(event);
					}}
				/>
			</div>
		);
	},
} satisfies Meta<typeof GitHubPatInput>;

export default meta;
type Story = StoryObj<typeof meta>;

export const MaskedByDefault: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const input = canvasElement.querySelector(
			"#storybook-github-pat",
		) as HTMLInputElement | null;
		if (!input) {
			throw new Error("expected storybook GitHub PAT input");
		}
		const hiddenHint = canvas.getByText(
			"当前内容已隐藏，仍可直接编辑。使用“显示 GitHub PAT”按钮可临时查看明文。",
		);
		await expect(input).toHaveAttribute("type", "text");
		await expect(input).toHaveAttribute("autocomplete", "off");
		await expect(input).toHaveAttribute(
			"aria-describedby",
			"storybook-github-pat-hidden-hint",
		);
		await expect(input).toHaveAttribute("data-1p-ignore", "true");
		await expect(input).toHaveAttribute("data-form-type", "other");
		await expect(input).toHaveAttribute("data-secret-visible", "false");
		await expect(input).toHaveAttribute("data-secret-mask-mode", "visual-mask");
		await expect(hiddenHint).toHaveAttribute(
			"id",
			"storybook-github-pat-hidden-hint",
		);

		await userEvent.click(
			canvas.getByRole("button", { name: "显示 GitHub PAT" }),
		);
		await expect(input).toHaveAttribute("type", "text");
		await expect(input).toHaveAttribute("data-secret-visible", "true");
		await expect(input).toHaveAttribute("data-secret-mask-mode", "plain-text");
		await expect(input).not.toHaveAttribute("aria-describedby");

		await userEvent.click(
			canvas.getByRole("button", { name: "隐藏 GitHub PAT" }),
		);
		await expect(input).toHaveAttribute("type", "text");
		await expect(input).toHaveAttribute("data-secret-visible", "false");
		await expect(input).toHaveAttribute("data-secret-mask-mode", "visual-mask");
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
				story: "可编辑态默认使用掩码字符隐藏；用户显式点亮后才切到明文。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const input = canvasElement.querySelector(
			"#storybook-github-pat",
		) as HTMLInputElement | null;
		if (!input) {
			throw new Error("expected editable storybook GitHub PAT input");
		}

		await userEvent.clear(input);
		await userEvent.type(input, "ghp_storybook_draft_next");
		await expect(input).toHaveValue("ghp_storybook_draft_next");
	},
};
