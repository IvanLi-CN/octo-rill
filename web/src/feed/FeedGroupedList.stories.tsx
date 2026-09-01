import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState, type ComponentProps } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, within } from "storybook/test";

import { FeedGroupedList } from "@/feed/FeedGroupedList";
import type { FeedItem, FeedLane, ReleaseFeedItem } from "@/feed/types";
import type { RepoVisual } from "@/lib/repoVisual";

type FeedGroupedListProps = ComponentProps<typeof FeedGroupedList>;

const repoVisual: RepoVisual = {
	owner_avatar_url: "https://github.com/IvanLi-CN.png?size=96",
	open_graph_image_url: null,
	uses_custom_open_graph_image: false,
};

function release(
	id: string,
	overrides: Partial<ReleaseFeedItem>,
): ReleaseFeedItem {
	return {
		kind: "release",
		ts: "2026-05-05T21:55:09Z",
		id,
		repo_full_name: "IvanLi-CN/dockrev",
		repo_visual: repoVisual,
		title: "0.44.4",
		body: "- 修复网页端：澄清更新确认详情",
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `https://github.com/IvanLi-CN/dockrev/releases/tag/${id}`,
		unread: null,
		actor: null,
		translated: {
			lang: "zh-CN",
			status: "disabled",
			title: null,
			summary: null,
		},
		smart: {
			lang: "zh-CN",
			status: "disabled",
			title: null,
			summary: null,
		},
		reactions: {
			counts: {
				plus1: 0,
				laugh: 0,
				heart: 0,
				hooray: 0,
				rocket: 0,
				eyes: 0,
			},
			viewer: {
				plus1: false,
				laugh: false,
				heart: false,
				hooray: false,
				rocket: false,
				eyes: false,
			},
			status: "ready",
		},
		...overrides,
	};
}

const earlyMorningReleases: FeedItem[] = [
	release("319182286", {
		ts: "2026-05-07T20:48:56Z",
		repo_full_name: "IvanLi-CN/tavreg-hikari",
		title: "v0.17.2",
		body: "- 通过 PR #68 修复并强化 Tavily 和 Grok 工作流恢复能力",
		html_url: "https://github.com/IvanLi-CN/tavreg-hikari/releases/tag/v0.17.2",
	}),
	release("318101716", {
		ts: "2026-05-05T23:24:41Z",
		repo_full_name: "amir20/dozzle",
		title: "Dozzle v10.5.2",
		body: "- 新增 Cloud Search 功能\n- 修复 Docker ContainerEvents 事件通道问题",
		html_url: "https://github.com/amir20/dozzle/releases/tag/v10.5.2",
	}),
	release("318080539", {
		ts: "2026-05-05T21:55:09Z",
		repo_full_name: "IvanLi-CN/dockrev",
		title: "0.44.4",
		body: "- 修复网页端：澄清更新确认详情",
		html_url: "https://github.com/IvanLi-CN/dockrev/releases/tag/0.44.4",
	}),
	release("318079870", {
		ts: "2026-05-05T21:50:33Z",
		repo_full_name: "IvanLi-CN/openwrt-builder",
		title: "CI 工作流更新：支持从 Workflow 发布固件",
		body: "- 添加 publish_release 参数\n- 支持发布 GitHub Release",
		html_url:
			"https://github.com/IvanLi-CN/openwrt-builder/releases/tag/ci-release-workflow",
	}),
];

const sanitizedProductionMay8Items: FeedItem[] = [
	release("prod-history-release", {
		ts: "2026-05-07T10:18:53Z",
		repo_full_name: "example/history-cycle-release",
		title: "History cycle release",
		body: "- sanitized release from the previous natural day",
		html_url:
			"https://github.com/example/history-cycle-release/releases/tag/history",
	}),
	release("prod-current-release-a", {
		ts: "2026-05-08T03:54:06Z",
		repo_full_name: "example/current-cycle-a",
		title: "Current cycle release A",
		body: "- sanitized release from the current natural day",
		html_url: "https://github.com/example/current-cycle-a/releases/tag/a",
	}),
	release("prod-current-release-b", {
		ts: "2026-05-08T02:39:33Z",
		repo_full_name: "example/current-cycle-b",
		title: "Current cycle release B",
		body: "- sanitized release from the current natural day",
		html_url: "https://github.com/example/current-cycle-b/releases/tag/b",
	}),
	release("prod-covered-early-release", {
		ts: "2026-05-07T23:56:53Z",
		repo_full_name: "example/covered-early-release",
		title: "Current-day early-morning release",
		body: "- sanitized release before the auto schedule time but still inside the same natural day",
		html_url: "https://github.com/example/covered-early-release/releases/tag/c",
	}),
];

