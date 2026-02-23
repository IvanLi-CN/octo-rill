import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { FeedList } from "@/feed/FeedList";
import type { FeedItem } from "@/feed/types";
import { InboxList } from "@/inbox/InboxList";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { DashboardHeader } from "@/pages/DashboardHeader";
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
			excerpt:
				"- This is a stable release\n- Includes performance improvements\n- Please update and rebuild images",
			subtitle: null,
			reason: null,
			subject_type: null,
			html_url: "https://github.com/acme/rocket/releases/tag/v1.8.0",
			unread: null,
			translated: {
				lang: "zh-CN",
				status: "ready",
				title: "v1.8.0（稳定版）",
				summary: "- 这是一个稳定版本\n- 包含性能改进\n- 建议升级并重新构建镜像",
			},
			reactions: {
				counts: {
					plus1: 12,
					laugh: 2,
					heart: 6,
					hooray: 4,
					rocket: 9,
					eyes: 3,
				},
				viewer: {
					plus1: true,
					laugh: false,
					heart: false,
					hooray: true,
					rocket: false,
					eyes: false,
				},
				status: "ready",
			},
		},
		{
			kind: "release",
			ts: "2026-02-21T06:20:00Z",
			id: "10000",
			repo_full_name: "acme/rocket",
			title: "v1.7.3",
			excerpt: "- Patch release\n- Fixes a regression in auth flow",
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
				status: "reauth_required",
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
			"## 概览\n\n- 时间窗口（本地）：2026-02-20T08:00:00+08:00 → 2026-02-21T08:00:00+08:00\n- 更新项目：1 个\n- Release：2 条（预发布 0 条）\n- 涉及项目：[acme/rocket](https://github.com/acme/rocket)\n\n## 项目更新\n\n### [acme/rocket](https://github.com/acme/rocket)\n\n- [v1.8.0](/?tab=briefs&release=10001) · 2026-02-21T08:05:00Z · [GitHub Release](https://github.com/acme/rocket/releases/tag/v1.8.0)\n  - 稳定版发布，包含性能优化。\n  - 建议升级后重新构建镜像。\n",
		created_at: "2026-02-21T08:00:03Z",
	},
	{
		date: "2026-02-20",
		window_start: "2026-02-19T08:00:00+08:00",
		window_end: "2026-02-20T08:00:00+08:00",
		content_markdown:
			"## 概览\n\n- 时间窗口（本地）：2026-02-19T08:00:00+08:00 → 2026-02-20T08:00:00+08:00\n- 更新项目：1 个\n- Release：1 条（预发布 0 条）\n- 涉及项目：[acme/rocket](https://github.com/acme/rocket)\n\n## 项目更新\n\n### [acme/rocket](https://github.com/acme/rocket)\n\n- [v1.7.3](/?tab=briefs&release=10000) · 2026-02-20T06:20:00Z · [GitHub Release](https://github.com/acme/rocket/releases/tag/v1.7.3)\n  - 修复认证回归问题。\n",
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
	const inFlightKeys = new Set<string>();
	const reactionBusyKeys = new Set<string>();
	const aiDisabledHint = items.some(
		(it) => it.translated?.status === "disabled",
	);
	type Tab = "all" | "releases" | "briefs" | "inbox";
	const [tab, setTab] = useState<Tab>("all");
	const [showOriginalByKey, setShowOriginalByKey] = useState<
		Record<string, boolean>
	>({});
	const [selectedDate, setSelectedDate] = useState<string | null>(
		mockBriefs[0]?.date ?? null,
	);

	return (
		<AppShell
			header={
				<DashboardHeader
					feedCount={items.length}
					inboxCount={mockNotifs.length}
					briefCount={mockBriefs.length}
					login="storybook-user"
					aiDisabledHint={aiDisabledHint}
					busy={false}
					onRefresh={() => {}}
					onSyncAll={() => {}}
					onSyncStarred={() => {}}
					onSyncReleases={() => {}}
					onSyncInbox={() => {}}
					logoutHref="#"
				/>
			}
			footer={<AppMetaFooter />}
		>
			<div className="mb-4 flex flex-wrap items-center justify-between gap-2">
				<div className="flex flex-wrap items-center gap-2">
					<Button
						variant={tab === "all" ? "default" : "outline"}
						size="sm"
						className="font-mono text-xs"
						onClick={() => setTab("all")}
					>
						全部
					</Button>
					<Button
						variant={tab === "releases" ? "default" : "outline"}
						size="sm"
						className="font-mono text-xs"
						onClick={() => setTab("releases")}
					>
						Releases
					</Button>
					<Button
						variant={tab === "briefs" ? "default" : "outline"}
						size="sm"
						className="font-mono text-xs"
						onClick={() => setTab("briefs")}
					>
						日报
					</Button>
					<Button
						variant={tab === "inbox" ? "default" : "outline"}
						size="sm"
						className="font-mono text-xs"
						onClick={() => setTab("inbox")}
					>
						Inbox
					</Button>
				</div>
			</div>

			<div className="grid gap-6 md:grid-cols-[minmax(0,1fr)_360px]">
				<section className="min-w-0">
					{tab === "all" || tab === "releases" ? (
						<FeedList
							items={items}
							error={null}
							loadingInitial={false}
							loadingMore={false}
							hasMore={false}
							inFlightKeys={inFlightKeys}
							registerItemRef={() => () => {}}
							onLoadMore={() => {}}
							showOriginalByKey={showOriginalByKey}
							onToggleOriginal={(key) =>
								setShowOriginalByKey((prev) => ({ ...prev, [key]: !prev[key] }))
							}
							onTranslateNow={() => {}}
							reactionBusyKeys={reactionBusyKeys}
							onToggleReaction={() => {}}
							onSyncReleases={() => {}}
						/>
					) : null}

					{tab === "briefs" ? (
						<div className="space-y-6">
							<ReleaseDailyCard
								briefs={mockBriefs}
								selectedDate={selectedDate}
								busy={false}
								onGenerate={() => {}}
								onOpenRelease={() => {}}
							/>
						</div>
					) : null}

					{tab === "inbox" ? <InboxList notifications={mockNotifs} /> : null}
				</section>

				<aside className="space-y-6">
					{tab === "briefs" ? (
						<BriefListCard
							briefs={mockBriefs}
							selectedDate={selectedDate}
							onSelectDate={(d) => setSelectedDate(d)}
						/>
					) : null}
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
