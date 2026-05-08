import type { Meta, StoryObj } from "@storybook/react-vite";
import type { ComponentProps } from "react";
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
	release("prod-current-release-a", {
		ts: "2026-05-08T03:54:06Z",
		repo_full_name: "example/current-cycle-a",
		title: "Current cycle release A",
		body: "- sanitized release after the daily boundary",
		html_url: "https://github.com/example/current-cycle-a/releases/tag/a",
	}),
	release("prod-current-release-b", {
		ts: "2026-05-08T02:39:33Z",
		repo_full_name: "example/current-cycle-b",
		title: "Current cycle release B",
		body: "- sanitized release after the daily boundary",
		html_url: "https://github.com/example/current-cycle-b/releases/tag/b",
	}),
	release("prod-covered-early-release", {
		ts: "2026-05-07T23:56:53Z",
		repo_full_name: "example/covered-early-release",
		title: "Covered early-morning release",
		body: "- sanitized release before the daily boundary",
		html_url: "https://github.com/example/covered-early-release/releases/tag/c",
	}),
];

const sanitizedProductionMay8Briefs: FeedGroupedListProps["briefs"] = [
	{
		id: "sanitized-prod-brief-2026-05-08",
		date: "2026-05-08",
		window_start: "2026-05-07T00:00:00+00:00",
		window_end: "2026-05-08T00:00:00+00:00",
		effective_time_zone: "Asia/Shanghai",
		effective_local_boundary: "08:00",
		release_count: 56,
		release_ids: ["prod-covered-early-release"],
		content_markdown:
			"## Sanitized Daily Brief\n\n- Window start: 2026-05-07 08:00\n- Source release count: 56\n- Includes the 2026-05-08 07:56 early-morning release from the production feed.\n",
		created_at: "2026-05-08T00:01:54.372532734+00:00",
	},
];

function FeedGroupedListPreview(props: {
	items?: FeedItem[];
	briefs?: FeedGroupedListProps["briefs"];
	now?: Date;
}) {
	const { items = earlyMorningReleases, briefs = [], now } = props;
	const selectedLaneByKey = Object.fromEntries(
		items.map((item) => [`${item.kind}:${item.id}`, "original"]),
	) as Record<string, FeedLane>;

	return (
		<div className="bg-background min-h-screen px-4 py-8 text-foreground sm:px-8">
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
					dailyBoundaryLocal="08:00"
					dailyBoundaryTimeZone="Asia/Shanghai"
					dailyBoundaryUtcOffsetMinutes={480}
					now={now ?? new Date("2026-05-07T12:00:00+08:00")}
					error={null}
					loadingInitial={false}
					loadingMore={false}
					hasMore={false}
					translationInFlightKeys={new Set()}
					smartInFlightKeys={new Set()}
					registerItemRef={() => () => {}}
					selectedLaneByKey={selectedLaneByKey}
					onLoadMore={() => {}}
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
		docs: {
			description: {
				component:
					"Feed 日组列表的稳定 canvas 场景，用于验证 08:00 日报边界前的 release 按日报周期开始日显示分组标题。",
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
		await expect(canvas.getByText("2026-05-05")).toBeVisible();
		await expect(canvas.queryByText("2026-05-06")).not.toBeInTheDocument();
		const may6Group = canvasElement.querySelector<HTMLElement>(
			'[data-feed-brief-date="2026-05-06"]',
		);
		expect(may6Group).toBeTruthy();
		if (!may6Group) {
			throw new Error("Expected 2026-05-06 raw fallback group");
		}
		await expect(within(may6Group).getByText("2026-05-05")).toBeVisible();
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
		await expect(canvas.getByText("Sanitized Daily Brief")).toBeVisible();
		await expect(
			canvas.queryByText("Covered early-morning release"),
		).not.toBeInTheDocument();

		const labels = Array.from(
			canvasElement.querySelectorAll<HTMLElement>("[data-feed-day-label]"),
		).map((element) => element.textContent?.replace(/\s+/g, " ").trim());
		expect(labels).toContain("2026-05-07 · 1 条 Release");
		expect(labels.some((label) => label?.startsWith("2026-05-08"))).toBe(false);
	},
};
