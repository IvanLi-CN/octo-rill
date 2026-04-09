import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useLayoutEffect, useState } from "react";
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
import { FeedGroupedList } from "@/feed/FeedGroupedList";
import { FeedPageLaneSelector } from "@/feed/FeedPageLaneSelector";
import {
	DEFAULT_PAGE_LANE,
	resolveDisplayLaneForFeed,
	resolvePreferredLaneForItem,
} from "@/feed/laneOptions";
import type { FeedItem, FeedLane } from "@/feed/types";
import { InboxList } from "@/inbox/InboxList";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";
import { DashboardHeader } from "@/pages/DashboardHeader";
import { BriefListCard } from "@/sidebar/BriefListCard";
import {
	InboxQuickList,
	type NotificationItem,
} from "@/sidebar/InboxQuickList";
import { type BriefItem, ReleaseDailyCard } from "@/sidebar/ReleaseDailyCard";
import { ReleaseDetailCard } from "@/sidebar/ReleaseDetailCard";
import { VersionMonitorStateProvider } from "@/version/versionMonitor";

type Tab = "all" | "releases" | "briefs" | "inbox";
type FeedMode =
	| "default"
	| "visible-window-queued"
	| "visible-window-settling"
	| "body-limit-error"
	| "sync-preheated"
	| "smart-ready-body"
	| "smart-ready-diff"
	| "smart-loading"
	| "smart-retry-error"
	| "smart-insufficient";
const SYNC_ALL_LABEL = "同步";
const LONG_BRIEF_RELEASE_ID = "777001";
const STORYBOOK_DAILY_BOUNDARY = "08:00";
const STORYBOOK_DAILY_BOUNDARY_TIME_ZONE = "Asia/Shanghai";
const STORYBOOK_DAILY_BOUNDARY_UTC_OFFSET_MINUTES = 8 * 60;
const STORYBOOK_NOW = new Date("2026-04-04T12:00:00+08:00");
const HISTORY_RAW_MARKER = "raw-history-guardrails-marker";
const FALLBACK_RAW_MARKER = "raw-fallback-release-marker";
const STORYBOOK_VERSION_STATE = {
	loadedVersion: "v2.4.6",
	availableVersion: null,
	hasUpdate: false,
	refreshPage: () => {},
} as const;

function feedItemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

