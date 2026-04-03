import type { Meta, StoryObj } from "@storybook/react-vite";
import { useLayoutEffect, useState } from "react";
import { expect, within } from "storybook/test";

import type { ReleaseDetailResponse } from "@/api";
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
import { ReleaseDetailCard } from "@/sidebar/ReleaseDetailCard";

type Tab = "all" | "releases" | "briefs" | "inbox";
type FeedMode =
	| "default"
	| "visible-window-queued"
	| "visible-window-settling"
	| "body-limit-error"
	| "sync-preheated";
const SYNC_ALL_LABEL = "同步";
const LONG_BRIEF_RELEASE_ID = "777001";

function buildFeedItem(id: string, overrides?: Partial<FeedItem>): FeedItem {
	return {
		kind: "release",
		ts: "2026-02-21T08:05:00Z",
		id,
		repo_full_name: "acme/rocket",
		title: `v${id}`,
		body: "- This is a stable release\n- Includes performance improvements\n- Please update and rebuild images",
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `https://github.com/acme/rocket/releases/tag/${id}`,
		unread: null,
		translated: {
			lang: "zh-CN",
			status: "missing",
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

function makeMockFeed(): FeedItem[] {
	return [
		buildFeedItem("10001", {
			title: "v1.8.0",
			html_url: "https://github.com/acme/rocket/releases/tag/v1.8.0",
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
		}),
		buildFeedItem("10000", {
			ts: "2026-02-21T06:20:00Z",
			title: "v1.7.3",
			body: "- Patch release\n- Fixes a regression in auth flow",
			html_url: "https://github.com/acme/rocket/releases/tag/v1.7.3",
			translated: {
				lang: "zh-CN",
				status: "disabled",
				title: null,
				summary: null,
			},
		}),
	];
}

function makeVisibleWindowFeed(
	mode: "visible-window-queued" | "visible-window-settling",
): FeedItem[] {
	const items = Array.from({ length: 12 }, (_, index) => {
		const seq = `${index + 1}`.padStart(2, "0");
		return buildFeedItem(`200${seq}`, {
			ts: `2026-02-21T0${(index % 6) + 1}:15:00Z`,
			title: `v2.0.${seq}`,
			html_url: `https://github.com/acme/rocket/releases/tag/v2.0.${seq}`,
			body: [
				`- Release lane ${seq}`,
				"- Includes UI polish and API cleanup",
				"- Refresh worker telemetry and translation batching",
			].join("\n"),
		});
	});

	if (mode === "visible-window-settling") {
		for (const index of [0, 1, 2, 3]) {
			items[index] = {
				...items[index],
				translated: {
					lang: "zh-CN",
					status: "ready",
					title: `${items[index].title}（中文）`,
					summary: `- 中文摘要 ${index + 1}\n- 可见卡片已经优先收敛\n- 可见窗口后的 10 条继续预取`,
				},
			};
		}
	}

	return items;
}

function makeBodyLimitErrorFeed(): FeedItem[] {
	return [
		buildFeedItem("30001", {
			title: "v3.0.0",
			body: [
				"- Introduces the new deployment lane",
				"- Ships a very long migration checklist",
				"- Operators should read the full release on GitHub",
			].join("\n"),
			body_truncated: true,
			translated: {
				lang: "zh-CN",
				status: "error",
				title: null,
				summary: null,
				auto_translate: false,
			},
		}),
	];
}

function makeSyncPreheatedFeed(): FeedItem[] {
	return [
		buildFeedItem("40001", {
			title: "v4.1.0",
			body: [
				"- Background sync already queued translation",
				"- The first dashboard open should reuse cached zh-CN content",
			].join("\n"),
			translated: {
				lang: "zh-CN",
				status: "ready",
				title: "v4.1.0（后台预热）",
				summary: [
					"- 后台同步已经预热翻译",
					"- 首次打开列表直接命中缓存结果",
				].join("\n"),
			},
		}),
		buildFeedItem("40000", {
			ts: "2026-02-21T05:40:00Z",
			title: "v4.0.9",
			body: "- Next release is still translating in background",
		}),
	];
}

function makeVisibleWindowInFlightKeys(
	mode: "visible-window-queued" | "visible-window-settling",
) {
	return new Set(
		mode === "visible-window-queued"
			? ["release:20001", "release:20002", "release:20003", "release:20004"]
			: ["release:20005", "release:20006"],
	);
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

const longBriefMarkdown = [
	"## 概览",
	"",
	"- 时间窗口（本地）：2026-04-02T08:00:00+08:00 → 2026-04-03T08:00:00+08:00",
	"- 更新项目：8 个",
	"- Release：14 条（预发布 3 条）",
	"- 涉及项目：`acme/rocket`、`acme/satellite`、`acme/hangar`、`acme/telemetry`、`acme/spark`、`acme/atlas`、`acme/relay`、`acme/fleet`",
	"",
	"## 项目更新",
	"",
	...Array.from({ length: 8 }, (_, index) => {
		const repo = [
			"acme/rocket",
			"acme/satellite",
			"acme/hangar",
			"acme/telemetry",
			"acme/spark",
			"acme/atlas",
			"acme/relay",
			"acme/fleet",
		][index];
		const releaseId = index === 0 ? LONG_BRIEF_RELEASE_ID : `${777002 + index}`;
		return [
			`### [${repo}](https://github.com/${repo})`,
			"",
			`- [v${index + 3}.4.${index}](/?tab=briefs&release=${releaseId}) · 2026-04-03T0${index}:12:00Z · [GitHub Release](https://github.com/${repo}/releases/tag/v${index + 3}.4.${index})`,
			`  - 完成第 ${index + 1} 波发布，补齐构建链路、监控面板与回滚脚本。`,
			`  - 新增 \`owner-reviewed\` 发布检查项，并收敛环境差异带来的配置漂移。`,
			`  - 清理过期镜像标签、重跑 smoke tests，并同步更新部署说明。`,
			"",
		].join("\n");
	}),
].join("\n");

const longBriefs: BriefItem[] = [
	{
		date: "2026-04-03",
		window_start: "2026-04-02T08:00:00+08:00",
		window_end: "2026-04-03T08:00:00+08:00",
		content_markdown: longBriefMarkdown,
		created_at: "2026-04-03T08:00:35Z",
	},
	mockBriefs[1],
];

const longReleaseDetail: ReleaseDetailResponse = {
	release_id: LONG_BRIEF_RELEASE_ID,
	repo_full_name: "acme/rocket",
	tag_name: "v3.4.0",
	name: "v3.4.0 · release train",
	body: [
		"## Release train summary",
		"",
		...Array.from({ length: 10 }, (_, index) => {
			return [
				`### Wave ${index + 1}`,
				"",
				"- Added rollout guardrails for environment-specific configuration.",
				"- Replayed migration checks against staging and production snapshots.",
				"- Published recovery steps and post-release verification notes.",
				"",
			].join("\n");
		}),
	].join("\n"),
	html_url: "https://github.com/acme/rocket/releases/tag/v3.4.0",
	published_at: "2026-04-03T06:18:00Z",
	is_prerelease: 0,
	is_draft: 0,
	translated: {
		lang: "zh-CN",
		status: "ready",
		title: "v3.4.0 · 发布波次总览",
		summary: [
			"## 翻译总览",
			"",
			...Array.from({ length: 10 }, (_, index) => {
				return [
					`### 变更波次 ${index + 1}`,
					"",
					"- 补齐环境差异下的发布守卫，统一回滚前置检查。",
					"- 对 staging / production 快照重新跑迁移校验，并记录验证结果。",
					"- 更新 smoke test、值班 SOP 与发布后核对清单，确保交付闭环。",
					"",
				].join("\n");
			}),
		].join("\n"),
	},
};

function useStorybookReleaseDetailMock(detail: ReleaseDetailResponse | null) {
	useLayoutEffect(() => {
		if (!detail) return;

		const previousFetch = globalThis.fetch.bind(globalThis);
		const detailPath = `/api/releases/${encodeURIComponent(detail.release_id)}/detail`;

		globalThis.fetch = async (input, init) => {
			const rawUrl =
				typeof input === "string"
					? input
					: input instanceof URL
						? input.toString()
						: input.url;
			const resolvedUrl = new URL(rawUrl, window.location.origin);

			if (resolvedUrl.pathname === detailPath) {
				return new Response(JSON.stringify(detail), {
					headers: { "Content-Type": "application/json" },
					status: 200,
				});
			}

			return previousFetch(input, init);
		};

		return () => {
			globalThis.fetch = previousFetch;
		};
	}, [detail]);
}

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
	syncingAll?: boolean;
	showEmptyInbox?: boolean;
	emptyState?: "content" | "auto-sync" | "no-cache";
	feedMode?: FeedMode;
	briefs?: BriefItem[];
	initialReleaseId?: string | null;
	releaseDetail?: ReleaseDetailResponse | null;
}) {
	const {
		initialTab = "all",
		initialPatDialogOpen = false,
		syncingAll = false,
		showEmptyInbox = false,
		emptyState = "content",
		feedMode = "default",
		briefs = mockBriefs,
		initialReleaseId = null,
		releaseDetail = null,
	} = props;
	useStorybookReleaseDetailMock(releaseDetail);
	const items =
		emptyState !== "content"
			? []
			: feedMode === "default"
				? makeMockFeed()
				: feedMode === "body-limit-error"
					? makeBodyLimitErrorFeed()
					: feedMode === "sync-preheated"
						? makeSyncPreheatedFeed()
						: makeVisibleWindowFeed(feedMode);
	const notifications = showEmptyInbox ? [] : mockNotifs;
	const inFlightKeys =
		emptyState !== "content" ||
		feedMode === "default" ||
		feedMode === "body-limit-error" ||
		feedMode === "sync-preheated"
			? new Set<string>()
			: makeVisibleWindowInFlightKeys(feedMode);
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
		briefs[0]?.date ?? null,
	);
	const [activeReleaseId, setActiveReleaseId] = useState<string | null>(
		initialReleaseId,
	);

	const feedPanel =
		items.length === 0 ? (
			<div className="bg-card/70 mb-4 rounded-xl border p-6 shadow-sm">
				{emptyState === "auto-sync" ? (
					<>
						<h2 className="text-base font-semibold tracking-tight">
							正在同步你的 Star / Release
						</h2>
						<p className="text-muted-foreground mt-1 text-sm">
							先展示服务端已有缓存，再补齐最新仓库数据；完成后这里会自动刷新。
						</p>
					</>
				) : (
					<>
						<h2 className="text-base font-semibold tracking-tight">
							还没有缓存内容
						</h2>
						<p className="text-muted-foreground mt-1 text-sm">
							可以先同步 Star / Release；Inbox 仍然单独同步。
						</p>
						<div className="mt-4 flex flex-wrap gap-2">
							<Button disabled={syncingAll}>{SYNC_ALL_LABEL}</Button>
							<Button variant="outline">Sync starred</Button>
							<Button variant="outline">Sync releases</Button>
							<Button variant="outline">Sync inbox</Button>
						</div>
					</>
				)}
			</div>
		) : (
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
			/>
		);

	return (
		<AppShell
			header={
				<DashboardHeader
					feedCount={items.length}
					inboxCount={notifications.length}
					briefCount={briefs.length}
					login="storybook-user"
					isAdmin
					aiDisabledHint={aiDisabledHint}
					busy={syncingAll}
					syncingAll={syncingAll}
					onSyncAll={() => {}}
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
									briefs={briefs}
									selectedDate={selectedDate}
									busy={false}
									onGenerate={() => {}}
									onOpenRelease={setActiveReleaseId}
								/>
								<ReleaseDetailCard
									releaseId={activeReleaseId}
									onClose={() => setActiveReleaseId(null)}
								/>
							</div>
						</TabsContent>
						<TabsContent value="inbox" className="mt-0 min-w-0">
							<InboxList
								notifications={notifications}
								busy={syncingAll}
								syncing={syncingAll}
								onSync={tab === "inbox" ? () => {} : undefined}
							/>
						</TabsContent>
					</section>

					<aside className="space-y-6">
						{tab === "briefs" ? (
							<BriefListCard
								briefs={briefs}
								selectedDate={selectedDate}
								onSelectDate={(d) => setSelectedDate(d)}
							/>
						) : null}
						<InboxQuickList notifications={notifications} />
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
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"Dashboard 组合了 Feed、Brief、Inbox、Release 详情与 PAT 对话框，是 OctoRill 登录后的主工作台。当前同步入口统一收敛为一个顶部主按钮，这组 stories 用来确认默认、同步中与空态文案是否保持一致。\n\n相关公开文档：[产品说明](../product.html) · [配置参考](../config.html)",
			},
		},
	},
	args: {
		initialTab: "all",
		initialPatDialogOpen: false,
		syncingAll: false,
		showEmptyInbox: false,
		emptyState: "content",
		feedMode: "default",
		briefs: undefined,
		initialReleaseId: null,
		releaseDetail: null,
	},
} satisfies Meta<typeof DashboardPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	parameters: {
		docs: {
			description: {
				story:
					"主工作区默认入口，验证顶部只保留一个主同步按钮，Feed 与侧栏维持正常内容态。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("button", { name: SYNC_ALL_LABEL }),
		).toBeVisible();
		await expect(
			canvas.queryByRole("button", { name: "Refresh" }),
		).not.toBeInTheDocument();
		await expect(
			canvas.queryByRole("button", { name: "Sync starred" }),
		).not.toBeInTheDocument();
		await expect(
			canvas.queryByRole("button", { name: "Sync releases" }),
		).not.toBeInTheDocument();
		await expect(
			canvas.queryByRole("button", { name: "Sync inbox" }),
		).not.toBeInTheDocument();
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

export const BriefsLongContent: Story = {
	render: () => <DashboardPreview initialTab="briefs" briefs={longBriefs} />,
	parameters: {
		docs: {
			description: {
				story:
					"长日报内容应直接拉伸卡片高度，由页面整体滚动承载阅读，不再在卡片内部出现纵向滚动区。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText(/更新项目：8 个/)).toBeVisible();
		expect(canvasElement.querySelector(".max-h-96")).toBeNull();
		expect(canvasElement.querySelector(".overflow-auto")).toBeNull();
	},
};

export const BriefsLongContentWithDetail: Story = {
	render: () => (
		<DashboardPreview
			initialTab="briefs"
			briefs={longBriefs}
			initialReleaseId={LONG_BRIEF_RELEASE_ID}
			releaseDetail={longReleaseDetail}
		/>
	),
	parameters: {
		docs: {
			description: {
				story:
					"从日报内部链接打开 release 详情时，详情正文同样随内容扩展，并继续由页面主滚动条承载。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(await canvas.findByText(/翻译总览/)).toBeVisible();
		await expect(canvas.getByText(/变更波次 10/)).toBeVisible();
		expect(canvasElement.querySelector(".max-h-96")).toBeNull();
		expect(canvasElement.querySelector(".overflow-auto")).toBeNull();
	},
};

export const Syncing: Story = {
	args: {
		syncingAll: true,
	},
	parameters: {
		docs: {
			description: {
				story:
					"同步进行中：顶部主按钮禁用，左侧刷新 icon 旋转，其他同步入口保持收敛。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const syncButton = canvas.getByRole("button", { name: SYNC_ALL_LABEL });
		await expect(syncButton).toBeDisabled();
		const icon = syncButton.querySelector("svg");
		expect(icon).not.toBeNull();
		expect(icon?.classList.contains("animate-spin")).toBe(true);
	},
};

export const VisibleWindowQueue: Story = {
	args: {
		initialTab: "releases",
		feedMode: "visible-window-queued",
	},
	parameters: {
		docs: {
			description: {
				story:
					"前端按真实视口收集当前可见 release，并把最后一个可见卡片之后的 10 条一起交给结果聚合接口。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "v2.0.01" }),
		).toBeVisible();
		await expect(
			canvas.getAllByRole("button", { name: "翻译中…" }),
		).toHaveLength(4);
	},
};

export const VisibleWindowSettling: Story = {
	args: {
		initialTab: "releases",
		feedMode: "visible-window-settling",
	},
	parameters: {
		docs: {
			description: {
				story:
					"部分可见卡片已回填中文，后续窗口仍继续从结果聚合接口回收运行中的译文。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "v2.0.01（中文）" }),
		).toBeVisible();
		await expect(
			canvas.getAllByRole("button", { name: "翻译中…" }),
		).toHaveLength(2);
	},
};

export const EmptyFeed: Story = {
	args: {
		emptyState: "no-cache",
	},
	parameters: {
		docs: {
			description: {
				story:
					"Feed 无缓存时展示 staged refresh 的回退空态，既保留主同步按钮，也允许单独触发 Star / Release / Inbox。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText(/还没有缓存内容/)).toBeVisible();
		await expect(
			canvas.getAllByRole("button", { name: SYNC_ALL_LABEL }),
		).toHaveLength(2);
		await expect(
			canvas.getByRole("button", { name: "Sync starred" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "Sync releases" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "Sync inbox" }),
		).toBeVisible();
	},
};

export const InboxEmpty: Story = {
	args: {
		initialTab: "inbox",
		showEmptyInbox: true,
	},
	parameters: {
		docs: {
			description: {
				story:
					"Inbox 空态保留局部 Sync inbox 入口，避免访问触发同步只覆盖 Star/Release 时无法补通知。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("button", { name: "Sync inbox" }),
		).toBeVisible();
		await expect(canvas.getByText(/暂无通知。可以点击/)).toBeVisible();
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

export const AccessSyncEmptyState: Story = {
	args: {
		emptyState: "auto-sync",
	},
	parameters: {
		docs: {
			description: {
				story:
					"首访或超过 1 小时未访问时的自动同步空态，验证 staged refresh 期间不会再提示手动点 Sync all。",
			},
		},
	},
};

export const BodyLimitError: Story = {
	args: {
		initialTab: "releases",
		feedMode: "body-limit-error",
	},
	parameters: {
		docs: {
			description: {
				story:
					"超长 Release 正文在列表中只展示受限正文，并明确提示该卡片不会自动翻译。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText(/列表正文已截断显示/)).toBeVisible();
		await expect(canvas.getByText(/自动翻译不可用/)).toBeVisible();
		await expect(canvas.getByRole("button", { name: "翻译" })).toBeDisabled();
	},
};

export const SyncPreheated: Story = {
	args: {
		initialTab: "releases",
		feedMode: "sync-preheated",
	},
	parameters: {
		docs: {
			description: {
				story:
					"同步任务写入新 release 后，后台预热翻译应让首次打开列表直接看到已缓存的正文译文。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "v4.1.0（后台预热）" }),
		).toBeVisible();
		await expect(canvas.getByText(/后台同步已经预热翻译/)).toBeVisible();
	},
};
