import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { DashboardHeader } from "@/pages/DashboardHeader";

function svgDataUrl(label: string, background: string, foreground = "#ffffff") {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240"><rect width="240" height="240" rx="120" fill="${background}"/><text x="120" y="132" font-family="Inter,Arial,sans-serif" font-size="44" font-weight="700" text-anchor="middle" fill="${foreground}">${label}</text></svg>`,
	)}`;
}

const STORYBOOK_AVATAR = svgDataUrl("SA", "#4f6a98");

function DashboardHeaderGallery() {
	return (
		<div className="bg-background grid gap-4 p-6">
			<section className="rounded-3xl border bg-card p-5 shadow-sm">
				<DashboardHeader
					avatarUrl={STORYBOOK_AVATAR}
					isAdmin
					login="storybook-admin"
					name="Storybook Admin"
					onSyncAll={() => {}}
					logoutHref="#"
				/>
			</section>

			<section className="rounded-3xl border bg-card p-5 shadow-sm">
				<DashboardHeader
					aiDisabledHint
					avatarUrl={STORYBOOK_AVATAR}
					isAdmin
					login="storybook-admin"
					name="Storybook Admin"
					onSyncAll={() => {}}
					logoutHref="#"
				/>
			</section>

			<section className="max-w-[440px] rounded-3xl border bg-card p-5 shadow-sm">
				<DashboardHeader
					avatarUrl={STORYBOOK_AVATAR}
					isAdmin
					login="storybook-admin"
					name="Storybook Admin"
					onSyncAll={() => {}}
					logoutHref="#"
				/>
			</section>
		</div>
	);
}

const meta = {
	title: "Pages/Dashboard Header",
	component: DashboardHeader,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"Dashboard 页头采用品牌优先双层布局：左侧只保留品牌与产品定位，右侧收敛为同步按钮与头像入口；头像悬浮或点击后显示账号详情，并把低频的退出登录动作收进浮层。",
			},
		},
	},
	args: {
		avatarUrl: STORYBOOK_AVATAR,
		email: "storybook-admin@example.com",
		login: "storybook-admin",
		name: "Storybook Admin",
		isAdmin: true,
		aiDisabledHint: false,
		busy: false,
		syncingAll: false,
		onSyncAll: () => {},
		logoutHref: "#",
	},
} satisfies Meta<typeof DashboardHeader>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		expect(
			canvasElement.querySelector("[data-dashboard-brand-heading]"),
		).not.toBeNull();
		await expect(
			canvas.getByRole("heading", { name: "OctoRill" }),
		).toBeVisible();
		await expect(
			canvas.getByText("GitHub 动态 · 中文翻译 · 日报与 Inbox"),
		).toBeVisible();
		await expect(canvas.getByRole("group", { name: "主题模式" })).toBeVisible();
		await expect(canvas.queryByText(/Logged in as/)).not.toBeInTheDocument();
		await expect(canvas.getByText(/Loaded\s+\d+/)).not.toBeInTheDocument();
		await expect(canvas.getByRole("button", { name: "同步" })).toBeVisible();
		const profileButton = canvas.getByRole("button", { name: "查看账号信息" });
		await expect(profileButton).toBeVisible();
		await userEvent.click(profileButton);
		const userCard = canvas.getByRole("dialog", { name: "账号信息" });
		await expect(canvas.getByText("Storybook Admin")).toBeVisible();
		await expect(canvas.getByText("@storybook-admin")).toBeVisible();
		await expect(canvas.getByText("storybook-admin@example.com")).toBeVisible();
		await expect(within(userCard).getByLabelText("管理员")).toBeVisible();
		await expect(canvas.getByRole("link", { name: "退出登录" })).toBeVisible();
	},
	parameters: {
		docs: {
			description: {
				story:
					"默认状态：品牌位先展示 OctoRill 与面向前台用户的核心能力概括；右侧只显示同步与头像入口，账号详情与退出登录收进头像浮层。",
			},
		},
	},
};

export const Syncing: Story = {
	args: {
		busy: true,
		syncingAll: true,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const syncButton = canvas.getByRole("button", { name: "同步" });
		await expect(syncButton).toBeDisabled();
		const icon = syncButton.querySelector("svg");
		expect(icon?.classList.contains("animate-spin")).toBe(true);
	},
	parameters: {
		docs: {
			description: {
				story:
					"同步中状态：右侧主按钮禁用并旋转 icon，头像入口保持稳定，退出登录继续收在浮层内。",
			},
		},
	},
};

export const StateGallery: Story = {
	name: "State gallery",
	render: () => <DashboardHeaderGallery />,
	parameters: {
		docs: {
			description: {
				story:
					"把默认、AI 未配置与紧凑宽度三种状态放进同一审阅面，便于确认品牌位独立、右侧账号入口收敛，以及窄宽度下同步/头像的排列。",
			},
		},
	},
};
