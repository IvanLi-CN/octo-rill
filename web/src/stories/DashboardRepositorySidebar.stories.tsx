import type { CSSProperties } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, waitFor } from "storybook/test";

import { useRepositoryPanelViewportHeight } from "@/dashboard/useRepositoryPanelViewportHeight";

const REPOSITORY_SIDEBAR_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	dashboardRepository1440x900: {
		name: "Repository sidebar desktop 1440x900",
		styles: { height: "900px", width: "1440px" },
		type: "desktop",
	},
	dashboardRepository1024x768: {
		name: "Repository sidebar desktop 1024x768",
		styles: { height: "768px", width: "1024px" },
		type: "desktop",
	},
} as const;

function RepositorySidebarPreview(props: { count: number; label: string }) {
	const { panelRef, listRef, panelStyle, shortList } =
		useRepositoryPanelViewportHeight({ enabled: true, itemCount: props.count });

	return (
		<div
			className="bg-background min-h-screen p-8"
			data-visual-evidence-surface
		>
			<div className="flex min-h-[680px] justify-end">
				<div
					ref={panelRef}
					className="flex min-h-0 w-[360px] flex-col rounded-[28px] border border-border/70 bg-card/82 p-5 shadow-sm"
					style={panelStyle}
					data-visual-evidence-target
					data-dashboard-repository-panel="true"
					data-dashboard-repository-panel-state={
						shortList ? "viewport-fill" : "natural-capped"
					}
				>
					<div className="flex items-start justify-between gap-3">
						<div>
							<p className="font-mono text-[11px] uppercase tracking-[0.22em] text-muted-foreground">
								关注
							</p>
							<h2 className="mt-1 text-xl font-semibold text-foreground">
								关注仓库
							</h2>
						</div>
						<span className="rounded-full border border-border/70 px-3 py-1 font-mono text-[11px] text-muted-foreground">
							{props.count} 个仓库
						</span>
					</div>
					<p className="mt-3 text-sm leading-6 text-muted-foreground">
						查看你当前关注仓库的发布与相关动态。
					</p>
					<div className="mt-4 grid grid-cols-2 gap-2">
						<div className="rounded-xl bg-muted/38 px-4 py-3">
							<p className="font-mono text-[11px] text-muted-foreground">
								关注仓库
							</p>
							<p className="mt-1 text-lg font-semibold">{props.count}</p>
						</div>
						<div className="rounded-xl px-4 py-3">
							<p className="font-mono text-[11px] text-muted-foreground">
								关联仓库
							</p>
							<p className="mt-1 text-lg font-semibold">{props.count + 1}</p>
						</div>
					</div>
					<div className="mt-4 flex min-h-0 flex-1 flex-col border-t border-border/60 pt-4">
						<div className="flex items-center justify-between gap-3">
							<p className="text-sm font-medium">{props.label}</p>
							<p className="font-mono text-[11px] text-muted-foreground">
								{props.count} 个
							</p>
						</div>
						<ul
							ref={listRef}
							className="mt-3 min-h-0 flex-1 divide-y divide-border/50 overflow-y-auto"
							data-dashboard-repository-list="true"
						>
							{Array.from({ length: props.count }, (_, index) => (
								<li
									key={index}
									className="flex min-h-[76px] items-center justify-between gap-3 py-3"
									data-dashboard-repository-item="true"
								>
									<span className="font-mono text-[12px]">
										octo-demo/repository-{index + 1}
									</span>
									<span className="rounded-full border border-border/65 px-2 py-0.5 text-[11px] text-muted-foreground">
										查看
									</span>
								</li>
							))}
						</ul>
					</div>
				</div>
			</div>
			<footer
				className="fixed inset-x-0 bottom-0 z-30 min-h-12 border-t bg-background/95"
				data-app-meta-footer="true"
				style={{ "--footer-test-color": "transparent" } as CSSProperties}
			/>
		</div>
	);
}

