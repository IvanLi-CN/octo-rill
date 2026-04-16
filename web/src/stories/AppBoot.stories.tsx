import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import type { MeResponse } from "@/api";
import {
	AdminJobsStartupSkeleton,
	AdminUsersStartupSkeleton,
	AppBoot,
	DashboardStartupSkeleton,
} from "@/pages/AppBoot";

const mockMe: MeResponse = {
	user: {
		id: "user_warm_start",
		github_user_id: 42,
		login: "octo-admin",
		name: "Octo Admin",
		avatar_url: null,
		email: "admin@example.com",
		is_admin: true,
	},
	dashboard: {
		daily_boundary_local: "08:00",
		daily_boundary_time_zone: "Asia/Shanghai",
		daily_boundary_utc_offset_minutes: 480,
	},
	access_sync: {
		task_id: null,
		task_type: null,
		event_path: null,
		reason: "none",
	},
};

const meta = {
	title: "Pages/App Boot",
	component: AppBoot,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"站点启动统一走三层模型：冷启动时显示品牌初始化态；一小时内有热缓存时直接复用上次页面数据；已识别登录但没有热缓存时显示目标页的 layout skeleton。整个过程不允许提前露出登录 CTA，也不应暗示用户是否已登录。",
			},
		},
	},
} satisfies Meta<typeof AppBoot>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ColdInit: Story = {
	render: () => <AppBoot />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("应用正在完成初始化，请稍候片刻。"),
		).toBeVisible();
		await expect(
			canvas.queryByRole("link", { name: "连接到 GitHub" }),
		).not.toBeInTheDocument();
		expect(canvasElement.querySelector("[data-app-boot]")).not.toBeNull();
	},
};

export const DashboardWarmSkeleton: Story = {
	render: () => <DashboardStartupSkeleton me={mockMe} />,
	parameters: {
		docs: {
			description: {
				story:
					"已识别为登录态、但当前路由还没有可复用热缓存时，Dashboard 显示接近真实工作台壳层的 layout skeleton：保留品牌文案与导航结构，但 tabs / controls 仍改成中性占位，不再提前泄露具体导航文案。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { level: 1, name: "OctoRill" }),
		).toBeVisible();
		await expect(
			canvas.queryByRole("link", { name: "连接到 GitHub" }),
		).not.toBeInTheDocument();
		await expect(canvas.queryByText("octo-admin")).not.toBeInTheDocument();
		await expect(
			canvas.getByText("GitHub 动态 · 中文翻译 · 日报与 Inbox"),
		).toBeVisible();
		await expect(canvas.queryByText("通知")).not.toBeInTheDocument();
		expect(
			canvasElement.querySelector("[data-dashboard-boot-tab-strip]"),
		).not.toBeNull();
	},
};

export const AdminUsersWarmSkeleton: Story = {
	render: () => <AdminUsersStartupSkeleton me={mockMe} />,
};

export const AdminJobsWarmSkeleton: Story = {
	render: () => <AdminJobsStartupSkeleton me={mockMe} />,
};
