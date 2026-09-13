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
	dashboardRepository1171x620: {
		name: "Repository sidebar compact desktop 1171x620",
		styles: { height: "620px", width: "1171px" },
		type: "desktop",
	},
	dashboardRepository1171x560: {
		name: "Repository sidebar short desktop 1171x560",
		styles: { height: "560px", width: "1171px" },
		type: "desktop",
	},
} as const;

function RepositorySidebarPreview(props: {
	count: number;
	label: string;
	topInset?: number;
}) {
	const { panelRef, listRef, panelStyle, listStyle, shortList, compact } =
		useRepositoryPanelViewportHeight({ enabled: true, itemCount: props.count });

	return (
		<div
			className="bg-background min-h-screen p-8"
			data-visual-evidence-surface
		>
			<div
				className="flex min-h-[680px] justify-end"
				style={props.topInset ? { paddingTop: props.topInset } : undefined}
			>
				<div
					ref={panelRef}
					className={[
						"flex min-h-0 w-[360px] flex-col rounded-[28px] border border-border/70 bg-card/82 shadow-sm",
						compact ? "p-3" : "p-5",
					].join(" ")}
					style={panelStyle}
					data-visual-evidence-target
					data-dashboard-repository-panel="true"
					data-dashboard-repository-panel-state={
						shortList ? "viewport-fill" : "natural-capped"
					}
					data-dashboard-repository-panel-density={
						compact ? "compact" : "comfortable"
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
					<p
						className={
							compact
								? "mt-1 line-clamp-1 text-xs leading-4 text-muted-foreground"
								: "mt-3 text-sm leading-6 text-muted-foreground"
						}
					>
						查看你当前关注仓库的发布与相关动态。
					</p>
					<div
						className={[
							"grid grid-cols-2 gap-2",
							compact ? "mt-2" : "mt-4",
						].join(" ")}
					>
						<div
							className={[
								"rounded-xl bg-muted/38",
								compact ? "px-3 py-1.5" : "px-4 py-3",
							].join(" ")}
						>
							<p className="font-mono text-[11px] text-muted-foreground">
								关注仓库
							</p>
							<p
								className={
									compact
										? "text-base font-semibold"
										: "mt-1 text-lg font-semibold"
								}
							>
								{props.count}
							</p>
						</div>
						<div
							className={[
								"rounded-xl",
								compact ? "px-3 py-1.5" : "px-4 py-3",
							].join(" ")}
						>
							<p className="font-mono text-[11px] text-muted-foreground">
								关联仓库
							</p>
							<p
								className={
									compact
										? "text-base font-semibold"
										: "mt-1 text-lg font-semibold"
								}
							>
								{props.count + 1}
							</p>
						</div>
					</div>
					<div
						className={[
							"flex min-h-0 flex-1 flex-col overflow-hidden border-t border-border/60",
							compact ? "mt-2 pt-2" : "mt-4 pt-4",
						].join(" ")}
					>
						<div className="flex items-center justify-between gap-3">
							<p className="text-sm font-medium">{props.label}</p>
							<p className="font-mono text-[11px] text-muted-foreground">
								{props.count} 个
							</p>
						</div>
						<ul
							ref={listRef}
							className={[
								"min-h-0 flex-1 divide-y divide-border/50 overflow-y-auto",
								compact ? "mt-2" : "mt-3",
							].join(" ")}
							style={listStyle}
							data-dashboard-repository-list="true"
							data-dashboard-repository-list-label={props.label}
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
					"仓库侧栏以固定页脚为下界；列表可视区至少容纳两张项目卡，长列表只在列表区滚动。",
			},
		},
	},
	args: { count: 1, label: "关注仓库" },
} satisfies Meta<typeof RepositorySidebarPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

async function expectTwoCardViewportCapacity(canvasElement: HTMLElement) {
	const list = canvasElement.querySelector<HTMLElement>(
		'[data-dashboard-repository-list="true"]',
	);
	if (!list) throw new Error("Expected repository list");
	const firstItem = list.querySelector<HTMLElement>(
		"[data-dashboard-repository-item]",
	);
	const itemHeight = firstItem?.getBoundingClientRect().height ?? 76;
	await waitFor(() => {
		expect(list.clientHeight).toBeGreaterThanOrEqual(itemHeight * 2 - 2);
	});
}

async function expectTwoFullyVisibleRepositoryCards(
	canvasElement: HTMLElement,
) {
	const list = canvasElement.querySelector<HTMLElement>(
		'[data-dashboard-repository-list="true"]',
	);
	if (!list) throw new Error("Expected repository list");
	const [firstItem, secondItem] = Array.from(
		list.querySelectorAll<HTMLElement>("[data-dashboard-repository-item]"),
	).slice(0, 2);
	if (!firstItem || !secondItem) {
		throw new Error("Expected two repository items");
	}
	await waitFor(() => {
		const listRect = list.getBoundingClientRect();
		expect(firstItem.getBoundingClientRect().top).toBeGreaterThanOrEqual(
			listRect.top - 1,
		);
		expect(secondItem.getBoundingClientRect().bottom).toBeLessThanOrEqual(
			listRect.bottom + 1,
		);
	});
}

