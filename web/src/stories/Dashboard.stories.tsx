import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";

import { FeedList } from "@/feed/FeedList";
import type { FeedItem } from "@/feed/types";
import { AppShell } from "@/layout/AppShell";
import { BriefListCard } from "@/sidebar/BriefListCard";
import {
	InboxQuickList,
	type NotificationItem,
} from "@/sidebar/InboxQuickList";
import { type BriefItem, ReleaseDailyCard } from "@/sidebar/ReleaseDailyCard";

function makeMockFeed(): FeedItem[] {
	return [
		{
			kind: "release",
			ts: "2026-02-21T08:05:00Z",
			id: "10001",
			repo_full_name: "acme/rocket",
			title: "v1.8.0",
			subtitle: null,
			reason: null,
			subject_type: null,
			html_url: "https://github.com/acme/rocket/releases/tag/v1.8.0",
			unread: null,
			translated: {
				lang: "zh-CN",
				status: "ready",
				title: "v1.8.0（稳定版）",
				summary:
					"- 修复发布流程中的签名问题\n- 提升启动速度\n- 建议升级并重新构建镜像",
			},
		},
		{
			kind: "release",
			ts: "2026-02-21T06:20:00Z",
			id: "10000",
			repo_full_name: "acme/rocket",
			title: "v1.7.3",
			subtitle: null,
			reason: null,
			subject_type: null,
			html_url: "https://github.com/acme/rocket/releases/tag/v1.7.3",
			unread: null,
			translated: {
				lang: "zh-CN",
				status: "disabled",
				title: null,
				summary: null,
			},
		},
	];
}

const mockBriefs: BriefItem[] = [
	{
		date: "2026-02-21",
		window_start: "2026-02-20T08:00:00+08:00",
		window_end: "2026-02-21T08:00:00+08:00",
		content_markdown:
			"## 昨日更新（Releases）\n\n- acme/rocket: v1.8.0\n\n## 建议跟进（Next actions）\n\n- 升级并验证 CI\n",
		created_at: "2026-02-21T08:00:03Z",
	},
	{
		date: "2026-02-20",
		window_start: "2026-02-19T08:00:00+08:00",
		window_end: "2026-02-20T08:00:00+08:00",
		content_markdown:
			"## 昨日更新（Releases）\n\n- acme/rocket: v1.7.3\n\n## 建议跟进（Next actions）\n\n- 观察回归问题\n",
		created_at: "2026-02-20T08:00:04Z",
	},
];

const mockNotifs: NotificationItem[] = [
	{
		thread_id: "90001",
		repo_full_name: "acme/rocket",
		subject_title: "Build failed on main",
		subject_type: "CheckSuite",
		reason: "ci_activity",
		updated_at: "2026-02-21T07:40:00Z",
		unread: 1,
		html_url: null,
	},
	{
		thread_id: "90000",
		repo_full_name: "acme/rocket",
		subject_title: "PR: bump deps",
		subject_type: "PullRequest",
		reason: "review_requested",
		updated_at: "2026-02-21T06:50:00Z",
		unread: 0,
		html_url: null,
	},
];

function DashboardPreview() {
	const items = makeMockFeed();
	const inFlightKeys = new Set(["release:10001"]);
	const [selectedDate, setSelectedDate] = useState<string | null>(
		mockBriefs[0]?.date ?? null,
	);

	return (
		<AppShell
			header={
				<div className="flex items-center justify-between">
					<div>
						<h1 className="text-xl font-semibold tracking-tight">OctoRill</h1>
						<p className="text-muted-foreground font-mono text-xs">
							Mock dashboard preview (Storybook)
						</p>
					</div>
				</div>
			}
		>
			<div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_360px]">
				<section className="min-w-0">
					<FeedList
						items={items}
						error={null}
						loadingInitial={false}
						loadingMore={false}
						hasMore={false}
						inFlightKeys={inFlightKeys}
						registerItemRef={() => () => {}}
						onLoadMore={() => {}}
						showOriginalByKey={{}}
						onToggleOriginal={() => {}}
						onTranslateNow={() => {}}
					/>
				</section>
				<aside className="space-y-6">
					<BriefListCard
						briefs={mockBriefs}
						selectedDate={selectedDate}
						onSelectDate={(d) => setSelectedDate(d)}
					/>
					<ReleaseDailyCard
						briefs={mockBriefs}
						selectedDate={selectedDate}
						busy={false}
						onGenerate={() => {}}
					/>
					<InboxQuickList notifications={mockNotifs} />
				</aside>
			</div>
		</AppShell>
	);
}

const meta = {
	title: "Pages/Dashboard",
	component: DashboardPreview,
	parameters: {
		layout: "fullscreen",
	},
} satisfies Meta<typeof DashboardPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
