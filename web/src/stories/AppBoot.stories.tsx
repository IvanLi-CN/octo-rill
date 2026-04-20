import type { Meta, StoryObj } from "@storybook/react-vite";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, within } from "storybook/test";

import type { MeResponse } from "@/api";
import {
	AdminDashboardStartupSkeleton,
	AdminJobsStartupSkeleton,
	AdminUsersStartupSkeleton,
	AppBoot,
	DashboardStartupSkeleton,
	SettingsStartupSkeleton,
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

const APP_BOOT_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	appBootMobileFold: {
		name: "App boot mobile 390x844",
		styles: {
			height: "844px",
			width: "390px",
		},
		type: "mobile",
	},
} as const;

const meta = {
	title: "Pages/App Boot",
	component: AppBoot,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		viewport: {
			options: APP_BOOT_VIEWPORTS,
		},
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

export const LandingLazyPending: Story = {
	render: () => <AppBoot />,
	parameters: {
		docs: {
			description: {
				story:
					"匿名访问 `/` 且 auth 已完成、但 Landing route chunk 仍在拉取时的 fallback。这里继续复用中性的 AppBoot，而不是提前露出登录卡或白屏。",
			},
		},
	},
	play: ColdInit.play,
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

export const AdminDashboardWarmSkeleton: Story = {
	render: () => <AdminDashboardStartupSkeleton me={mockMe} />,
	parameters: {
		docs: {
			description: {
				story:
					"管理员访问 `/admin` 且 route chunk 尚未就绪时的稳定 fallback，保留 admin shell 轮廓但不提前渲染真实 dashboard 内容。",
			},
		},
	},
};

export const AdminUsersWarmSkeleton: Story = {
	render: () => <AdminUsersStartupSkeleton me={mockMe} />,
};

export const AdminJobsWarmSkeleton: Story = {
	render: () => <AdminJobsStartupSkeleton me={mockMe} />,
};

export const SettingsWarmSkeleton: Story = {
	render: () => <SettingsStartupSkeleton me={mockMe} />,
	parameters: {
		docs: {
			description: {
				story:
					"访问 `/settings` 且 chunk 仍在拉取时的轻量 shell-level skeleton，用来避免白屏，同时不回退到 Landing 登录页。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole("contentinfo")).toBeVisible();
		expect(
			canvasElement.querySelector("[data-app-shell-header]"),
		).not.toBeNull();
	},
};

export const DashboardWarmSkeletonMobile: Story = {
	name: "Dashboard Warm Skeleton / Mobile shell",
	render: () => <DashboardStartupSkeleton me={mockMe} />,
	globals: {
		viewport: {
			value: "appBootMobileFold",
			isRotated: false,
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"移动端审阅入口：warm skeleton 的页头要对齐真实 Dashboard mobile shell——副标题隐藏、品牌与右侧 actions 同行，第二行才是 control band。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const subtitle = canvasElement.querySelector(
			"[data-dashboard-boot-brand-subtitle]",
		) as HTMLElement | null;
		const mobileActions = canvasElement.querySelector(
			"[data-dashboard-boot-primary-actions-mobile]",
		) as HTMLElement | null;
		const desktopActions = canvasElement.querySelector(
			"[data-dashboard-boot-primary-actions]",
		) as HTMLElement | null;
		expect(subtitle).not.toBeVisible();
		expect(mobileActions).toBeVisible();
		expect(desktopActions).not.toBeVisible();
		expect(
			canvasElement.querySelector("[data-dashboard-boot-tab-strip]"),
		).not.toBeNull();
	},
};