const sanitizedProductionMay8Briefs: FeedGroupedListProps["briefs"] = [
	{
		id: "sanitized-prod-brief-2026-05-07",
		date: "2026-05-07",
		window_start: "2026-05-06T16:00:00+00:00",
		window_end: "2026-05-07T16:00:00+00:00",
		effective_time_zone: "Asia/Shanghai",
		effective_local_boundary: "00:00",
		release_count: 1,
		release_ids: ["prod-history-release"],
		content_markdown:
			"## Sanitized Daily Brief\n\n- Window start: 2026-05-07 00:00\n- Source release count: 1\n- The 2026-05-08 07:56 release now stays in the 2026-05-08 natural-day group instead of being folded into yesterday.\n",
		created_at: "2026-05-07T16:01:54.372532734+00:00",
	},
];

function FeedGroupedListPreview(props: {
	items?: FeedItem[];
	briefs?: FeedGroupedListProps["briefs"];
	now?: Date;
	onGenerateBriefForDate?: FeedGroupedListProps["onGenerateBriefForDate"];
	initialBriefErrorSummariesByDate?: Record<string, string>;
}) {
	const {
		items = earlyMorningReleases,
		briefs = [],
		now,
		onGenerateBriefForDate,
		initialBriefErrorSummariesByDate,
	} = props;
	const selectedLaneByKey = Object.fromEntries(
		items.map((item) => [`${item.kind}:${item.id}`, "original"]),
	) as Record<string, FeedLane>;

	return (
		<div className="bg-background px-4 py-8 text-foreground sm:px-8">
			<div className="mx-auto max-w-4xl">
				<FeedGroupedList
					mode="all"
					items={items}
					currentViewer={{
						login: "IvanLi-CN",
						avatar_url: "https://github.com/IvanLi-CN.png?size=96",
						html_url: "https://github.com/IvanLi-CN",
					}}
					briefs={briefs}
					dailyBoundaryLocal="00:00"
					dailyBoundaryTimeZone="Asia/Shanghai"
					dailyBoundaryUtcOffsetMinutes={480}
					now={now ?? new Date("2026-05-08T12:00:00+08:00")}
					error={null}
					loadingInitial={false}
					loadingMore={false}
					hasMore={false}
					translationInFlightKeys={new Set()}
					translationAutoRetryingKeys={new Set()}
					smartInFlightKeys={new Set()}
					smartAutoRetryingKeys={new Set()}
					registerItemRef={() => () => {}}
					selectedLaneByKey={selectedLaneByKey}
					onLoadMore={() => {}}
					onRetryInitial={() => {}}
					onGenerateBriefForDate={onGenerateBriefForDate}
					initialBriefErrorSummariesByDate={initialBriefErrorSummariesByDate}
					onSelectLane={() => {}}
					onTranslateNow={() => {}}
					onSmartNow={() => {}}
					reactionBusyKeys={new Set()}
					reactionErrorByKey={{}}
					onToggleReaction={() => {}}
				/>
			</div>
		</div>
	);
}

const foldedHistoryBriefs: FeedGroupedListProps["briefs"] = [
	{
		id: "brief-folded-history-2026-05-07",
		date: "2026-05-07",
		window_start: "2026-05-06T16:00:00Z",
		window_end: "2026-05-07T16:00:00Z",
		effective_time_zone: "Asia/Shanghai",
		effective_local_boundary: "00:00",
		release_count: 2,
		release_ids: ["folded-history-40", "folded-history-41"],
		content_markdown: "## 历史日报\n\n日报已覆盖这一天的 Release。",
		created_at: "2026-05-07T16:05:00Z",
	},
];

const FEED_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	feedMobile393: {
		name: "Feed mobile 393x852",
		styles: {
			height: "852px",
			width: "393px",
		},
		type: "mobile",
	},
} as const;