async function expectLongRepositoryListGeometry(
	canvasElement: HTMLElement,
	options?: { allowFooterOverlap?: boolean },
) {
	const panel = canvasElement.querySelector<HTMLElement>(
		'[data-dashboard-repository-panel="true"]',
	);
	const footer = canvasElement.querySelector<HTMLElement>(
		'[data-app-meta-footer="true"]',
	);
	const list = canvasElement.querySelector<HTMLElement>(
		'[data-dashboard-repository-list="true"]',
	);
	if (!panel || !footer || !list) {
		throw new Error("Expected panel, footer, and repository list");
	}
	await waitFor(() => {
		const gap =
			footer.getBoundingClientRect().top - panel.getBoundingClientRect().bottom;
		if (!options?.allowFooterOverlap) {
			expect(gap).toBeGreaterThanOrEqual(14);
			expect(gap).toBeLessThanOrEqual(18);
		}
	});
	expect(getComputedStyle(list).overflowY).toBe("auto");
	await waitFor(() => {
		expect(list.scrollHeight).toBeGreaterThan(list.clientHeight);
	});
	await expectTwoCardViewportCapacity(canvasElement);
	await expectTwoFullyVisibleRepositoryCards(canvasElement);
	expect(list.getBoundingClientRect().bottom).toBeLessThanOrEqual(
		panel.getBoundingClientRect().bottom + 1,
	);
}

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
		await expectTwoCardViewportCapacity(canvasElement);
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
		await expectTwoCardViewportCapacity(canvasElement);
	},
};

export const LongFollowing: Story = {
	args: { count: 18, label: "关注仓库" },
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
		).toHaveLength(18);
		await expectLongRepositoryListGeometry(canvasElement);
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
		await expectTwoCardViewportCapacity(canvasElement);
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

export const CompactLongFollowing: Story = {
	args: { count: 18, label: "关注仓库", topInset: 142 },
	parameters: {
		viewport: { defaultViewport: "dashboardRepository1171x620" },
		docs: {
			description: {
				story: "矮桌面会压缩非列表信息区，但至少完整显示两张项目卡。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		await waitFor(() => {
			expect(
				canvasElement.querySelector(
					'[data-dashboard-repository-panel-density="compact"]',
				),
			).not.toBeNull();
		});
		await expectLongRepositoryListGeometry(canvasElement);
	},
};

export const ShortViewportLongFollowing: Story = {
	args: { count: 18, label: "关注仓库", topInset: 142 },
	parameters: {
		viewport: { defaultViewport: "dashboardRepository1171x560" },
		docs: {
			description: {
				story:
					"当页脚上方不足以容纳两张项目卡时，面板扩展到两卡最小高度，列表仍在内部滚动。",
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
		await expectLongRepositoryListGeometry(canvasElement, {
			allowFooterOverlap: true,
		});
	},
};

export const ViewportLimitedLongFollowing: Story = {
	args: { count: 18, label: "关注仓库", topInset: 182 },
	parameters: {
		viewport: { defaultViewport: "dashboardRepository1171x620" },
		docs: {
			description: {
				story:
					"即使有 18 个项目，当可用列表视口只能容纳两张卡时也进入视口填充状态。",
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
		await expectLongRepositoryListGeometry(canvasElement, {
			allowFooterOverlap: true,
		});
	},
};

export const CompactLongAssociated: Story = {
	args: { count: 19, label: "关联仓库", topInset: 142 },
	parameters: {
		viewport: { defaultViewport: "dashboardRepository1171x620" },
	},
	play: async ({ canvasElement }) => {
		await expect(
			canvasElement.querySelector(
				'[data-dashboard-repository-list-label="关联仓库"]',
			),
		).not.toBeNull();
		await expectLongRepositoryListGeometry(canvasElement);
	},
};

export const LongPersonalRepositories: Story = {
	args: { count: 18, label: "个人仓库" },
	play: async ({ canvasElement }) => {
		await expect(
			canvasElement.querySelector(
				'[data-dashboard-repository-list-label="个人仓库"]',
			),
		).not.toBeNull();
		await expectLongRepositoryListGeometry(canvasElement);
	},
};

export const CompactLongPersonalRepositories: Story = {
	args: { count: 18, label: "个人仓库", topInset: 142 },
	parameters: {
		viewport: { defaultViewport: "dashboardRepository1171x620" },
	},
	play: async ({ canvasElement }) => {
		await expect(
			canvasElement.querySelector(
				'[data-dashboard-repository-list-label="个人仓库"]',
			),
		).not.toBeNull();
		await expectLongRepositoryListGeometry(canvasElement);
	},
};