function defaultLaneForItem(
	item: FeedItem,
	pageDefaultLane: FeedLane,
): FeedLane {
	return resolvePreferredLaneForItem(item, pageDefaultLane);
}

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
		smart: {
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
			ts: "2026-04-04T16:16:29+08:00",
			title: "v2.63.0",
			html_url: "https://github.com/acme/rocket/releases/tag/v2.63.0",
			smart: {
				lang: "zh-CN",
				status: "ready",
				title: "v2.63.0 · 版本变化",
				summary:
					"- 收敛发布链路与 smoke tests，降低稳定版上线风险\n- 补齐缓存策略与构建链路，减少部署后手动修复",
			},
			translated: {
				lang: "zh-CN",
				status: "ready",
				title: "v2.63.0（稳定版）",
				summary:
					"- 发布稳定版本并更新构建链路\n- 补齐 smoke tests 与缓存策略\n- 建议升级后重新构建镜像",
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
		buildFeedItem("10002", {
			ts: "2026-04-04T15:56:24+08:00",
			repo_full_name: "lobehub/lobe-chat",
			title: "桌面版 Canary v2.1.48-canary.31",
			body: "- Canary 构建\n- 自动发布桌面包\n- 建议先在测试环境验证",
			html_url:
				"https://github.com/lobehub/lobe-chat/releases/tag/v2.1.48-canary.31",
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
		}),
		buildFeedItem("10003", {
			ts: "2026-04-04T07:10:00+08:00",
			title: "nightly guardrails",
			body: `- ${HISTORY_RAW_MARKER}\n- Tighten upload guardrails\n- Normalize rollout order`,
			html_url:
				"https://github.com/acme/rocket/releases/tag/nightly-guardrails",
		}),
		buildFeedItem("10004", {
			ts: "2026-04-03T21:30:00+08:00",
			repo_full_name: "acme/satellite",
			title: "oauth action bubble polish",
			body: "- stabilize oauth actions\n- dedupe previews\n- align hover states",
			html_url:
				"https://github.com/acme/satellite/releases/tag/oauth-action-bubble",
		}),
		buildFeedItem("10005", {
			ts: "2026-04-03T06:20:00+08:00",
			repo_full_name: "acme/fleet",
			title: "fallback lane release",
			body: `- ${FALLBACK_RAW_MARKER}\n- no brief available for this day\n- keep original release cards visible`,
			html_url:
				"https://github.com/acme/fleet/releases/tag/fallback-lane-release",
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
			smart: {
				lang: "zh-CN",
				status: "ready",
				title: "v4.1.0 · 后台智能预热",
				summary: [
					"- 后台同步已经预热智能版本摘要",
					"- 首次打开 release feed 直接看到版本变化要点",
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

function makeSmartReadyBodyFeed(): FeedItem[] {
	return [
		buildFeedItem("50001", {
			title: "v5.0.0",
			body: [
				"- Added signed desktop packages for macOS and Windows",
				"- Fixed production webhook retry duplication",
				"- Simplified release health checks for operators",
			].join("\n"),
			smart: {
				lang: "zh-CN",
				status: "ready",
				title: "v5.0.0 · 版本变化",
				summary: [
					"- 新增 macOS / Windows 已签名桌面包，交付链路更完整",
					"- 修复生产环境 webhook 重试重复触发问题",
					"- 收敛发布健康检查步骤，降低运维核对成本",
				].join("\n"),
			},
		}),
	];
}

function makeSmartReadyDiffFeed(): FeedItem[] {
	return [
		buildFeedItem("50002", {
			title: "v5.1.0",
			body: "See compare view for details.",
			smart: {
				lang: "zh-CN",
				status: "ready",
				title: "v5.1.0 · 智能整理",
				summary: [
					"- 主要变更集中在认证中间件与上传流程，强化失败回退",
					"- 补齐批处理调度与监控字段，方便排查运行中的任务状态",
					"- 若干 UI 细节与错误提示被统一，减少发布后的人工确认步骤",
				].join("\n"),
			},
		}),
	];
}

function makeSmartLoadingFeed(): FeedItem[] {
	return [
		buildFeedItem("50003", {
			title: "v5.2.0",
			body: "- Placeholder body for smart loading state",
			smart: {
				lang: "zh-CN",
				status: "missing",
				title: null,
				summary: null,
			},
		}),
	];
}

function makeSmartRetryErrorFeed(): FeedItem[] {
	return [
		buildFeedItem("50005", {
			title: "v5.2.1",
			body: "- Placeholder body for smart retry state",
			smart: {
				lang: "zh-CN",
				status: "error",
				title: null,
				summary: null,
				auto_translate: false,
			},
		}),
	];
}

function makeSmartInsufficientFeed(): FeedItem[] {
	return [
		buildFeedItem("50004", {
			title: "v5.3.0",
			body: "See assets below.",
			smart: {
				lang: "zh-CN",
				status: "insufficient",
				title: null,
				summary: null,
				auto_translate: false,
			},
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

function makeSmartInFlightKeys(mode: FeedMode) {
	return new Set(
		mode === "smart-loading"
			? ["release:50003"]
			: mode === "smart-retry-error"
				? []
				: [],
	);
}

const mockBriefs: BriefItem[] = [
	{
		date: "2026-04-04",
		window_start: "2026-04-03T08:00:00+08:00",
		window_end: "2026-04-04T08:00:00+08:00",
		content_markdown:
			"## 概览\n\n- 时间窗口（本地）：2026-04-03T08:00:00+08:00 → 2026-04-04T08:00:00+08:00\n- 更新项目：2 个\n- Release：2 条（预发布 0 条）\n- 涉及项目：[acme/rocket](https://github.com/acme/rocket)、[acme/satellite](https://github.com/acme/satellite)\n\n## 项目更新\n\n### [acme/rocket](https://github.com/acme/rocket)\n\n- [nightly guardrails](/?tab=briefs&release=10003) · 2026-04-03T23:10:00+08:00 · [GitHub Release](https://github.com/acme/rocket/releases/tag/nightly-guardrails)\n  - 收敛上传守卫，避免批量发布时的顺序漂移。\n\n### [acme/satellite](https://github.com/acme/satellite)\n\n- [oauth action bubble polish](/?tab=briefs&release=10004) · 2026-04-03T21:30:00+08:00 · [GitHub Release](https://github.com/acme/satellite/releases/tag/oauth-action-bubble)\n  - 统一 oauth 批量操作气泡与 hover 态。\n",
		created_at: "2026-04-04T08:00:03+08:00",
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
		date: "2026-04-04",
		window_start: "2026-04-03T08:00:00+08:00",
		window_end: "2026-04-04T08:00:00+08:00",
		content_markdown: longBriefMarkdown,
		created_at: "2026-04-04T08:00:35+08:00",
	},
	mockBriefs[0],
];

const generatedBriefTemplates: Record<string, BriefItem> = {
	"2026-04-03": {
		date: "2026-04-03",
		window_start: "2026-04-02T08:00:00+08:00",
		window_end: "2026-04-03T08:00:00+08:00",
		content_markdown:
			"## 概览\n\n- 时间窗口（本地）：2026-04-02T08:00:00+08:00 → 2026-04-03T08:00:00+08:00\n- 更新项目：1 个\n- Release：1 条（预发布 0 条）\n- 涉及项目：[acme/fleet](https://github.com/acme/fleet)\n\n## 项目更新\n\n### [acme/fleet](https://github.com/acme/fleet)\n\n- [fallback lane release](/?tab=briefs&release=10005) · 2026-04-03T06:20:00+08:00 · [GitHub Release](https://github.com/acme/fleet/releases/tag/fallback-lane-release)\n  - 回补这一天的日报摘要，用来验证按天生成后的展示切换。\n",
		created_at: "2026-04-03T08:00:03+08:00",
	},
};

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
		html_url: "https://github.com/notifications?query=repo%3Aacme%2Frocket",
	},
	{
		thread_id: "90000",
		repo_full_name: "acme/rocket",
		subject_title: "PR: bump deps",
		subject_type: "PullRequest",
		reason: "review_requested",
		updated_at: "2026-02-21T06:50:00Z",
		unread: 0,
		html_url: "https://github.com/acme/rocket/pull/42",
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
	dailyBoundaryLocal?: string;
	dailyBoundaryTimeZone?: string;
	dailyBoundaryUtcOffsetMinutes?: number;
	now?: Date;
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
		dailyBoundaryLocal = STORYBOOK_DAILY_BOUNDARY,
		dailyBoundaryTimeZone = STORYBOOK_DAILY_BOUNDARY_TIME_ZONE,
		dailyBoundaryUtcOffsetMinutes = STORYBOOK_DAILY_BOUNDARY_UTC_OFFSET_MINUTES,
		now = STORYBOOK_NOW,
		initialReleaseId = null,
		releaseDetail = null,
	} = props;
	useStorybookReleaseDetailMock(releaseDetail);
	const [storyBriefs, setStoryBriefs] = useState<BriefItem[]>(briefs);

	useEffect(() => {
		setStoryBriefs(briefs);
	}, [briefs]);

	const items =
		emptyState !== "content"
			? []
			: feedMode === "default"
				? makeMockFeed()
				: feedMode === "body-limit-error"
					? makeBodyLimitErrorFeed()
					: feedMode === "sync-preheated"
						? makeSyncPreheatedFeed()
						: feedMode === "smart-ready-body"
							? makeSmartReadyBodyFeed()
							: feedMode === "smart-ready-diff"
								? makeSmartReadyDiffFeed()
								: feedMode === "smart-loading"
									? makeSmartLoadingFeed()
									: feedMode === "smart-retry-error"
										? makeSmartRetryErrorFeed()
										: feedMode === "smart-insufficient"
											? makeSmartInsufficientFeed()
											: makeVisibleWindowFeed(feedMode);
	const notifications = showEmptyInbox ? [] : mockNotifs;
	const translationInFlightKeys =
		emptyState !== "content" ||
		feedMode === "default" ||
		feedMode === "body-limit-error" ||
		feedMode === "sync-preheated" ||
		feedMode === "smart-ready-body" ||
		feedMode === "smart-ready-diff" ||
		feedMode === "smart-loading" ||
		feedMode === "smart-retry-error" ||
		feedMode === "smart-insufficient"
			? new Set<string>()
			: makeVisibleWindowInFlightKeys(feedMode);
	const [smartInFlightKeys, setSmartInFlightKeys] = useState<Set<string>>(() =>
		makeSmartInFlightKeys(feedMode),
	);
	const reactionBusyKeys = new Set<string>();
	const aiDisabledHint = items.some(
		(it) =>
			it.translated?.status === "disabled" || it.smart?.status === "disabled",
	);
	const [tab, setTab] = useState<Tab>(initialTab);
	const [patDialogOpen, setPatDialogOpen] = useState(initialPatDialogOpen);
	const [selectedLaneByKey, setSelectedLaneByKey] = useState<
		Record<string, FeedLane>
	>({});
	const [pageDefaultLane, setPageDefaultLane] =
		useState<FeedLane>(DEFAULT_PAGE_LANE);
	const effectivePageDefaultLane = resolveDisplayLaneForFeed(
		items,
		pageDefaultLane,
	);
	const [selectedDate, setSelectedDate] = useState<string | null>(
		briefs[0]?.date ?? null,
	);
	const [activeReleaseId, setActiveReleaseId] = useState<string | null>(
		initialReleaseId,
	);

	useEffect(() => {
		setSmartInFlightKeys(makeSmartInFlightKeys(feedMode));
	}, [feedMode]);

	const openReleaseDetail = (releaseId: string) => {
		setTab("briefs");
		setActiveReleaseId(releaseId);
	};

	const generateBriefForDate = async (date: string) => {
		await new Promise((resolve) => window.setTimeout(resolve, 900));
		const nextBrief =
			generatedBriefTemplates[date] ??
			({
				date,
				window_start: null,
				window_end: null,
				content_markdown:
					"## 概览\n\n- 这是一条 Storybook 生成的占位日报，用于验证日组交互。",
				created_at: `${date}T08:00:03+08:00`,
			} satisfies BriefItem);
		setStoryBriefs((current) =>
			[nextBrief, ...current.filter((brief) => brief.date !== date)].sort(
				(a, b) => b.date.localeCompare(a.date),
			),
		);
	};

	const triggerSmartNow = (item: FeedItem) => {
		if (feedMode !== "smart-retry-error") return;
		const key = feedItemKey(item);
		setSmartInFlightKeys((current) => {
			if (current.has(key)) return current;
			const next = new Set(current);
			next.add(key);
			return next;
		});
	};

	const renderFeedPanel = (mode: "all" | "releases") =>
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
			<FeedGroupedList
				mode={mode}
				items={items}
				briefs={storyBriefs}
				dailyBoundaryLocal={dailyBoundaryLocal}
				dailyBoundaryTimeZone={dailyBoundaryTimeZone}
				dailyBoundaryUtcOffsetMinutes={dailyBoundaryUtcOffsetMinutes}
				now={now}
				error={null}
				loadingInitial={false}
				loadingMore={false}
				hasMore={false}
				translationInFlightKeys={translationInFlightKeys}
				smartInFlightKeys={smartInFlightKeys}
				registerItemRef={() => () => {}}
				onLoadMore={() => {}}
				selectedLaneByKey={Object.fromEntries(
					items.map((item) => [
						feedItemKey(item),
						selectedLaneByKey[feedItemKey(item)] ??
							defaultLaneForItem(item, pageDefaultLane),
					]),
				)}
				onSelectLane={(item, lane) =>
					setSelectedLaneByKey((prev) => ({
						...prev,
						[feedItemKey(item)]: lane,
					}))
				}
				onTranslateNow={() => {}}
				onSmartNow={triggerSmartNow}
				reactionBusyKeys={reactionBusyKeys}
				reactionErrorByKey={{}}
				onToggleReaction={() => {}}
				onOpenReleaseFromBrief={mode === "all" ? openReleaseDetail : undefined}
				onGenerateBriefForDate={
					mode === "all" ? generateBriefForDate : undefined
				}
			/>
		);

	return (
		<VersionMonitorStateProvider value={STORYBOOK_VERSION_STATE}>
			<AppShell
				header={
					<DashboardHeader
						feedCount={items.length}
						inboxCount={notifications.length}
						briefCount={storyBriefs.length}
						login="storybook-user"
						isAdmin
						aiDisabledHint={aiDisabledHint}
						busy={syncingAll}
						syncingAll={syncingAll}
						onSyncAll={() => {}}
						logoutHref="#"
					/>
				}
				notice={<VersionUpdateNotice />}
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
						<div
							className="flex items-center gap-2"
							data-dashboard-secondary-controls
						>
							{tab === "all" || tab === "releases" ? (
								<FeedPageLaneSelector
									value={effectivePageDefaultLane}
									onValueChange={(lane) => {
										setPageDefaultLane(lane);
										setSelectedLaneByKey({});
									}}
								/>
							) : null}
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
								{renderFeedPanel("all")}
							</TabsContent>
							<TabsContent value="releases" className="mt-0 min-w-0">
								{renderFeedPanel("releases")}
							</TabsContent>
							<TabsContent value="briefs" className="mt-0 min-w-0">
								<ReleaseDailyCard
									briefs={storyBriefs}
									selectedDate={selectedDate}
									busy={false}
									onGenerate={() => {}}
									onOpenRelease={setActiveReleaseId}
								/>
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
									briefs={storyBriefs}
									selectedDate={selectedDate}
									onSelectDate={(d) => setSelectedDate(d)}
								/>
							) : null}
							<InboxQuickList notifications={notifications} />
						</aside>
					</div>
				</Tabs>

				<ReleaseDetailCard
					releaseId={activeReleaseId}
					onClose={() => setActiveReleaseId(null)}
				/>

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
		</VersionMonitorStateProvider>
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
					"主工作区默认入口，验证顶部只保留一个主同步按钮，且 `全部` tab 会把历史日组默认折叠为日报。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const secondaryControls = canvasElement.querySelector(
			"[data-dashboard-secondary-controls]",
		);
		expect(secondaryControls).not.toBeNull();
		await expect(
			canvas.getByRole("button", { name: SYNC_ALL_LABEL }),
		).toBeVisible();
		await expect(
			canvas.getAllByRole("button", { name: SYNC_ALL_LABEL }),
		).toHaveLength(1);
		expect(
			secondaryControls?.querySelector(
				'button[aria-label="同步"], a[aria-label="同步"]',
			),
		).toBeNull();
		expect(secondaryControls?.textContent).not.toContain(SYNC_ALL_LABEL);
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

export const ReleasesGroupedByDay: Story = {
	args: {
		initialTab: "releases",
	},
	parameters: {
		docs: {
			description: {
				story:
					"`Releases` tab 只在日组切换处显示弱化分隔线：首组不画前置分隔，历史日组继续用日期与当日 Release 数提示边界。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.queryByText(/^2026-04-04\s+·\s+2 条 Release$/),
		).not.toBeInTheDocument();
		await expect(
			canvas.getByText(/^2026-04-03\s+·\s+2 条 Release$/),
		).toBeVisible();
		await expect(
			canvas.getByText(/^2026-04-02\s+·\s+1 条 Release$/),
		).toBeVisible();
		await expect(canvas.getByText(HISTORY_RAW_MARKER)).toBeVisible();
	},
};

export const EvidenceReleasesGroupedByDay: Story = {
	name: "Evidence / Releases Grouped By Day",
	args: {
		initialTab: "releases",
	},
	parameters: {
		docs: {
			disable: true,
		},
	},
};

export const AllHistoryCollapsedToBriefs: Story = {
	args: {
		initialTab: "all",
	},
	parameters: {
		docs: {
			description: {
				story:
					"`全部` tab 中，今天保持原始 Release feed；历史日组默认展示日报卡片，切到 releases 视图后只保留日期分界与原始列表。",
			},
		},
	},
	play: async ({ canvasElement, step }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText(/^2026-04-03\s+·\s+2 条 Release$/),
		).toBeVisible();
		await expect(
			canvas.getByText(/时间窗口（本地）：2026-04-03T08:00:00\+08:00/),
		).toBeVisible();
		await expect(
			canvas.queryByText(HISTORY_RAW_MARKER),
		).not.toBeInTheDocument();
		await step("expand historical releases", async () => {
			const historicalGroup = canvasElement.querySelector<HTMLElement>(
				'[data-feed-group-type="historical"][data-feed-brief-date="2026-04-04"]',
			);
			expect(historicalGroup).toBeTruthy();
			if (!historicalGroup) {
				throw new Error("Expected 2026-04-04 historical group to exist");
			}
			const beforeSlot = historicalGroup.querySelector<HTMLElement>(
				"[data-feed-day-action-slot]",
			);
			const expandButton =
				beforeSlot?.querySelector<HTMLButtonElement>("button");
			expect(beforeSlot).toBeTruthy();
			expect(expandButton).toBeTruthy();
			if (!beforeSlot || !expandButton) {
				throw new Error("Expected action slot and expand button to exist");
			}
			expect(expandButton.textContent?.trim()).toBe("Releases");
			const beforeSlotRect = beforeSlot.getBoundingClientRect();
			const beforeButtonRect = expandButton.getBoundingClientRect();
			await expandButton.click();
			await expect(canvas.getByText(HISTORY_RAW_MARKER)).toBeVisible();
			await expect(
				canvas.queryByText(/时间窗口（本地）：2026-04-03T08:00:00\+08:00/),
			).not.toBeInTheDocument();
			const afterSlot = historicalGroup.querySelector<HTMLElement>(
				"[data-feed-day-action-slot]",
			);
			const briefButton = afterSlot?.querySelector<HTMLButtonElement>("button");
			expect(afterSlot).toBeTruthy();
			expect(briefButton).toBeTruthy();
			if (!afterSlot || !briefButton) {
				throw new Error("Expected action slot and 日报 button after expand");
			}
			await expect(briefButton).toBeVisible();
			const afterSlotRect = afterSlot.getBoundingClientRect();
			const afterButtonRect = briefButton.getBoundingClientRect();
			expect(
				Math.abs(beforeSlotRect.top - afterSlotRect.top),
			).toBeLessThanOrEqual(1);
			expect(
				Math.abs(beforeSlotRect.left - afterSlotRect.left),
			).toBeLessThanOrEqual(1);
			expect(
				Math.abs(beforeSlotRect.width - afterSlotRect.width),
			).toBeLessThanOrEqual(1);
			expect(
				Math.abs(beforeSlotRect.height - afterSlotRect.height),
			).toBeLessThanOrEqual(1);
			expect(
				Math.abs(beforeButtonRect.top - afterButtonRect.top),
			).toBeLessThanOrEqual(1);
			expect(
				Math.abs(beforeButtonRect.left - afterButtonRect.left),
			).toBeLessThanOrEqual(1);
			expect(
				Math.abs(beforeButtonRect.width - afterButtonRect.width),
			).toBeLessThanOrEqual(1);
			expect(
				Math.abs(beforeButtonRect.height - afterButtonRect.height),
			).toBeLessThanOrEqual(1);
		});
	},
};

export const EvidenceAllHistoryCollapsedToBriefs: Story = {
	name: "Evidence / All History Collapsed To Briefs",
	args: {
		initialTab: "all",
	},
	parameters: {
		docs: {
			disable: true,
		},
	},
};

export const AllHistoryFallbackToReleaseCards: Story = {
	args: {
		initialTab: "all",
		briefs: [],
	},
	parameters: {
		docs: {
			description: {
				story:
					"历史日组没有对应日报时，`全部` tab 先显示日期分隔线 + 原始 Release 卡片；点击“生成日报”后进入 spinning + 占位日报，完成后切成真实日报。",
			},
		},
	},
	play: async ({ canvasElement, step }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("2026-04-02")).toBeVisible();
		await expect(canvas.getByText(FALLBACK_RAW_MARKER)).toBeVisible();
		await step("start generating fallback brief", async () => {
			await canvas.getByRole("button", { name: "生成日报" }).click();
			await expect(canvas.getByText("正在生成这一天的日报摘要…")).toBeVisible();
			await expect(
				canvas.getByRole("button", { name: "生成日报" }),
			).toBeDisabled();
			await expect(
				canvas.queryByText(FALLBACK_RAW_MARKER),
			).not.toBeInTheDocument();
			await expect(
				await canvas.findByText(/回补这一天的日报摘要/),
			).toBeVisible();
		});
	},
};

export const EvidenceAllHistoryFallbackToReleaseCards: Story = {
	name: "Evidence / All History Fallback To Release Cards",
	args: {
		initialTab: "all",
		briefs: [],
	},
	parameters: {
		docs: {
			disable: true,
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
					"从日报内部链接打开 release 详情时，详情应以模态弹窗显示，不再作为正文流里的第二张卡片出现。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const body = within(canvasElement.ownerDocument.body);
		await expect(await body.findByRole("dialog")).toBeVisible();
		await expect(await body.findByText(/翻译总览/)).toBeVisible();
		await expect(body.getByText(/变更波次 10/)).toBeVisible();
		expect(canvasElement.querySelector(".max-h-96")).toBeNull();
		expect(canvasElement.querySelector(".overflow-auto")).toBeNull();
		await expect(
			canvas.queryByRole("heading", { name: "Release 详情" }),
		).not.toBeInTheDocument();
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
		const secondaryControls = canvasElement.querySelector(
			"[data-dashboard-secondary-controls]",
		);
		expect(secondaryControls).not.toBeNull();
		const syncButton = canvas.getByRole("button", { name: SYNC_ALL_LABEL });
		await expect(
			canvas.getAllByRole("button", { name: SYNC_ALL_LABEL }),
		).toHaveLength(1);
		expect(secondaryControls?.textContent).not.toContain(SYNC_ALL_LABEL);
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
		await expect(canvas.getAllByRole("tab", { name: "智能" })).toHaveLength(12);
		await expect(
			canvas.getAllByRole("button", { name: "生成智能版" }),
		).toHaveLength(12);
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
			canvas.getByRole("heading", { name: "v2.0.01" }),
		).toBeVisible();
		await expect(
			canvas.getAllByRole("button", { name: "生成智能版" }),
		).toHaveLength(12);
		canvas.getAllByRole("tab", { name: "翻译" })[0]?.click();
		await expect(
			canvas.getByRole("heading", { name: "v2.0.01（中文）" }),
		).toBeVisible();
		await expect(canvas.getByText(/中文摘要 1/)).toBeVisible();
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

export const InboxLinksResolved: Story = {
	args: {
		initialTab: "inbox",
	},
	parameters: {
		docs: {
			description: {
				story:
					"Inbox 通知项直接消费后端给出的 html_url；缺失时由前端回退到 GitHub Inbox 或 repo 过滤页，不再拼接 thread URL。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const inboxLinks = canvas.getAllByRole("link");
		await expect(
			inboxLinks.some((link) =>
				link.getAttribute("href")?.includes("/notifications/thread/"),
			),
		).toBe(false);
		await expect(
			inboxLinks.some((link) =>
				link.getAttribute("href")?.includes("/pull/42"),
			),
		).toBe(true);
		await expect(
			inboxLinks.some((link) =>
				link
					.getAttribute("href")
					?.includes("/notifications?query=repo%3Aacme%2Frocket"),
			),
		).toBe(true);
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
		await expect(canvas.getByRole("heading", { name: "v3.0.0" })).toBeVisible();
		await canvas.getByRole("tab", { name: "翻译" }).click();
		await expect(canvas.getByText(/正文过长，无法直接翻译/)).toBeVisible();
		await expect(
			canvas.getByText(/建议直接打开 GitHub 阅读完整内容/),
		).toBeVisible();
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
			canvas.getByRole("heading", { name: "v4.1.0 · 后台智能预热" }),
		).toBeVisible();
		await expect(
			canvas.getByText(/后台同步已经预热智能版本摘要/),
		).toBeVisible();
		await canvas.getByRole("tab", { name: "翻译" }).click();
		await expect(
			canvas.getByRole("heading", { name: "v4.1.0（后台预热）" }),
		).toBeVisible();
		await expect(canvas.getByText(/后台同步已经预热翻译/)).toBeVisible();
	},
};

export const SmartReadyBody: Story = {
	args: {
		initialTab: "releases",
		feedMode: "smart-ready-body",
	},
	parameters: {
		docs: {
			description: {
				story:
					"智能总结可直接从 release body 提炼版本变化时，默认 tab 应展示中文要点，而不是直译正文。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "v5.0.0 · 版本变化" }),
		).toBeVisible();
		await expect(
			canvas.getByText(/新增 macOS \/ Windows 已签名桌面包/),
		).toBeVisible();
		await expect(canvas.getByRole("tab", { name: "智能" })).toHaveAttribute(
			"data-state",
			"active",
		);
	},
};

export const SmartReadyDiff: Story = {
	args: {
		initialTab: "releases",
		feedMode: "smart-ready-diff",
	},
	parameters: {
		docs: {
			description: {
				story:
					"当 release body 没有价值时，智能总结回退到 compare diff，并输出适合人类阅读的版本变化要点。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "v5.1.0 · 智能整理" }),
		).toBeVisible();
		await expect(canvas.getByText(/认证中间件与上传流程/)).toBeVisible();
	},
};

export const SmartLoading: Story = {
	args: {
		initialTab: "releases",
		feedMode: "smart-loading",
	},
	parameters: {
		docs: {
			description: {
				story:
					"智能 tab 缺数据时，正文继续回退显示原文，加载状态仅通过卡片内智能选项的呼吸效果表达。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole("heading", { name: "v5.2.0" })).toBeVisible();
		await expect(
			canvas.getByText(/Placeholder body for smart loading state/),
		).toBeVisible();
		const smartTrigger = canvasElement.querySelector(
			'[data-feed-lane-trigger="smart"][data-feed-lane-loading="true"]',
		);
		expect(smartTrigger).not.toBeNull();
	},
};

export const SmartRetryActionLoading: Story = {
	args: {
		initialTab: "releases",
		feedMode: "smart-retry-error",
	},
	parameters: {
		docs: {
			description: {
				story:
					"智能整理失败时点击重试，错误态按钮会立即进入旋转 loading，并在动作完成前保持禁用，避免重复点击。",
			},
		},
	},
	play: async ({ canvasElement, userEvent }) => {
		const canvas = within(canvasElement);
		const retryButton = canvas.getByRole("button", { name: "重试智能整理" });
		await expect(retryButton).toBeEnabled();
		await userEvent.click(retryButton);
		await expect(retryButton).toBeDisabled();
		await expect(retryButton).toHaveAttribute("aria-busy", "true");
		const icon = retryButton.querySelector("svg");
		expect(icon).not.toBeNull();
		expect(icon?.classList.contains("animate-spin")).toBe(true);
		await expect(
			canvas.getByText("智能整理失败", { exact: true }),
		).toBeVisible();
	},
};

export const PageDefaultLaneSwitching: Story = {
	args: {
		initialTab: "releases",
		feedMode: "default",
	},
	parameters: {
		docs: {
			description: {
				story:
					"页面级默认显示模式切换器会立即切换当前 release feed 的所有卡片；之后仍允许单卡再单独覆盖。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "v2.63.0 · 版本变化" }),
		).toBeVisible();
		await canvas.getByRole("button", { name: "翻译" }).click();
		await expect(
			canvas.getByRole("heading", { name: "v2.63.0（稳定版）" }),
		).toBeVisible();
		await canvas.getByRole("button", { name: "原文" }).click();
		await expect(
			canvas.getByRole("heading", { name: "v2.63.0" }),
		).toBeVisible();
	},
};

export const SmartInsufficient: Story = {
	args: {
		initialTab: "releases",
		feedMode: "smart-insufficient",
	},
	parameters: {
		docs: {
			description: {
				story:
					"如果 release body 与 compare diff 都缺乏有效版本说明，卡片退化为仅版本号的折叠态，不渲染 tabs 与正文区。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole("heading", { name: "v5.3.0" })).toBeVisible();
		await expect(
			canvas.queryByRole("tab", { name: "智能" }),
		).not.toBeInTheDocument();
		await expect(
			canvas.queryByText(/还没有智能版本变化摘要/),
		).not.toBeInTheDocument();
	},
};