function FeedGroupedListContinuationPreview() {
	const currentItem = release("current-history-39", {
		ts: "2026-05-08T10:00:00Z",
		title: "Current history release 39",
	});
	const initialItem = release("folded-history-40", {
		ts: "2026-05-07T10:27:01Z",
		title: "Folded history release 40",
	});
	const foldedItem = release("folded-history-41", {
		ts: "2026-05-07T09:12:00Z",
		title: "Folded history release 41",
	});
	const visibleItem = release("visible-history-43", {
		ts: "2026-05-06T09:12:00Z",
		title: "Visible history release 43",
	});
	const [items, setItems] = useState<FeedItem[]>([currentItem, initialItem]);
	const [nextCursor, setNextCursor] = useState<string | null>("page-2");
	const [loadingMore, setLoadingMore] = useState(false);
	const selectedLaneByKey = Object.fromEntries(
		items.map((item) => [`${item.kind}:${item.id}`, "original"]),
	) as Record<string, FeedLane>;

	const loadMore = () => {
		if (loadingMore || !nextCursor) return;
		setLoadingMore(true);
		window.setTimeout(() => {
			if (nextCursor === "page-2") {
				setItems((current) => [...current, foldedItem]);
				setNextCursor("page-3");
			} else {
				setItems((current) => [...current, visibleItem]);
				setNextCursor(null);
			}
			setLoadingMore(false);
		}, 100);
	};

	return (
		<div className="bg-slate-800 px-4 py-8 text-foreground sm:px-8">
			<div className="mx-auto max-w-4xl rounded-2xl bg-background p-4">
				<FeedGroupedList
					mode="all"
					items={items}
					currentViewer={{
						login: "IvanLi-CN",
						avatar_url: "https://github.com/IvanLi-CN.png?size=96",
						html_url: "https://github.com/IvanLi-CN",
					}}
					briefs={foldedHistoryBriefs}
					dailyBoundaryLocal="00:00"
					dailyBoundaryTimeZone="Asia/Shanghai"
					dailyBoundaryUtcOffsetMinutes={480}
					now={new Date("2026-05-08T12:00:00+08:00")}
					error={null}
					loadingInitial={false}
					loadingMore={loadingMore}
					hasMore={Boolean(nextCursor)}
					translationInFlightKeys={new Set()}
					translationAutoRetryingKeys={new Set()}
					smartInFlightKeys={new Set()}
					smartAutoRetryingKeys={new Set()}
					registerItemRef={() => () => {}}
					selectedLaneByKey={selectedLaneByKey}
					onLoadMore={loadMore}
					onRetryInitial={() => {}}
					onSelectLane={() => {}}
					onTranslateNow={() => {}}
					onSmartNow={() => {}}
					reactionBusyKeys={new Set()}
					reactionErrorByKey={{}}
					onToggleReaction={() => {}}
				/>
			</div>
		</div>
	);
}

const meta = {
	title: "Feed/FeedGroupedList",
	component: FeedGroupedListPreview,
	parameters: {
		layout: "fullscreen",
		viewport: {
			options: FEED_VIEWPORTS,
		},
		docs: {
			description: {
				component:
					"Feed 日组列表的稳定 canvas 场景，用于验证自然日分组、历史日报折叠，以及自动出报时间不再影响同一天的 raw release 归属。",
			},
		},
	},
} satisfies Meta<typeof FeedGroupedListPreview>;

export default meta;

type Story = StoryObj<typeof meta>;

export const EarlyMorningRawFallbackDateLabel: Story = {
	name: "Early Morning Raw Fallback Date Label",
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("2026-05-06")).toBeVisible();
		await expect(canvas.queryByText("2026-05-05")).not.toBeInTheDocument();
		const may6Group = canvasElement.querySelector<HTMLElement>(
			'[data-feed-brief-date="2026-05-06"]',
		);
		expect(may6Group).toBeTruthy();
		if (!may6Group) {
			throw new Error("Expected 2026-05-06 raw fallback group");
		}
		await expect(within(may6Group).getByText("2026-05-06")).toBeVisible();
		await expect(canvas.getByText("Dozzle v10.5.2")).toBeVisible();
		await expect(canvas.getByText("0.44.4")).toBeVisible();
	},
};

