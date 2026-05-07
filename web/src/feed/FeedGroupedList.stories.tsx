import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import { FeedGroupedList } from "@/feed/FeedGroupedList";
import type { FeedItem, FeedLane, ReleaseFeedItem } from "@/feed/types";
import type { RepoVisual } from "@/lib/repoVisual";

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

function FeedGroupedListPreview() {
	const selectedLaneByKey = Object.fromEntries(
		earlyMorningReleases.map((item) => [`${item.kind}:${item.id}`, "original"]),
	) as Record<string, FeedLane>;

	return (
		<div className="bg-background min-h-screen px-4 py-8 text-foreground sm:px-8">
			<div className="mx-auto max-w-4xl">
				<FeedGroupedList
					mode="all"
					items={earlyMorningReleases}
					currentViewer={{
						login: "IvanLi-CN",
						avatar_url: "https://github.com/IvanLi-CN.png?size=96",
						html_url: "https://github.com/IvanLi-CN",
					}}
					briefs={[]}
					dailyBoundaryLocal="08:00"
					dailyBoundaryTimeZone="Asia/Shanghai"
					dailyBoundaryUtcOffsetMinutes={480}
					now={new Date("2026-05-07T12:00:00+08:00")}
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
					"Feed 日组列表的稳定 canvas 场景，用于验证 08:00 日报边界前的本地次日 release 分组标题。",
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
