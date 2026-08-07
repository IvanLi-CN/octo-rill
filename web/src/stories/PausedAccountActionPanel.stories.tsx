import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, within } from "storybook/test";

import { PausedAccountActionPanel } from "@/account/PausedAccountActionPanel";

const meta = {
	title: "Account/PausedAccountActionPanel",
	component: PausedAccountActionPanel,
	tags: ["autodocs"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"暂停账号的恢复状态面板。恢复动作先确认账号状态，再以任务流展示访问同步结果；入队失败保持可重试。",
			},
		},
	},
	args: {
		login: "octo-member",
		error: null,
		onResume: fn(),
		onHome: fn(),
		onLogout: fn(),
	},
} satisfies Meta<typeof PausedAccountActionPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Idle: Story = {
	globals: { theme: "light" },
	args: { state: "idle" },
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "账号已暂停" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "恢复账号" }),
		).toBeEnabled();
		await expect(canvas.getByText("等待恢复")).toBeVisible();
	},
};

export const IdleDark: Story = {
	globals: { theme: "dark" },
	args: { state: "idle" },
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "账号已暂停" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "恢复账号" }),
		).toBeEnabled();
	},
};

export const Syncing: Story = {
	args: { state: "syncing" },
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("访问同步进行中")).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "恢复账号" }),
		).toBeDisabled();
	},
};

export const EnqueueFailure: Story = {
	args: {
		state: "enqueue_failed",
		error: "账号已恢复，但访问同步没有入队成功，请重试。",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("同步等待重试")).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "重试同步" }),
		).toBeEnabled();
		await expect(
			canvas.getByText("账号已恢复，但访问同步没有入队成功，请重试。"),
		).toBeVisible();
	},
};

export const Complete: Story = {
	args: { state: "succeeded" },
	play: async ({ canvasElement }) => {
		await expect(
			within(canvasElement).getByText("访问同步已完成"),
		).toBeVisible();
	},
};
