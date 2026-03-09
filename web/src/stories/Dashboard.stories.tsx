import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
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

type Tab = "all" | "releases" | "briefs" | "inbox";

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
				status: "ready",
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

function DashboardPreview(props: {
	initialTab?: Tab;
	initialPatDialogOpen?: boolean;
}) {
	const { initialTab = "all", initialPatDialogOpen = false } = props;
	const items = makeMockFeed();
	const inFlightKeys = new Set<string>();
	const reactionBusyKeys = new Set<string>();
	const aiDisabledHint = items.some(
		(it) => it.translated?.status === "disabled",
	);
	const [tab, setTab] = useState<Tab>(initialTab);
	const [patDialogOpen, setPatDialogOpen] = useState(initialPatDialogOpen);
	const [showOriginalByKey, setShowOriginalByKey] = useState<
		Record<string, boolean>
	>({});
	const [selectedDate, setSelectedDate] = useState<string | null>(
		mockBriefs[0]?.date ?? null,
	);

	const feedPanel = (
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
			reactionErrorByKey={{}}
			onToggleReaction={() => {}}
			onSyncReleases={() => {}}
		/>
	);

	return (
		<AppShell
			header={
				<DashboardHeader
					feedCount={items.length}
					inboxCount={mockNotifs.length}
					briefCount={mockBriefs.length}
					login="storybook-user"
					isAdmin
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
			<Tabs
				value={tab}
				onValueChange={(nextTab) => setTab(nextTab as Tab)}
				className="gap-6"
			>
				<div className="flex flex-wrap items-center justify-between gap-2">
					<TabsList className="h-auto flex-wrap rounded-lg bg-muted/60 p-1">
						<TabsTrigger value="all" className="font-mono text-xs">
							全部
						</TabsTrigger>
						<TabsTrigger value="releases" className="font-mono text-xs">
							Releases
						</TabsTrigger>
						<TabsTrigger value="briefs" className="font-mono text-xs">
							日报
						</TabsTrigger>
						<TabsTrigger value="inbox" className="font-mono text-xs">
							Inbox
						</TabsTrigger>
					</TabsList>
					<div className="flex items-center gap-2">
						<Button
							variant="outline"
							size="sm"
							className="font-mono text-xs"
							onClick={() => setPatDialogOpen(true)}
						>
							打开 PAT 配置
						</Button>
						<Button
							asChild
							variant="outline"
							size="sm"
							className="font-mono text-xs"
						>
							<a href="/admin">管理员面板</a>
						</Button>
					</div>
				</div>

				<div className="grid gap-6 md:grid-cols-[minmax(0,1fr)_360px]">
					<section className="min-w-0">
						<TabsContent value="all" className="mt-0 min-w-0">
							{feedPanel}
						</TabsContent>
						<TabsContent value="releases" className="mt-0 min-w-0">
							{feedPanel}
						</TabsContent>
						<TabsContent value="briefs" className="mt-0 min-w-0">
							<div className="space-y-6">
								<ReleaseDailyCard
									briefs={mockBriefs}
									selectedDate={selectedDate}
									busy={false}
									onGenerate={() => {}}
									onOpenRelease={() => {}}
								/>
							</div>
						</TabsContent>
						<TabsContent value="inbox" className="mt-0 min-w-0">
							<InboxList notifications={mockNotifs} />
						</TabsContent>
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
			</Tabs>

			<Dialog open={patDialogOpen} onOpenChange={setPatDialogOpen}>
				<DialogContent
					showCloseButton={false}
					className="max-w-2xl"
					onInteractOutside={(event) => event.preventDefault()}
				>
					<DialogHeader>
						<DialogTitle>配置 GitHub PAT 以启用反馈表情</DialogTitle>
						<DialogDescription>
							当前 OAuth 登录仅用于读取与同步。站内点按反馈需要额外配置 PAT。
						</DialogDescription>
					</DialogHeader>
					<div className="bg-muted/40 rounded-lg border p-3">
						<p className="font-medium text-sm">创建路径（不限仓库口径）</p>
						<p className="text-muted-foreground mt-1 font-mono text-xs">
							Settings → Developer settings → Personal access tokens → Tokens
							(classic)
						</p>
					</div>
					<div className="space-y-2">
						<Label htmlFor="storybook-reaction-pat">GitHub PAT</Label>
						<Input
							id="storybook-reaction-pat"
							type="password"
							value="ghp_mock_dashboard_storybook_token"
							readOnly
							className="font-mono text-sm"
						/>
					</div>
					<p className="text-xs text-emerald-600">
						Storybook 中使用固定有效态，便于回归 Dialog / Input / Label 布局。
					</p>
					<DialogFooter>
						<Button variant="outline" onClick={() => setPatDialogOpen(false)}>
							稍后再说
						</Button>
						<Button>保存并继续</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
		</AppShell>
	);
}

const meta = {
	title: "Pages/Dashboard",
	component: DashboardPreview,
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"Dashboard 组合了 Feed、Brief、Inbox、Release 详情与 PAT 对话框，是 OctoRill 登录后的主工作台。通过这组故事可以确认不同默认入口和操作弹窗是否符合预期。\n\n相关公开文档：[产品说明](../product.html) · [配置参考](../config.html)",
			},
		},
	},
	args: {
		initialTab: "all",
		initialPatDialogOpen: false,
	},
} satisfies Meta<typeof DashboardPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	parameters: {
		docs: {
			description: {
				story: "主工作区默认入口，覆盖 Feed、侧栏摘要与页面骨架。",
			},
		},
	},
};

export const BriefsFocused: Story = {
	args: {
		initialTab: "briefs",
	},
	parameters: {
		docs: {
			description: {
				story: "把初始焦点切到 Briefs，用来验证日报与摘要场景的可读性。",
			},
		},
	},
};

export const PatDialogOpen: Story = {
	args: {
		initialTab: "briefs",
		initialPatDialogOpen: true,
	},
	parameters: {
		docs: {
			description: {
				story: "直接展示 Release 反馈 PAT 对话框打开时的交互状态。",
			},
		},
	},
};