export const SanitizedProductionMay8Boundary: Story = {
	name: "Sanitized Production May 8 Boundary",
	render: () => (
		<FeedGroupedListPreview
			items={sanitizedProductionMay8Items}
			briefs={sanitizedProductionMay8Briefs}
			now={new Date("2026-05-08T16:30:00+08:00")}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("Current cycle release A")).toBeVisible();
		await expect(canvas.getByText("Current cycle release B")).toBeVisible();
		await expect(
			canvas.getByText("Current-day early-morning release"),
		).toBeVisible();
		await expect(canvas.getByText("Sanitized Daily Brief")).toBeVisible();
		await expect(
			canvas.queryByText("History cycle release"),
		).not.toBeInTheDocument();

		const labels = Array.from(
			canvasElement.querySelectorAll<HTMLElement>("[data-feed-day-label]"),
		).map((element) => element.textContent?.replace(/\s+/g, " ").trim());
		expect(labels).toContain("2026-05-07 · 1 条 Release");
		expect(labels).toContain("2026-05-08 · 3 条 Release");
	},
};

export const FoldedHistoryPaginationContinuation: Story = {
	name: "Folded History Pagination Continuation",
	render: () => <FeedGroupedListContinuationPreview />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const continueButton = canvas.getByRole("button", {
			name: "继续加载历史动态",
		});
		await expect(continueButton).toBeVisible();
		await continueButton.click();
		await expect(canvas.getByText("Visible history release 43")).toBeVisible();
		await expect(
			canvas.queryByRole("button", { name: "继续加载历史动态" }),
		).not.toBeInTheDocument();
	},
};

export const FoldedHistoryPaginationPaused: Story = {
	name: "Folded History Pagination Paused",
	render: () => <FeedGroupedListContinuationPreview />,
};

export const FoldedHistoryPaginationListView: Story = {
	name: "Folded History Pagination List View",
	render: () => <FeedGroupedListContinuationPreview />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const historicalGroup = canvasElement.querySelector<HTMLElement>(
			'[data-feed-brief-date="2026-05-07"]',
		);
		expect(historicalGroup).toBeTruthy();
		if (!historicalGroup) {
			throw new Error("Expected folded history group");
		}
		await expect(
			within(historicalGroup).getByRole("button", { name: "列表" }),
		).toBeVisible();
		await within(historicalGroup).getByRole("button", { name: "列表" }).click();
		await expect(canvas.getByText("Folded history release 40")).toBeVisible();
		await expect(canvas.getByText("Folded history release 41")).toBeVisible();
		await expect(
			within(historicalGroup).getByRole("button", { name: "日报" }),
		).toBeVisible();
	},
};

export const FoldedHistoryPaginationPausedMobile: Story = {
	name: "Folded History Pagination Paused Mobile",
	globals: {
		viewport: { value: "feedMobile393", isRotated: false },
	},
	render: () => <FeedGroupedListContinuationPreview />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const continueButton = canvas.getByRole("button", {
			name: "继续加载历史动态",
		});
		await expect(continueButton).toBeVisible();
		const buttonBounds = continueButton.getBoundingClientRect();
		expect(buttonBounds.left).toBeGreaterThanOrEqual(0);
		expect(buttonBounds.right).toBeLessThanOrEqual(
			canvasElement.ownerDocument.documentElement.clientWidth,
		);
	},
};

export const HistoricalBriefInlineErrorVisible: Story = {
	name: "Historical Brief Inline Error Visible",
	render: () => (
		<FeedGroupedListPreview
			items={earlyMorningReleases.slice(-1)}
			briefs={[]}
			now={new Date("2026-05-07T12:00:00+08:00")}
			initialBriefErrorSummariesByDate={{
				"2026-05-06": "后台没有返回日报内容，请稍后重试。",
			}}
			onGenerateBriefForDate={async () => {}}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("日报生成失败")).toBeVisible();
		await expect(
			canvas.getByText("后台没有返回日报内容，请稍后重试。"),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "重试日报" }),
		).toBeVisible();
		await expect(
			canvas.queryByText("这份日报是降级摘要"),
		).not.toBeInTheDocument();
	},
};
