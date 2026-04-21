import type * as React from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, userEvent, within } from "storybook/test";

import { DEFAULT_PAGE_LANE } from "@/feed/laneOptions";
import type { FeedLane } from "@/feed/types";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import {
	DashboardMobileControlBand,
	type DashboardTab,
} from "@/pages/DashboardControlBand";
import { DashboardHeader } from "@/pages/DashboardHeader";

function svgDataUrl(label: string, background: string, foreground = "#ffffff") {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240"><rect width="240" height="240" rx="120" fill="${background}"/><text x="120" y="132" font-family="Inter,Arial,sans-serif" font-size="44" font-weight="700" text-anchor="middle" fill="${foreground}">${label}</text></svg>`,
	)}`;
}

const STORYBOOK_AVATAR = svgDataUrl("SA", "#4f6a98");
const DASHBOARD_HEADER_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	dashboardHeaderMobile390: {
		name: "Dashboard header mobile 390x844",
		styles: {
			height: "844px",
			width: "390px",
		},
		type: "mobile",
	},
	dashboardHeaderTablet853: {
		name: "Dashboard header tablet 853x1280",
		styles: {
			height: "1280px",
			width: "853px",
		},
		type: "tablet",
	},
} as const;

function dispatchSyntheticTouchEvent(
	target: HTMLElement,
	type: "touchstart" | "touchmove",
	offsetX = 0,
	offsetY = 0,
) {
	const rect = target.getBoundingClientRect();
	const touchPoint = {
		clientX: rect.left + rect.width / 2 + offsetX,
		clientY: rect.top + rect.height / 2 + offsetY,
	};
	const event = new Event(type, {
		bubbles: true,
		cancelable: true,
	}) as Event & {
		touches: Array<typeof touchPoint>;
		targetTouches: Array<typeof touchPoint>;
		changedTouches: Array<typeof touchPoint>;
	};

	Object.defineProperty(event, "touches", { value: [touchPoint] });
	Object.defineProperty(event, "targetTouches", { value: [touchPoint] });
	Object.defineProperty(event, "changedTouches", { value: [touchPoint] });
	target.dispatchEvent(event);
}

function expectTabletInlineLayout(options: {
	mainRow: HTMLElement | null;
	leadingBlock: HTMLElement | null;
	trailingBlock: HTMLElement | null;
}) {
	const { mainRow, leadingBlock, trailingBlock } = options;
	expect(mainRow).not.toBeNull();
	expect(leadingBlock).not.toBeNull();
	expect(trailingBlock).not.toBeNull();
	if (!mainRow || !leadingBlock || !trailingBlock) {
		throw new Error("Expected main row, leading block, and trailing block");
	}

	const mainRect = mainRow.getBoundingClientRect();
	const leadingRect = leadingBlock.getBoundingClientRect();
	const trailingRect = trailingBlock.getBoundingClientRect();
	expect(mainRow.scrollWidth - mainRow.clientWidth).toBeLessThanOrEqual(1);
	expect(trailingRect.top - mainRect.top).toBeLessThanOrEqual(12);
	expect(trailingRect.left).toBeGreaterThanOrEqual(
		mainRect.left + mainRect.width * 0.5,
	);
	expect(trailingRect.top - leadingRect.top).toBeLessThanOrEqual(12);
}

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

function DashboardHeaderMobileShellPreview(
	args: React.ComponentProps<typeof DashboardHeader>,
) {
	const [tab, setTab] = useState<DashboardTab>("all");
	const [lane, setLane] = useState<FeedLane>(DEFAULT_PAGE_LANE);
	const showPageLaneSelector = tab === "all" || tab === "releases";
	const previewItems = Array.from({ length: 14 }, (_, index) => ({
		id: index + 1,
		title:
			index % 3 === 0
				? "发布摘要与反应区"
				: index % 3 === 1
					? "收件箱工作卡片"
					: "日报摘要卡片",
		lines: [
			"用于验证移动端顶部壳层在长滚动列表中的高度切换与吸顶稳定性。",
			"滚动离开顶部后页脚自动收起，回到顶部前保持隐藏，避免挤占可视区。",
			"内容继续上滑时应切到薄页头；向下回拉时恢复展开态顶部壳层。",
		],
	}));

	return (
		<AppShell
			header={
				<DashboardHeader
					{...args}
					mobileControlBand={
						<DashboardMobileControlBand
							tab={tab}
							onSelectTab={setTab}
							showPageLaneSelector={showPageLaneSelector}
							pageLane={lane}
							onSelectPageLane={setLane}
							layout="stacked"
						/>
					}
				/>
			}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<div className="space-y-3 sm:space-y-4">
				{previewItems.map((item) => {
					return (
						<section
							key={`mobile-header-proof-${item.id}`}
							className="rounded-3xl border bg-card p-5 shadow-sm sm:p-6"
						>
							<p className="text-muted-foreground text-xs font-mono">
								2026-04-12 · Proof {item.id}
							</p>
							<h2 className="mt-2 text-lg font-semibold">
								{item.title} #{item.id}
							</h2>
							<ul className="text-muted-foreground mt-3 space-y-2 text-sm leading-6">
								{item.lines.map((line) => (
									<li key={`${item.id}-${line}`} className="flex gap-2">
										<span aria-hidden="true">•</span>
										<span>{line}</span>
									</li>
								))}
							</ul>
							<div className="mt-4 flex min-h-16 flex-wrap gap-2 border-t border-border/50 pt-4">
								<span className="rounded-full border border-border/50 px-3 py-1 text-xs">
									展开态顶部壳层
								</span>
								<span className="rounded-full border border-border/50 px-3 py-1 text-xs">
									薄页头
								</span>
								<span className="rounded-full border border-border/50 px-3 py-1 text-xs">
									footer auto-hide
								</span>
							</div>
						</section>
					);
				})}
			</div>
		</AppShell>
	);
}

const meta = {
	title: "Pages/Dashboard Header",
	component: DashboardHeader,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		viewport: {
			options: DASHBOARD_HEADER_VIEWPORTS,
		},
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
		await expect(canvas.getByRole("link", { name: "设置" })).toBeVisible();
		await expect(canvas.getByRole("link", { name: "退出登录" })).toBeVisible();
	},
	parameters: {
		docs: {
			description: {
				story:
					"默认状态：品牌位先展示 OctoRill 与面向前台用户的核心能力概括；右侧只显示同步与头像入口，账号浮层内提供设置与退出登录等低频动作。",
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

export const EvidenceTabletInline: Story = {
	name: "Evidence / Tablet Inline Header",
	globals: {
		viewport: {
			value: "dashboardHeaderTablet853",
			isRotated: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("GitHub 动态 · 中文翻译 · 日报与 Inbox"),
		).toBeVisible();
		expectTabletInlineLayout({
			mainRow: canvasElement.querySelector<HTMLElement>(
				"[data-dashboard-header-main-row]",
			),
			leadingBlock: canvasElement.querySelector<HTMLElement>(
				"[data-dashboard-brand-block]",
			),
			trailingBlock: canvasElement.querySelector<HTMLElement>(
				"[data-dashboard-primary-actions]",
			),
		});
	},
	parameters: {
		docs: {
			description: {
				story:
					"平板 853x1280 证据入口：品牌块与 utility actions 必须回到同一主行，且右侧按钮组不再掉到品牌块下方。",
			},
		},
	},
};

export const EvidenceMobileShell: Story = {
	name: "Evidence / Mobile Shell",
	render: (args) => <DashboardHeaderMobileShellPreview {...args} />,
	globals: {
		viewport: {
			value: "dashboardHeaderMobile390",
			isRotated: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const appShellHeader = canvasElement.querySelector<HTMLElement>(
			"[data-app-shell-header-interacting]",
		);
		expect(appShellHeader).not.toBeNull();
		if (!appShellHeader) {
			throw new Error("Expected app shell interaction state to exist");
		}

		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);

		const laneMenuTrigger = canvas.getByRole("button", {
			name: "当前阅读模式：润色",
		}) as HTMLButtonElement;
		dispatchSyntheticTouchEvent(laneMenuTrigger, "touchstart");
		dispatchSyntheticTouchEvent(laneMenuTrigger, "touchmove", 10, 0);
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);
		dispatchSyntheticTouchEvent(laneMenuTrigger, "touchmove", 0, -10);
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);

		await userEvent.click(laneMenuTrigger);
		await expect(
			canvas.getByRole("menu", { name: "选择阅读模式" }),
		).toBeVisible();
		const laneMenu = canvas.getByRole("menu", { name: "选择阅读模式" });
		const translatedOption = within(laneMenu).getByRole("menuitemradio", {
			name: "翻译",
		}) as HTMLButtonElement;
		dispatchSyntheticTouchEvent(translatedOption, "touchstart");
		dispatchSyntheticTouchEvent(translatedOption, "touchmove", 10, 0);
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);
		dispatchSyntheticTouchEvent(translatedOption, "touchmove", 0, -10);
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);
		await userEvent.click(translatedOption);
		await expect(
			canvas.queryByRole("menu", { name: "选择阅读模式" }),
		).not.toBeInTheDocument();
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);

		await userEvent.click(canvas.getByRole("button", { name: "查看账号信息" }));
		await expect(
			canvas.getByRole("dialog", { name: "账号信息" }),
		).toBeVisible();
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);
	},
	parameters: {
		docs: {
			disable: true,
		},
	},
};

export const VerificationMobileShellDrag: Story = {
	name: "Verification / Mobile shell drag",
	render: (args) => <DashboardHeaderMobileShellPreview {...args} />,
	globals: {
		viewport: {
			value: "dashboardHeaderMobile390",
			isRotated: false,
		},
	},
	parameters: {
		docs: {
			disable: true,
		},
	},
};