const meta = {
	title: "Pages/Dashboard Repository Sidebar",
	component: RepositorySidebarPreview,
	tags: ["autodocs", "dashboard-repository-sidebar-viewport"],
	parameters: {
		layout: "fullscreen",
		viewport: { options: REPOSITORY_SIDEBAR_VIEWPORTS },
		docs: {
			description: {
				component:
					"仓库侧栏在桌面端以固定页脚为下界；少于两张项目卡时填满可用高度，长列表只在列表区滚动。",
			},
		},
	},
	args: { count: 1, label: "关注仓库" },
} satisfies Meta<typeof RepositorySidebarPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ShortFollowing: Story = {
	play: async ({ canvasElement }) => {
		await waitFor(() => {
			expect(
				canvasElement.querySelector(
					'[data-dashboard-repository-panel-state="viewport-fill"]',
				),
			).not.toBeNull();
		});
		const panel = canvasElement.querySelector<HTMLElement>(
			'[data-dashboard-repository-panel="true"]',
		);
		const footer = canvasElement.querySelector<HTMLElement>(
			'[data-app-meta-footer="true"]',
		);
		if (!panel || !footer) throw new Error("Expected panel and footer");
		const gap =
			footer.getBoundingClientRect().top - panel.getBoundingClientRect().bottom;
		expect(gap).toBeGreaterThanOrEqual(14);
		expect(gap).toBeLessThanOrEqual(18);
	},
};

export const EmptyFollowing: Story = {
	args: { count: 0, label: "关注仓库" },
	parameters: {
		docs: {
			description: {
				story: "空列表使用每张项目卡 76px 的回退阈值，面板仍填充到页脚边界。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		await waitFor(() => {
			expect(
				canvasElement.querySelector(
					'[data-dashboard-repository-panel-state="viewport-fill"]',
				),
			).not.toBeNull();
		});
		await expect(
			canvasElement.querySelectorAll("[data-dashboard-repository-item]"),
		).toHaveLength(0);
	},
};

export const LongFollowing: Story = {
	args: { count: 12, label: "关注仓库" },
	play: async ({ canvasElement }) => {
		await waitFor(() => {
			expect(
				canvasElement.querySelector(
					'[data-dashboard-repository-panel-state="natural-capped"]',
				),
			).not.toBeNull();
		});
		await expect(
			canvasElement.querySelectorAll("[data-dashboard-repository-item]"),
		).toHaveLength(12);
		const list = canvasElement.querySelector<HTMLElement>(
			'[data-dashboard-repository-list="true"]',
		);
		if (!list) throw new Error("Expected repository list");
		expect(getComputedStyle(list).overflowY).toBe("auto");
		await waitFor(() => {
			expect(list.scrollHeight).toBeGreaterThan(list.clientHeight);
		});
		const panel = canvasElement.querySelector<HTMLElement>(
			'[data-dashboard-repository-panel="true"]',
		);
		const footer = canvasElement.querySelector<HTMLElement>(
			'[data-app-meta-footer="true"]',
		);
		if (!panel || !footer) throw new Error("Expected panel and footer");
		const gap =
			footer.getBoundingClientRect().top - panel.getBoundingClientRect().bottom;
		expect(gap).toBeGreaterThanOrEqual(14);
		expect(gap).toBeLessThanOrEqual(18);
	},
};

export const PersonalRepositories: Story = {
	args: { count: 1, label: "个人仓库" },
	parameters: {
		docs: {
			description: {
				story: "个人仓库列表使用同一短列表视口填充规则。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		await waitFor(() => {
			expect(
				canvasElement.querySelector(
					'[data-dashboard-repository-panel-state="viewport-fill"]',
				),
			).not.toBeNull();
		});
		await expect(
			canvasElement.querySelectorAll("[data-dashboard-repository-item]"),
		).toHaveLength(1);
		const panel = canvasElement.querySelector<HTMLElement>(
			'[data-dashboard-repository-panel="true"]',
		);
		const footer = canvasElement.querySelector<HTMLElement>(
			'[data-app-meta-footer="true"]',
		);
		if (!panel || !footer) throw new Error("Expected panel and footer");
		const gap =
			footer.getBoundingClientRect().top - panel.getBoundingClientRect().bottom;
		expect(gap).toBeGreaterThanOrEqual(14);
		expect(gap).toBeLessThanOrEqual(18);
	},
};
