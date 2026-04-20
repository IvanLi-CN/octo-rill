import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useLayoutEffect, useState } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, userEvent, within } from "storybook/test";

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
import { Tabs, TabsContent } from "@/components/ui/tabs";
import { FeedPageLaneSelector } from "@/feed/FeedPageLaneSelector";
import { FeedGroupedList } from "@/feed/FeedGroupedList";
import { FeedItemCard } from "@/feed/FeedItemCard";
import {
	DEFAULT_PAGE_LANE,
	resolveDisplayLaneForFeed,
	resolvePreferredLaneForItem,
} from "@/feed/laneOptions";
import type {
	FeedItem,
	FeedLane,
	ReleaseFeedItem,
	SocialFeedItem,
} from "@/feed/types";
import { isSocialFeedItem } from "@/feed/types";
import type { FeedLoadError } from "@/feed/useFeed";
import { InboxList } from "@/inbox/InboxList";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { InternalLink } from "@/lib/internalNavigation";
import { AppShell } from "@/layout/AppShell";
import { useMediaQuery } from "@/lib/useMediaQuery";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";
import type { RepoVisual } from "@/lib/repoVisual";
import {
	DashboardMobileControlBand,
	type DashboardTab as Tab,
	DashboardTabsList,
} from "@/pages/DashboardControlBand";
import { DashboardHeader } from "@/pages/DashboardHeader";
import { buildSettingsHref, buildSettingsSearch } from "@/settings/routeState";
import { BriefListCard } from "@/sidebar/BriefListCard";
import {
	InboxQuickList,
	type NotificationItem,
} from "@/sidebar/InboxQuickList";
import { type BriefItem, ReleaseDailyCard } from "@/sidebar/ReleaseDailyCard";
import { ReleaseDetailCard } from "@/sidebar/ReleaseDetailCard";
import { VersionMonitorStateProvider } from "@/version/versionMonitor";

type FeedMode =
	| "default"
	| "visible-window-queued"
	| "visible-window-settling"
	| "long-body-translation"
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
const STORYBOOK_VERSION_STATE = {
	loadedVersion: "v2.4.6",
	availableVersion: null,
	hasUpdate: false,
	refreshPage: () => {},
} as const;
const DASHBOARD_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	dashboardMobileDivider375: {
		name: "Dashboard mobile divider 375x667",
		styles: {
			height: "667px",
			width: "375px",
		},
		type: "mobile",
	},
	dashboardMobile390: {
		name: "Dashboard mobile 390x844",
		styles: {
			height: "844px",
			width: "390px",
		},
		type: "mobile",
	},
	dashboardMobile375: {
		name: "Dashboard mobile 375x667",
		styles: {
			height: "667px",
			width: "375px",
		},
		type: "mobile",
	},
} as const;
const HISTORY_RAW_MARKER = "raw-history-guardrails-marker";
const FALLBACK_RAW_MARKER = "raw-fallback-release-marker";
const OWNER_RELEASE_OPT_IN_TITLE = "v2.64.0 · owner release opt-in";

function dispatchSyntheticTouchEvent(
	target: HTMLElement,
	type: "touchstart" | "touchmove",
	offsetX = 0,
	offsetY = 0,
) {
	const rect = target.getBoundingClientRect();
	const touchPoint = {
		clientX: rect.left + rect.width / 2 + offsetX,
		clientY: rect.top + rect.height / 2 + offsetY,
	};
	const event = new Event(type, {
		bubbles: true,
		cancelable: true,
	}) as Event & {
		touches: Array<typeof touchPoint>;
		targetTouches: Array<typeof touchPoint>;
		changedTouches: Array<typeof touchPoint>;
	};

	Object.defineProperty(event, "touches", { value: [touchPoint] });
	Object.defineProperty(event, "targetTouches", { value: [touchPoint] });
	Object.defineProperty(event, "changedTouches", { value: [touchPoint] });
	target.dispatchEvent(event);
}

function makeBrief(
	brief: Omit<
		BriefItem,
		| "id"
		| "effective_time_zone"
		| "effective_local_boundary"
		| "release_count"
		| "release_ids"
	> &
		Partial<
			Pick<
				BriefItem,
				| "id"
				| "effective_time_zone"
				| "effective_local_boundary"
				| "release_count"
				| "release_ids"
			>
		>,
): BriefItem {
	return {
		id: brief.id ?? `brief-${brief.date}`,
		effective_time_zone: brief.effective_time_zone ?? "Asia/Shanghai",
		effective_local_boundary: brief.effective_local_boundary ?? "08:00",
		release_count: brief.release_count ?? brief.release_ids?.length ?? 0,
		release_ids: brief.release_ids ?? [],
		...brief,
	};
}

function socialPreviewDataUrl(title: string, accent: string, body: string) {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="640" viewBox="0 0 1280 640"><defs><linearGradient id="g" x1="0" x2="1" y1="0" y2="1"><stop offset="0%" stop-color="${accent}"/><stop offset="100%" stop-color="#0f172a"/></linearGradient></defs><rect width="1280" height="640" rx="48" fill="url(#g)"/><rect x="72" y="72" width="1136" height="496" rx="36" fill="rgba(15,23,42,0.18)" stroke="rgba(255,255,255,0.18)" stroke-width="4"/><text x="120" y="228" font-family="Inter,Arial,sans-serif" font-size="84" font-weight="800" fill="#ffffff">${title}</text><text x="120" y="334" font-family="Inter,Arial,sans-serif" font-size="40" font-weight="600" fill="rgba(255,255,255,0.84)">${body}</text></svg>`,
	)}`;
}

function avatarDataUrl(
	label: string,
	background: string,
	foreground = "#ffffff",
) {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240"><rect width="240" height="240" rx="120" fill="${background}"/><text x="120" y="132" font-family="Inter,Arial,sans-serif" font-size="44" font-weight="700" text-anchor="middle" fill="${foreground}">${label}</text></svg>`,
	)}`;
}

function githubAvatarUrl(username: string, size = 96) {
	return `https://github.com/${username}.png?size=${size}`;
}

function githubAvatarUrlByUserId(userId: number, size = 96) {
	return `https://avatars.githubusercontent.com/u/${userId}?v=4&size=${size}`;
}

const PROJECT_OWNER_LOGIN = "IvanLi-CN" as const;
const PROJECT_OWNER_GITHUB_USER_ID = 30215105 as const;
const PROJECT_REPO_FULL_NAME = "IvanLi-CN/octo-rill" as const;
const PROJECT_REPO_URL = "https://github.com/IvanLi-CN/octo-rill" as const;

const STORYBOOK_VIEWER = {
	login: PROJECT_OWNER_LOGIN,
	avatar_url: githubAvatarUrlByUserId(PROJECT_OWNER_GITHUB_USER_ID),
	html_url: "https://github.com/IvanLi-CN",
} as const;

const repoVisualFixtures: Record<
	"social" | "avatar" | "text",
	RepoVisual | null
> = {
	social: {
		owner_avatar_url: githubAvatarUrlByUserId(PROJECT_OWNER_GITHUB_USER_ID),
		open_graph_image_url: socialPreviewDataUrl(
			"Rocket Release",
			"#2563eb",
			"custom social preview",
		),
		uses_custom_open_graph_image: true,
	},
	avatar: {
		owner_avatar_url: githubAvatarUrl("openai"),
		open_graph_image_url: null,
		uses_custom_open_graph_image: false,
	},
	text: null,
};
function feedItemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

function defaultLaneForItem(
	item: FeedItem,
	pageDefaultLane: FeedLane,
	allowItemOverride = true,
	selectedLane?: FeedLane,
): FeedLane {
	if (allowItemOverride && selectedLane) {
		return resolvePreferredLaneForItem(item, selectedLane);
	}
	return resolvePreferredLaneForItem(item, pageDefaultLane);
}

function buildFeedItem(
	id: string,
	overrides?: Partial<ReleaseFeedItem>,
): ReleaseFeedItem {
	return {
		kind: "release",
		ts: "2026-02-21T08:05:00Z",
		id,
		repo_full_name: PROJECT_REPO_FULL_NAME,
		repo_visual: repoVisualFixtures.social,
		title: `v${id}`,
		body: "- This is a stable release\n- Includes performance improvements\n- Please update and rebuild images",
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `${PROJECT_REPO_URL}/releases/tag/${id}`,
		unread: null,
		actor: null,
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
			html_url: `${PROJECT_REPO_URL}/releases/tag/v2.63.0`,
			repo_visual: repoVisualFixtures.social,
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
			repo_visual: repoVisualFixtures.avatar,
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
			html_url: `${PROJECT_REPO_URL}/releases/tag/nightly-guardrails`,
		}),
		buildFeedItem("10004", {
			ts: "2026-04-03T21:30:00+08:00",
			repo_full_name: "acme/satellite",
			repo_visual: repoVisualFixtures.avatar,
			title: "oauth action bubble polish",
			body: "- stabilize oauth actions\n- dedupe previews\n- align hover states",
			html_url:
				"https://github.com/acme/satellite/releases/tag/oauth-action-bubble",
		}),
		buildFeedItem("10005", {
			ts: "2026-04-03T06:20:00+08:00",
			repo_full_name: "acme/fleet",
			repo_visual: repoVisualFixtures.text,
			title: "fallback lane release",
			body: `- ${FALLBACK_RAW_MARKER}\n- no brief available for this day\n- keep original release cards visible`,
			html_url:
				"https://github.com/acme/fleet/releases/tag/fallback-lane-release",
		}),
	];
}

function makeReactionCompactFeed(): FeedItem[] {
	return makeMockFeed().map((item, index) =>
		item.kind === "release" && index === 0
			? {
					...item,
					reactions: {
						counts: {
							plus1: 2,
							laugh: 1,
							heart: 3,
							hooray: 1,
							rocket: 2,
							eyes: 0,
						},
						viewer: {
							plus1: true,
							laugh: false,
							heart: false,
							hooray: false,
							rocket: false,
							eyes: false,
						},
						status: "ready",
					},
				}
			: item,
	);
}

function makeMobileDayDividerProofFeed(): FeedItem[] {
	return [
		buildFeedItem("mobile-divider-current", {
			ts: "2026-04-04T16:42:00+08:00",
			repo_full_name: "acme/rocket-mobile",
			repo_visual: repoVisualFixtures.social,
			title: "mobile divider current day",
			body: "- keep reaction footer visible\n- preserve grouped-feed divider readability\n- leave space before the next day header",
			html_url:
				"https://github.com/acme/rocket-mobile/releases/tag/mobile-divider-current",
			reactions: {
				counts: {
					plus1: 4,
					laugh: 1,
					heart: 2,
					hooray: 1,
					rocket: 1,
					eyes: 0,
				},
				viewer: {
					plus1: true,
					laugh: false,
					heart: false,
					hooray: false,
					rocket: false,
					eyes: false,
				},
				status: "ready",
			},
		}),
		buildFeedItem("mobile-divider-history", {
			ts: "2026-04-03T19:18:00+08:00",
			repo_full_name: "acme/rocket-mobile",
			repo_visual: repoVisualFixtures.avatar,
			title: "mobile divider previous day",
			body: "- historical release without brief\n- render generate button beside the day header on narrow screens",
			html_url:
				"https://github.com/acme/rocket-mobile/releases/tag/mobile-divider-history",
		}),
	];
}

function makeMobileMixedActivityDividerProofFeed(): FeedItem[] {
	return [
		buildFeedItem("mobile-mixed-current", {
			ts: "2026-04-04T16:42:00+08:00",
			repo_full_name: "acme/rocket-mobile",
			repo_visual: repoVisualFixtures.social,
			title: "mobile mixed divider current day",
			body: "- keep reaction footer visible\n- next divider also includes mixed activity counts\n- list action must not collide with the label",
			html_url:
				"https://github.com/acme/rocket-mobile/releases/tag/mobile-mixed-current",
			reactions: {
				counts: {
					plus1: 4,
					laugh: 1,
					heart: 2,
					hooray: 1,
					rocket: 1,
					eyes: 0,
				},
				viewer: {
					plus1: true,
					laugh: false,
					heart: false,
					hooray: false,
					rocket: false,
					eyes: false,
				},
				status: "ready",
			},
		}),
		buildFeedItem("mobile-mixed-history-release-1", {
			ts: "2026-04-03T23:10:00+08:00",
			repo_full_name: "acme/rocket-mobile",
			repo_visual: repoVisualFixtures.social,
			title: "nightly guardrails",
			body: "- first release inside the mixed-activity historical group",
			html_url:
				"https://github.com/acme/rocket-mobile/releases/tag/nightly-guardrails",
		}),
		buildFeedItem("mobile-mixed-history-release-2", {
			ts: "2026-04-03T21:30:00+08:00",
			repo_full_name: "acme/satellite-mobile",
			repo_visual: repoVisualFixtures.avatar,
			title: "oauth action bubble polish",
			body: "- second release inside the mixed-activity historical group",
			html_url:
				"https://github.com/acme/satellite-mobile/releases/tag/oauth-action-bubble",
		}),
		buildRepoStarItem("mobile-mixed-history-star", {
			ts: "2026-04-03T22:10:00+08:00",
			repo_full_name: "acme/satellite-mobile",
			repo_visual: repoVisualFixtures.avatar,
			actor: {
				login: "linus",
				avatar_url: githubAvatarUrl("linus"),
				html_url: "https://github.com/linus",
			},
			html_url: "https://github.com/linus",
		}),
		buildFollowerItem("mobile-mixed-history-follow", {
			ts: "2026-04-03T18:45:00+08:00",
			actor: {
				login: "yyx990803",
				avatar_url: githubAvatarUrl("yyx990803"),
				html_url: "https://github.com/yyx990803",
			},
			html_url: "https://github.com/yyx990803",
		}),
	];
}

const MOBILE_MIXED_ACTIVITY_DIVIDER_PROOF_BRIEFS: BriefItem[] = [
	makeBrief({
		id: "brief-mobile-mixed-2026-04-04",
		date: "2026-04-04",
		window_start: "2026-04-03T08:00:00+08:00",
		window_end: "2026-04-04T08:00:00+08:00",
		release_count: 2,
		release_ids: [
			"mobile-mixed-history-release-1",
			"mobile-mixed-history-release-2",
		],
		content_markdown:
			"## 概览\n\n- 时间窗口（本地）：2026-04-03T08:00:00+08:00 → 2026-04-04T08:00:00+08:00\n- 更新项目：4 个\n- Release：2 条（预发布 0 条）\n- 其余动态：2 条\n",
		created_at: "2026-04-04T08:00:03+08:00",
	}),
];

function buildRepoStarItem(
	id: string,
	overrides?: Partial<SocialFeedItem>,
): SocialFeedItem {
	return {
		kind: "repo_star_received",
		ts: "2026-04-04T14:20:00+08:00",
		id,
		repo_full_name: PROJECT_REPO_FULL_NAME,
		repo_visual: repoVisualFixtures.social,
		title: null,
		body: null,
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: "https://github.com/torvalds",
		unread: null,
		actor: {
			login: "torvalds",
			avatar_url: githubAvatarUrl("torvalds"),
			html_url: "https://github.com/torvalds",
		},
		translated: null,
		smart: null,
		reactions: null,
		...overrides,
	};
}

function buildFollowerItem(
	id: string,
	overrides?: Partial<SocialFeedItem>,
): SocialFeedItem {
	return {
		kind: "follower_received",
		ts: "2026-04-04T13:45:00+08:00",
		id,
		repo_full_name: null,
		repo_visual: null,
		title: null,
		body: null,
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: "https://github.com/gaearon",
		unread: null,
		actor: {
			login: "gaearon",
			avatar_url: githubAvatarUrl("gaearon"),
			html_url: "https://github.com/gaearon",
		},
		translated: null,
		smart: null,
		reactions: null,
		...overrides,
	};
}

function makeMixedSocialFeed(): FeedItem[] {
	return [
		...makeMockFeed(),
		buildRepoStarItem("star-10001", {
			ts: "2026-04-04T16:06:00+08:00",
			repo_full_name: PROJECT_REPO_FULL_NAME,
		}),
		buildFollowerItem("follow-10001", {
			ts: "2026-04-04T14:48:00+08:00",
		}),
		buildRepoStarItem("star-10002", {
			ts: "2026-04-03T22:10:00+08:00",
			repo_full_name: "acme/satellite",
			repo_visual: repoVisualFixtures.avatar,
			actor: {
				login: "linus",
				avatar_url: githubAvatarUrl("linus"),
				html_url: "https://github.com/linus",
			},
			html_url: "https://github.com/linus",
		}),
		buildFollowerItem("follow-fallback", {
			ts: "2026-04-03T06:45:00+08:00",
			actor: {
				login: "yyx990803",
				avatar_url: githubAvatarUrl("yyx990803"),
				html_url: "https://github.com/yyx990803",
			},
			html_url: "https://github.com/yyx990803",
		}),
	];
}

function makeOwnReleaseOptInFeed(includeOwnRelease: boolean): FeedItem[] {
	const items: FeedItem[] = [
		buildFeedItem("own-release-external", {
			ts: "2026-04-04T17:32:00+08:00",
			repo_full_name: "lobehub/lobe-chat",
			repo_visual: repoVisualFixtures.avatar,
			title: "桌面版 Stable v2.1.47",
			body: "- external starred release\n- stays visible regardless of owner opt-in",
			html_url: "https://github.com/lobehub/lobe-chat/releases/tag/v2.1.47",
		}),
		buildRepoStarItem("own-release-star", {
			ts: "2026-04-04T16:42:00+08:00",
			repo_full_name: "lobehub/lobe-chat",
			repo_visual: repoVisualFixtures.avatar,
			actor: {
				login: "gaearon",
				avatar_url: githubAvatarUrl("gaearon"),
				html_url: "https://github.com/gaearon",
			},
			html_url: "https://github.com/gaearon",
		}),
	];

	if (includeOwnRelease) {
		items.unshift(
			buildFeedItem("own-release-owner", {
				ts: "2026-04-04T18:12:00+08:00",
				repo_full_name: PROJECT_REPO_FULL_NAME,
				repo_visual: repoVisualFixtures.social,
				title: OWNER_RELEASE_OPT_IN_TITLE,
				body: "- owner-only release\n- visible only when `我的发布` is enabled",
				html_url: `${PROJECT_REPO_URL}/releases/tag/v2.64.0`,
				translated: {
					lang: "zh-CN",
					status: "ready",
					title: "v2.64.0 · 自有仓库发布",
					summary: "- 开启“我的发布”后进入发布流\n- 不会污染真实加星列表",
				},
				smart: {
					lang: "zh-CN",
					status: "ready",
					title: "v2.64.0 · 版本变化",
					summary:
						"- 仅 release 能见面扩展到 owner repo\n- Feed/详情/日报使用统一可见性来源",
				},
			}),
		);
	}

	return items;
}

function makeMobileCompactSocialFeed(): FeedItem[] {
	return [
		buildRepoStarItem("mobile-star-proof", {
			ts: "2026-04-04T18:06:00+08:00",
			repo_full_name: PROJECT_REPO_FULL_NAME,
			repo_visual: {
				owner_avatar_url: githubAvatarUrlByUserId(PROJECT_OWNER_GITHUB_USER_ID),
				open_graph_image_url: null,
				uses_custom_open_graph_image: false,
			},
			actor: {
				login: "frontend-systems-maintainer",
				avatar_url: avatarDataUrl("MS", "#7c3aed"),
				html_url: "https://github.com/frontend-systems-maintainer",
			},
			html_url: "https://github.com/frontend-systems-maintainer",
		}),
		buildFollowerItem("mobile-follow-proof", {
			ts: "2026-04-04T17:48:00+08:00",
			actor: {
				login: "design-ops-collaborator",
				avatar_url: avatarDataUrl("MF", "#0f766e"),
				html_url: "https://github.com/design-ops-collaborator",
			},
			html_url: "https://github.com/design-ops-collaborator",
		}),
		buildFeedItem("mobile-proof-release", {
			ts: "2026-04-04T16:12:00+08:00",
			title: "移动端社交卡片重设计验证版",
		}),
	];
}

function makeMobileSocialEdgeCaseFeed(): FeedItem[] {
	return [
		buildRepoStarItem("mobile-edge-right-long", {
			ts: "2026-04-04T18:30:00+08:00",
			repo_full_name: "IvanLi-CN/mobile-dashboard-social-adaptive-case",
			repo_visual: {
				owner_avatar_url: githubAvatarUrlByUserId(PROJECT_OWNER_GITHUB_USER_ID),
				open_graph_image_url: null,
				uses_custom_open_graph_image: false,
			},
			actor: {
				login: "ms",
				avatar_url: avatarDataUrl("MS", "#7c3aed"),
				html_url: "https://github.com/ms",
			},
			html_url: "https://github.com/ms",
		}),
		buildFollowerItem("mobile-edge-left-long", {
			ts: "2026-04-04T18:12:00+08:00",
			actor: {
				login: "design-ops-collaborator-case",
				avatar_url: avatarDataUrl("MF", "#0f766e"),
				html_url: "https://github.com/design-ops-collaborator-case",
			},
			html_url: "https://github.com/design-ops-collaborator-case",
		}),
		buildRepoStarItem("mobile-edge-bilateral-long", {
			ts: "2026-04-04T17:54:00+08:00",
			repo_full_name:
				"IvanLi-CN/mobile-dashboard-social-activity-feed-bilateral-proof",
			repo_visual: {
				owner_avatar_url: githubAvatarUrlByUserId(PROJECT_OWNER_GITHUB_USER_ID),
				open_graph_image_url: null,
				uses_custom_open_graph_image: false,
			},
			actor: {
				login: "frontend-systems-maintainer-centered-proof",
				avatar_url: avatarDataUrl("FL", "#2563eb"),
				html_url:
					"https://github.com/frontend-systems-maintainer-centered-proof",
			},
			html_url: "https://github.com/frontend-systems-maintainer-centered-proof",
		}),
		buildFollowerItem("mobile-edge-balanced", {
			ts: "2026-04-04T17:36:00+08:00",
			actor: {
				login: "design-ops-collaborator",
				avatar_url: avatarDataUrl("MF", "#0f766e"),
				html_url: "https://github.com/design-ops-collaborator",
			},
			html_url: "https://github.com/design-ops-collaborator",
		}),
	];
}

function makeMobileShortFollowerFeed(): FeedItem[] {
	return [
		["mobile-short-follow-brutany", "brutany", "#d97706"],
		["mobile-short-follow-zhenyuan", "zhenyuanwang46-droid", "#7c3aed"],
		["mobile-short-follow-pseudocodes", "pseudocodes", "#0f766e"],
		["mobile-short-follow-zyou", "zyou9724-creator", "#60a5fa"],
		["mobile-short-follow-mrlrk", "mrlrk82", "#c2410c"],
	].map(([id, login, color]) =>
		buildFollowerItem(id, {
			ts: "2026-04-04T17:12:00+08:00",
			actor: {
				login,
				avatar_url: avatarDataUrl(login.slice(0, 2).toUpperCase(), color),
				html_url: `https://github.com/${login}`,
			},
			html_url: `https://github.com/${login}`,
		}),
	);
}

function SocialCardsMatrixPreview(props: {
	items: SocialFeedItem[];
	currentViewer?: typeof STORYBOOK_VIEWER;
}) {
	const { items, currentViewer = STORYBOOK_VIEWER } = props;
	return (
		<div className="bg-background min-h-screen px-3 py-4">
			<div className="mx-auto w-full max-w-[390px] space-y-3">
				{items.map((item) => (
					<FeedItemCard
						key={item.id}
						item={item}
						currentViewer={currentViewer}
						activeLane={DEFAULT_PAGE_LANE}
						isTranslating={false}
						isSmartGenerating={false}
						isReactionBusy={false}
						reactionError={null}
						onSelectLane={() => {}}
						onTranslateNow={() => {}}
						onSmartNow={() => {}}
						onToggleReaction={() => {}}
					/>
				))}
			</div>
		</div>
	);
}

function assertInlineSocialCardLayout(card: HTMLElement) {
	const row = card.querySelector<HTMLElement>("[data-social-card-row]");
	const actor = card.querySelector<HTMLElement>(
		'[data-social-card-segment="actor"]',
	);
	const action = card.querySelector<HTMLElement>(
		'[data-social-card-segment="action"]',
	);
	const target = card.querySelector<HTMLElement>(
		'[data-social-card-segment="target"]',
	);
	expect(card.dataset.socialCardLayout).toBe("inline-compact");
	expect(row).toBeTruthy();
	expect(actor).toBeTruthy();
	expect(action).toBeTruthy();
	expect(target).toBeTruthy();
	if (!row || !actor || !action || !target) {
		throw new Error("Expected social card row and segments to exist");
	}
	const actorGroup =
		actor.querySelector<HTMLElement>(
			'[data-social-card-entity-group="actor"]',
		) ?? actor;
	const targetGroup =
		target.querySelector<HTMLElement>(
			'[data-social-card-entity-group="target"]',
		) ?? target;

	const rowRect = row.getBoundingClientRect();
	const actorRect = actorGroup.getBoundingClientRect();
	const actionRect = action.getBoundingClientRect();
	const targetRect = targetGroup.getBoundingClientRect();
	const rowCenterY = rowRect.top + rowRect.height / 2;
	const centers = [actorRect, actionRect, targetRect].map(
		(rect) => rect.top + rect.height / 2,
	);
	for (const centerY of centers) {
		expect(Math.abs(centerY - rowCenterY)).toBeLessThan(rowRect.height * 0.32);
	}

	expect(actorRect.left).toBeLessThan(actionRect.left);
	expect(actionRect.left).toBeLessThan(targetRect.left);
	expect(row.scrollWidth - row.clientWidth).toBeLessThanOrEqual(1);
	expect(actorRect.left - rowRect.left).toBeLessThanOrEqual(14);
	expect(rowRect.right - targetRect.right).toBeLessThanOrEqual(14);
	const actionCenterX = actionRect.left + actionRect.width / 2;
	const rowCenterX = rowRect.left + rowRect.width / 2;
	if (row.dataset.socialCardBalanceMode !== "adaptive") {
		expect(Math.abs(actionCenterX - rowCenterX)).toBeLessThanOrEqual(12);
	}
	expect(action.textContent?.trim() ?? "").toBe("");

	for (const label of card.querySelectorAll<HTMLElement>(
		"[data-social-card-primary]",
	)) {
		const style = window.getComputedStyle(label);
		expect(style.whiteSpace).toBe("nowrap");
		expect(style.overflow).toBe("hidden");
		expect(label.dataset.socialCardPrimaryFull).toBeTruthy();
		expect(label.dataset.socialCardPrimaryMobile).toBeTruthy();
	}
}

function countVisibleGithubLinks(card: HTMLElement) {
	return Array.from(
		card.querySelectorAll<HTMLAnchorElement>('a[href^="https://github.com/"]'),
	).filter((link) => window.getComputedStyle(link).display !== "none").length;
}

function makeVisibleWindowFeed(
	mode: "visible-window-queued" | "visible-window-settling",
): FeedItem[] {
	const items: ReleaseFeedItem[] = Array.from({ length: 12 }, (_, index) => {
		const seq = `${index + 1}`.padStart(2, "0");
		return buildFeedItem(`200${seq}`, {
			ts: `2026-02-21T0${(index % 6) + 1}:15:00Z`,
			title: `v2.0.${seq}`,
			html_url: `${PROJECT_REPO_URL}/releases/tag/v2.0.${seq}`,
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

function makeLongBodyTranslationFeed(): FeedItem[] {
	return [
		buildFeedItem("30001", {
			title: "v3.0.0（长正文）",
			body: [
				"- 长正文会在列表里保留截断原文，避免卡片失控膨胀",
				"- 切到翻译标签后会走 release_detail 分块翻译链路",
				"- sync 后的后台预热也会复用同一套长正文缓存结果",
			].join("\n"),
			body_truncated: true,
			translated: {
				lang: "zh-CN",
				status: "ready",
				title: "v3.0.0（长正文）",
				summary: [
					"- 长正文会走完整 release 详情翻译链路，不再直接拒绝",
					"- 列表卡片继续展示截断原文，翻译标签展示完整分块译文",
					"- 后台预热与手动触发共用同一份长正文缓存",
				].join("\n"),
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
	makeBrief({
		id: "brief-2026-04-04",
		date: "2026-04-04",
		window_start: "2026-04-03T08:00:00+08:00",
		window_end: "2026-04-04T08:00:00+08:00",
		release_count: 2,
		release_ids: ["10003", "10004"],
		content_markdown:
			"## 项目更新\n\n### [acme/rocket](https://github.com/acme/rocket)\n\n- [nightly guardrails](/?tab=briefs&release=10003) · 2026-04-03T23:10:00+08:00 · [GitHub Release](https://github.com/acme/rocket/releases/tag/nightly-guardrails)\n  - 收敛上传守卫，避免批量发布时的顺序漂移。\n  - 相关链接：[4d8f459](https://github.com/acme/rocket/commit/4d8f459e7869d3e0b57fafe1b7a9034cb9b2d999)\n\n### [acme/satellite](https://github.com/acme/satellite)\n\n- [oauth action bubble polish](/?tab=briefs&release=10004) · 2026-04-03T21:30:00+08:00 · [GitHub Release](https://github.com/acme/satellite/releases/tag/oauth-action-bubble)\n  - 统一 oauth 批量操作气泡与 hover 态。\n  - 相关链接：[#13840](https://github.com/acme/satellite/pull/13840)\n\n## 获星与关注\n\n### 获星\n\n- [acme/rocket](https://github.com/acme/rocket)：[@yyx990803](https://github.com/yyx990803)、[@gaearon](https://github.com/gaearon)\n\n### 关注\n\n- [@antfu](https://github.com/antfu)、[@sindresorhus](https://github.com/sindresorhus)\n",
		created_at: "2026-04-04T08:00:03+08:00",
	}),
];

const projectOnlyBriefs: BriefItem[] = [
	makeBrief({
		id: "brief-project-only-2026-04-05",
		date: "2026-04-05",
		window_start: "2026-04-04T08:00:00+08:00",
		window_end: "2026-04-05T08:00:00+08:00",
		release_count: 1,
		release_ids: ["10006"],
		content_markdown:
			"## 项目更新\n\n### [acme/relay](https://github.com/acme/relay)\n\n- [stability lane release](/?tab=briefs&release=10006) · 2026-04-04T20:15:00+08:00 · [GitHub Release](https://github.com/acme/relay/releases/tag/stability-lane-release)\n  - 收敛日报刷新流程，避免历史摘要与卡片重复呈现。\n  - 相关链接：[#13888](https://github.com/acme/relay/pull/13888)\n",
		created_at: "2026-04-05T08:00:03+08:00",
	}),
];

const longBriefMarkdown = [
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
	"",
	"## 获星与关注",
	"",
	"### 获星",
	"",
	"- [acme/rocket](https://github.com/acme/rocket)：[@alice](https://github.com/alice)、[@bob](https://github.com/bob)、[@carol](https://github.com/carol) 等 7 人",
	"- [acme/atlas](https://github.com/acme/atlas)：[@dave](https://github.com/dave)、[@erin](https://github.com/erin)",
	"",
	"### 关注",
	"",
	"- [@yyx990803](https://github.com/yyx990803)、[@gaearon](https://github.com/gaearon)、[@antfu](https://github.com/antfu) 等 8 人",
].join("\n");

const longBriefs: BriefItem[] = [
	makeBrief({
		id: "brief-long-2026-04-04",
		date: "2026-04-04",
		window_start: "2026-04-03T08:00:00+08:00",
		window_end: "2026-04-04T08:00:00+08:00",
		release_count: 8,
		release_ids: [
			LONG_BRIEF_RELEASE_ID,
			"777003",
			"777004",
			"777005",
			"777006",
			"777007",
			"777008",
			"777009",
		],
		content_markdown: longBriefMarkdown,
		created_at: "2026-04-04T08:00:35+08:00",
	}),
	mockBriefs[0],
];

const generatedBriefTemplates: Record<string, BriefItem> = {
	"2026-04-03": makeBrief({
		id: "brief-2026-04-03",
		date: "2026-04-03",
		window_start: "2026-04-02T08:00:00+08:00",
		window_end: "2026-04-03T08:00:00+08:00",
		release_count: 1,
		release_ids: ["10005"],
		content_markdown:
			"## 项目更新\n\n### [acme/fleet](https://github.com/acme/fleet)\n\n- [fallback lane release](/?tab=briefs&release=10005) · 2026-04-03T06:20:00+08:00 · [GitHub Release](https://github.com/acme/fleet/releases/tag/fallback-lane-release)\n  - 回补这一天的日报摘要，用来验证按天生成后的展示切换。\n\n## 获星与关注\n\n### 关注\n\n- [@withastro](https://github.com/withastro)\n",
		created_at: "2026-04-03T08:00:03+08:00",
	}),
};

const longReleaseDetail: ReleaseDetailResponse = {
	release_id: LONG_BRIEF_RELEASE_ID,
	repo_full_name: "acme/rocket",
	repo_visual: repoVisualFixtures.social,
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

function isFeedBackedTab(
	value: Tab,
): value is Extract<Tab, "all" | "releases" | "stars" | "followers"> {
	return (
		value === "all" ||
		value === "releases" ||
		value === "stars" ||
		value === "followers"
	);
}

function DashboardPreview(props: {
	initialTab?: Tab;
	initialPatDialogOpen?: boolean;
	syncingAll?: boolean;
	showEmptyInbox?: boolean;
	emptyState?: "content" | "auto-sync" | "no-cache";
	feedMode?: FeedMode;
	briefs?: BriefItem[];
	feedItems?: FeedItem[];
	dailyBoundaryLocal?: string;
	dailyBoundaryTimeZone?: string;
	dailyBoundaryUtcOffsetMinutes?: number;
	now?: Date;
	initialReleaseId?: string | null;
	releaseDetail?: ReleaseDetailResponse | null;
	showFooter?: boolean;
	deferredFeedTabs?: Tab[];
	initialFeedTabLoading?: Tab | null;
	deferredFeedLoadDelayMs?: number;
	feedError?: FeedLoadError | null;
	reactionErrorByKey?: Record<string, string>;
}) {
	const {
		initialTab = "all",
		initialPatDialogOpen = false,
		syncingAll = false,
		showEmptyInbox = false,
		emptyState = "content",
		feedMode = "default",
		briefs = mockBriefs,
		feedItems,
		dailyBoundaryLocal = STORYBOOK_DAILY_BOUNDARY,
		dailyBoundaryTimeZone = STORYBOOK_DAILY_BOUNDARY_TIME_ZONE,
		dailyBoundaryUtcOffsetMinutes = STORYBOOK_DAILY_BOUNDARY_UTC_OFFSET_MINUTES,
		now = STORYBOOK_NOW,
		initialReleaseId = null,
		releaseDetail = null,
		showFooter = true,
		deferredFeedTabs = [],
		initialFeedTabLoading = null,
		deferredFeedLoadDelayMs = 1400,
		feedError = null,
		reactionErrorByKey = {},
	} = props;
	useStorybookReleaseDetailMock(releaseDetail);
	const [storyBriefs, setStoryBriefs] = useState<BriefItem[]>(briefs);

	useEffect(() => {
		setStoryBriefs(briefs);
	}, [briefs]);

	const items =
		emptyState !== "content"
			? []
			: feedItems
				? feedItems
				: feedMode === "default"
					? makeMockFeed()
					: feedMode === "long-body-translation"
						? makeLongBodyTranslationFeed()
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
		feedMode === "long-body-translation" ||
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
	const [selectedBriefId, setSelectedBriefId] = useState<string | null>(
		briefs[0]?.id ?? null,
	);
	const [activeReleaseId, setActiveReleaseId] = useState<string | null>(
		initialReleaseId,
	);
	const allowReleaseItemLaneOverride = useMediaQuery("(min-width: 640px)");
	const hasDesktopSidebar = useMediaQuery("(min-width: 768px)");
	const [pendingFeedTab, setPendingFeedTab] = useState<Tab | null>(
		initialFeedTabLoading,
	);
	const [resolvedDeferredFeedTabs, setResolvedDeferredFeedTabs] = useState<
		Set<Tab>
	>(() => new Set<Tab>());
	const renderSidebarInbox = hasDesktopSidebar;
	const renderSidebar = tab === "briefs" || renderSidebarInbox;

	const visibleItems = (mode: "all" | "releases" | "stars" | "followers") => {
		switch (mode) {
			case "releases":
				return items.filter((item) => item.kind === "release");
			case "stars":
				return items.filter((item) => item.kind === "repo_star_received");
			case "followers":
				return items.filter((item) => item.kind === "follower_received");
			default:
				return items;
		}
	};

	useEffect(() => {
		setSmartInFlightKeys(makeSmartInFlightKeys(feedMode));
	}, [feedMode]);

	useEffect(() => {
		setSelectedBriefId((current) => {
			if (current && storyBriefs.some((brief) => brief.id === current)) {
				return current;
			}
			return storyBriefs[0]?.id ?? null;
		});
	}, [storyBriefs]);

	useEffect(() => {
		if (!pendingFeedTab || !isFeedBackedTab(pendingFeedTab)) {
			return;
		}
		const timer = window.setTimeout(() => {
			setResolvedDeferredFeedTabs((current) => {
				const next = new Set(current);
				next.add(pendingFeedTab);
				return next;
			});
			setPendingFeedTab((current) =>
				current === pendingFeedTab ? null : current,
			);
		}, deferredFeedLoadDelayMs);
		return () => window.clearTimeout(timer);
	}, [deferredFeedLoadDelayMs, pendingFeedTab]);

	useEffect(() => {
		if (!isFeedBackedTab(tab)) {
			return;
		}
		if (!deferredFeedTabs.includes(tab)) {
			return;
		}
		if (resolvedDeferredFeedTabs.has(tab) || pendingFeedTab === tab) {
			return;
		}
		setPendingFeedTab(tab);
	}, [deferredFeedTabs, pendingFeedTab, resolvedDeferredFeedTabs, tab]);

	const openReleaseDetail = (releaseId: string) => {
		setTab("briefs");
		setActiveReleaseId(releaseId);
	};

	const generateBriefForDate = async (date: string) => {
		await new Promise((resolve) => window.setTimeout(resolve, 900));
		const nextBrief =
			generatedBriefTemplates[date] ??
			makeBrief({
				id: `brief-generated-${date}`,
				date,
				window_start: null,
				window_end: null,
				content_markdown: "## 项目更新\n\n- 本时间窗口内没有新的 Release。",
				created_at: `${date}T08:00:03+08:00`,
			});
		setStoryBriefs((current) =>
			[nextBrief, ...current.filter((brief) => brief.id !== nextBrief.id)].sort(
				(a, b) =>
					(b.window_end ?? b.created_at).localeCompare(
						a.window_end ?? a.created_at,
					),
			),
		);
		setSelectedBriefId(nextBrief.id);
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

	const renderFeedPanel = (
		mode: "all" | "releases" | "stars" | "followers",
	) => {
		const isActiveMode = tab === mode;
		const loadingInitial = isActiveMode && pendingFeedTab === mode;
		const filteredItems = loadingInitial ? [] : visibleItems(mode);
		const blockingFeedError =
			feedError?.phase === "initial" && filteredItems.length === 0;

		return filteredItems.length === 0 &&
			!loadingInitial &&
			!blockingFeedError ? (
			<div className="bg-card/70 mb-4 rounded-xl border p-6 shadow-sm">
				{emptyState === "auto-sync" ? (
					<>
						<h2 className="text-base font-semibold tracking-tight">
							正在同步你的 GitHub 动态
						</h2>
						<p className="text-muted-foreground mt-1 text-sm">
							先展示已有缓存，再补齐最新
							release、被加星和被关注记录；完成后这里会自动刷新。
						</p>
					</>
				) : (
					<>
						<h2 className="text-base font-semibold tracking-tight">
							还没有缓存内容
						</h2>
						<p className="text-muted-foreground mt-1 text-sm">
							可以先同步一次，把 release 和社交动态都拉下来。
						</p>
						<div className="mt-4 flex flex-wrap gap-2">
							<Button disabled={syncingAll}>{SYNC_ALL_LABEL}</Button>
						</div>
					</>
				)}
			</div>
		) : (
			<FeedGroupedList
				mode={mode}
				items={filteredItems}
				currentViewer={STORYBOOK_VIEWER}
				briefs={storyBriefs}
				dailyBoundaryLocal={dailyBoundaryLocal}
				dailyBoundaryTimeZone={dailyBoundaryTimeZone}
				dailyBoundaryUtcOffsetMinutes={dailyBoundaryUtcOffsetMinutes}
				now={now}
				error={feedError}
				loadingInitial={loadingInitial}
				loadingMore={false}
				hasMore={false}
				translationInFlightKeys={translationInFlightKeys}
				smartInFlightKeys={smartInFlightKeys}
				registerItemRef={() => () => {}}
				onLoadMore={() => {}}
				onRetryInitial={() => {}}
				selectedLaneByKey={Object.fromEntries(
					filteredItems.map((item) => [
						feedItemKey(item),
						defaultLaneForItem(
							item,
							pageDefaultLane,
							allowReleaseItemLaneOverride,
							selectedLaneByKey[feedItemKey(item)],
						),
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
				reactionErrorByKey={reactionErrorByKey}
				onToggleReaction={() => {}}
				onOpenReleaseFromBrief={mode === "all" ? openReleaseDetail : undefined}
				onGenerateBriefForDate={
					mode === "all" ? generateBriefForDate : undefined
				}
			/>
		);
	};

	return (
		<VersionMonitorStateProvider value={STORYBOOK_VERSION_STATE}>
			<AppShell
				header={
					<DashboardHeader
						login={STORYBOOK_VIEWER.login}
						avatarUrl={STORYBOOK_VIEWER.avatar_url}
						isAdmin
						aiDisabledHint={aiDisabledHint}
						busy={syncingAll}
						syncingAll={syncingAll}
						onSyncAll={() => {}}
						logoutHref="#"
						mobileControlBand={
							<DashboardMobileControlBand
								tab={tab}
								onSelectTab={(nextTab) => setTab(nextTab)}
								showPageLaneSelector={tab === "all" || tab === "releases"}
								pageLane={effectivePageDefaultLane}
								onSelectPageLane={(lane) => {
									setPageDefaultLane(lane);
									setSelectedLaneByKey({});
								}}
								layout="stacked"
							/>
						}
					/>
				}
				notice={<VersionUpdateNotice />}
				footer={showFooter ? <AppMetaFooter /> : undefined}
				mobileChrome
			>
				<Tabs
					value={tab}
					onValueChange={(nextTab) => setTab(nextTab as Tab)}
					className="gap-4 sm:gap-6"
				>
					<div className="hidden flex-wrap items-center justify-between gap-2 sm:flex">
						<DashboardTabsList />
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
									className="hidden sm:inline-flex"
								/>
							) : null}
							<Button
								asChild
								variant="outline"
								size="sm"
								className="font-mono text-xs"
							>
								<InternalLink href="/admin" to="/admin">
									管理员面板
								</InternalLink>
							</Button>
						</div>
					</div>

					<div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_360px] md:gap-6">
						<section className="min-w-0">
							<TabsContent value="all" className="mt-0 min-w-0">
								{renderFeedPanel("all")}
							</TabsContent>
							<TabsContent value="releases" className="mt-0 min-w-0">
								{renderFeedPanel("releases")}
							</TabsContent>
							<TabsContent value="stars" className="mt-0 min-w-0">
								{renderFeedPanel("stars")}
							</TabsContent>
							<TabsContent value="followers" className="mt-0 min-w-0">
								{renderFeedPanel("followers")}
							</TabsContent>
							<TabsContent value="briefs" className="mt-0 min-w-0">
								<ReleaseDailyCard
									briefs={storyBriefs}
									selectedId={selectedBriefId}
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

						{renderSidebar ? (
							<aside className="space-y-4 sm:space-y-6">
								{tab === "briefs" ? (
									<BriefListCard
										briefs={storyBriefs}
										selectedId={selectedBriefId}
										onSelectId={(id) => setSelectedBriefId(id)}
									/>
								) : null}
								{renderSidebarInbox ? (
									<div data-dashboard-sidebar-inbox="true">
										<InboxQuickList notifications={notifications} />
									</div>
								) : null}
							</aside>
						) : null}
					</div>
				</Tabs>

				<ReleaseDetailCard
					releaseId={activeReleaseId}
					onClose={() => setActiveReleaseId(null)}
				/>

				<Dialog open={patDialogOpen} onOpenChange={setPatDialogOpen}>
					<DialogContent className="max-w-md">
						<DialogHeader>
							<DialogTitle>配置 GitHub PAT</DialogTitle>
							<DialogDescription>
								不用跳走，直接在这里补齐就行。
							</DialogDescription>
						</DialogHeader>
						<div className="space-y-4">
							<p className="text-sm text-foreground">
								先补齐 GitHub PAT，才能继续使用站内反馈。
							</p>
							<div className="space-y-2">
								<div className="flex items-center justify-between gap-3">
									<Label htmlFor="story-pat-input">GitHub PAT</Label>
									<span className="text-muted-foreground font-mono text-xs">
										已保存：ghp_****wxyz
									</span>
								</div>
								<Input
									id="story-pat-input"
									type="password"
									autoComplete="new-password"
									value="ghp_example_valid_token"
									readOnly
									className="h-10 font-mono text-sm"
								/>
							</div>
							<div className="rounded-xl border border-border/70 bg-muted/20 px-3 py-2.5">
								<p className="text-sm font-medium">GitHub PAT 可用</p>
								<p className="text-muted-foreground mt-1 text-xs leading-5">
									输入后会在 800ms 后自动校验；通过后才能保存。
								</p>
							</div>
							<div className="text-muted-foreground flex items-center justify-between gap-3 text-xs">
								<span>最近检查：2026/4/18 19:20:38</span>
								<Button
									asChild
									variant="ghost"
									size="sm"
									className="h-auto px-0 text-xs"
								>
									<InternalLink
										href={buildSettingsHref("github-pat")}
										to="/settings"
										search={buildSettingsSearch("github-pat")}
									>
										去完整设置
									</InternalLink>
								</Button>
							</div>
						</div>
						<DialogFooter>
							<Button variant="outline" onClick={() => setPatDialogOpen(false)}>
								取消
							</Button>
							<Button>保存 GitHub PAT</Button>
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
		viewport: {
			options: DASHBOARD_VIEWPORTS,
		},
		docs: {
			description: {
				component:
					"Dashboard 组合了 Feed、Brief、Inbox、Release 详情与 reaction fallback 快速补录弹层，是 OctoRill 登录后的主工作台。当前同步入口统一收敛为一个顶部主按钮，这组 stories 用来确认默认、同步中与空态文案是否保持一致。\n\n相关公开文档：[产品说明](../product.html) · [配置参考](../config.html)",
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
		showFooter: true,
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
		await expect(
			canvas.queryByRole("button", { name: "日报设置" }),
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
					"`发布` tab 只在日组切换处显示弱化分隔线：首组不画前置分隔，历史日组继续用日期与当日 Release 数提示边界。",
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
		showFooter: false,
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
					"`全部` tab 中，今天保持原始 Release feed；历史日组默认展示日报卡片，切到列表视图后恢复同一天的原始混排记录。",
			},
		},
	},
	play: async ({ canvasElement, step }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText(/^2026-04-03\s+·\s+2 条 Release$/),
		).toBeVisible();
		await expect(canvas.getByText("## 获星与关注")).toBeVisible();
		await expect(canvas.getByRole("link", { name: "#13840" })).toBeVisible();
		await expect(
			canvas.queryByText(HISTORY_RAW_MARKER),
		).not.toBeInTheDocument();
		const historicalGroup = canvasElement.querySelector<HTMLElement>(
			'[data-feed-group-type="historical"][data-feed-brief-date="2026-04-04"]',
		);
		expect(historicalGroup).toBeTruthy();
		if (!historicalGroup) {
			throw new Error("Expected 2026-04-04 historical group to exist");
		}
		expect(
			historicalGroup.querySelectorAll("[data-social-card-kind]").length,
		).toBe(0);
		await step("expand historical releases", async () => {
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
			expect(expandButton.textContent?.trim()).toBe("列表");
			const beforeSlotRect = beforeSlot.getBoundingClientRect();
			const beforeButtonRect = expandButton.getBoundingClientRect();
			await expandButton.click();
			await expect(canvas.getByText(HISTORY_RAW_MARKER)).toBeVisible();
			await expect(canvas.queryByText("## 获星与关注")).not.toBeInTheDocument();
			expect(
				historicalGroup.querySelectorAll("[data-social-card-kind]").length,
			).toBeGreaterThan(0);
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
		showFooter: false,
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
			await expect(canvas.getByText("### 关注")).toBeVisible();
			await expect(canvas.queryByText("### 获星")).not.toBeInTheDocument();
			const historicalGroup = canvasElement.querySelector<HTMLElement>(
				'[data-feed-group-type="historical"][data-feed-brief-date="2026-04-03"]',
			);
			expect(historicalGroup).toBeTruthy();
			if (!historicalGroup) {
				throw new Error("Expected 2026-04-03 historical group to exist");
			}
			expect(
				historicalGroup.querySelectorAll("[data-social-card-kind]").length,
			).toBe(0);
		});
	},
};

export const EvidenceAllHistoryFallbackToReleaseCards: Story = {
	name: "Evidence / All History Fallback To Release Cards",
	args: {
		initialTab: "all",
		briefs: [],
		showFooter: false,
	},
	parameters: {
		docs: {
			disable: true,
		},
	},
};

export const MobileDayDividerNoOverlap: Story = {
	render: () => (
		<DashboardPreview
			initialTab="all"
			briefs={[]}
			feedItems={makeMobileDayDividerProofFeed()}
			showFooter={false}
		/>
	),
	globals: {
		viewport: {
			value: "dashboardMobileDivider375",
			isRotated: false,
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"移动端窄宽度下，当前日 release 卡片的 reaction footer 下方仍要给下一天 divider 留出安全间距；若历史组存在 action，则 action 与 label 要么同排分离、要么稳定换行，不得互相覆盖。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("button", { name: "生成日报" }),
		).toBeVisible();
		const reactionFooter = canvasElement.querySelector<HTMLElement>(
			'[data-reaction-footer="true"]',
		);
		const dayLabel = Array.from(
			canvasElement.querySelectorAll<HTMLElement>(
				'[data-feed-day-label="true"]',
			),
		).find(
			(element) => element.textContent?.trim() === "2026-04-03 · 1 条 Release",
		);
		expect(reactionFooter).not.toBeNull();
		expect(dayLabel).not.toBeUndefined();
		if (!reactionFooter || !dayLabel) {
			throw new Error("Expected reaction footer and day label");
		}
		const dayHeader = dayLabel.closest<HTMLElement>(
			'[data-feed-day-header="true"]',
		);
		const actionSlot = dayHeader?.querySelector<HTMLElement>(
			'[data-feed-day-action-slot="true"]',
		);
		expect(dayHeader).not.toBeNull();
		expect(actionSlot).not.toBeNull();
		if (!dayHeader || !actionSlot) {
			throw new Error("Expected header, label, and action slot");
		}

		const footerRect = reactionFooter.getBoundingClientRect();
		const headerRect = dayHeader.getBoundingClientRect();
		const labelRect = dayLabel.getBoundingClientRect();
		const actionRect = actionSlot.getBoundingClientRect();
		const intersects =
			labelRect.left < actionRect.right &&
			actionRect.left < labelRect.right &&
			labelRect.top < actionRect.bottom &&
			actionRect.top < labelRect.bottom;

		expect(headerRect.top - footerRect.bottom).toBeGreaterThanOrEqual(8);
		expect(
			Math.min(labelRect.top, actionRect.top) - footerRect.bottom,
		).toBeGreaterThanOrEqual(8);
		expect(intersects).toBe(false);
	},
};

export const EvidenceMobileDayDividerNoOverlap: Story = {
	name: "Evidence / Mobile Day Divider No Overlap",
	render: MobileDayDividerNoOverlap.render,
	globals: MobileDayDividerNoOverlap.globals,
	parameters: {
		docs: {
			disable: true,
		},
	},
};

export const MobileMixedActivityDayDividerNoOverlap: Story = {
	render: () => (
		<DashboardPreview
			initialTab="all"
			briefs={MOBILE_MIXED_ACTIVITY_DIVIDER_PROOF_BRIEFS}
			feedItems={makeMobileMixedActivityDividerProofFeed()}
			showFooter={false}
		/>
	),
	globals: {
		viewport: {
			value: "dashboardMobileDivider375",
			isRotated: false,
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"复现主人截图对应的窄屏场景：当前日 release 卡片下方紧跟一个同时包含 release 计数、动态计数和右侧 `列表` action 的历史 divider；移动端不得出现 `…动态` 与 `列表` 相互压叠。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const listButton = await canvas.findByRole("button", { name: "列表" });
		const reactionFooter = canvasElement.querySelector<HTMLElement>(
			'[data-reaction-footer="true"]',
		);
		const historicalGroup = listButton.closest<HTMLElement>(
			'[data-feed-group-type="historical"]',
		);
		const dayLabel = historicalGroup?.querySelector<HTMLElement>(
			'[data-feed-day-label="true"]',
		);
		const actionSlot = historicalGroup?.querySelector<HTMLElement>(
			'[data-feed-day-action-slot="true"]',
		);
		expect(reactionFooter).not.toBeNull();
		expect(historicalGroup).not.toBeNull();
		expect(dayLabel).not.toBeNull();
		expect(actionSlot).not.toBeNull();
		if (!reactionFooter || !historicalGroup || !dayLabel || !actionSlot) {
			throw new Error(
				"Expected reaction footer, historical group, label, and action",
			);
		}

		expect(dayLabel.textContent?.trim()).toBe("2026-04-04 · 4 条动态");

		const footerRect = reactionFooter.getBoundingClientRect();
		const labelRect = dayLabel.getBoundingClientRect();
		const actionRect = actionSlot.getBoundingClientRect();
		const buttonRect = listButton.getBoundingClientRect();
		const intersects =
			labelRect.left < buttonRect.right &&
			buttonRect.left < labelRect.right &&
			labelRect.top < buttonRect.bottom &&
			buttonRect.top < labelRect.bottom;

		expect(
			Math.min(labelRect.top, actionRect.top) - footerRect.bottom,
		).toBeGreaterThanOrEqual(8);
		expect(intersects).toBe(false);
		expect(actionRect.right - buttonRect.right).toBeLessThanOrEqual(4);
	},
};

export const EvidenceMobileMixedActivityDayDividerNoOverlap: Story = {
	name: "Evidence / Mobile Mixed Activity Day Divider No Overlap",
	render: MobileMixedActivityDayDividerNoOverlap.render,
	globals: MobileMixedActivityDayDividerNoOverlap.globals,
	parameters: {
		docs: {
			disable: true,
		},
	},
};

export const MobileAllTabStickyShell: Story = {
	name: "Evidence / Mobile all tab sticky shell",
	render: () => (
		<DashboardPreview
			initialTab="all"
			feedItems={makeMobileMixedActivityDividerProofFeed()}
			briefs={MOBILE_MIXED_ACTIVITY_DIVIDER_PROOF_BRIEFS}
		/>
	),
	globals: {
		viewport: {
			value: "dashboardMobile390",
			isRotated: false,
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"移动端 `全部` tab 的长内容场景：顶部壳层会跟随真实 viewport 高度更新，滚动进入 compact header 后仍保持吸顶，不再因为高度链失真而整段滑出视口。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const storyWindow = canvasElement.ownerDocument.defaultView;
		const shell = canvasElement.querySelector<HTMLElement>(
			"[data-app-shell-mobile-chrome='true']",
		);
		const headerState = canvasElement.querySelector<HTMLElement>(
			"[data-dashboard-header-progress]",
		);
		const stickyHeader = canvasElement.querySelector<HTMLElement>(
			"[data-app-shell-header='true']",
		);
		if (!storyWindow || !shell || !headerState || !stickyHeader) {
			throw new Error(
				"Expected story window, app shell, dashboard header state, and sticky header",
			);
		}

		await expect(canvas.getByRole("tab", { name: "全部" })).toBeVisible();
		await expect(headerState).toHaveAttribute(
			"data-dashboard-header-compact",
			"false",
		);
		const initialViewportHeight = Math.round(
			storyWindow.visualViewport?.height ?? storyWindow.innerHeight,
		);
		expect(
			Math.abs(
				Number(shell.dataset.appShellViewportHeight ?? "0") -
					initialViewportHeight,
			),
		).toBeLessThanOrEqual(1);

		storyWindow.scrollTo({ top: 420, behavior: "auto" });
		await new Promise((resolve) => storyWindow.setTimeout(resolve, 280));

		await expect(headerState).toHaveAttribute(
			"data-dashboard-header-compact",
			"true",
		);
		expect(
			Math.abs(stickyHeader.getBoundingClientRect().top),
		).toBeLessThanOrEqual(1);
		const compactViewportHeight = Math.round(
			storyWindow.visualViewport?.height ?? storyWindow.innerHeight,
		);
		expect(
			Math.abs(
				Number(shell.dataset.appShellViewportHeight ?? "0") -
					compactViewportHeight,
			),
		).toBeLessThanOrEqual(1);
	},
};

export const VerificationMobileAllTabStickyShell: Story = {
	name: "Verification / Mobile all tab sticky shell",
	render: MobileAllTabStickyShell.render,
	globals: MobileAllTabStickyShell.globals,
	play: MobileAllTabStickyShell.play,
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

export const BriefsProjectOnly: Story = {
	render: () => (
		<DashboardPreview initialTab="briefs" briefs={projectOnlyBriefs} />
	),
	parameters: {
		docs: {
			description: {
				story:
					"当日报窗口内只有 Release、没有获星或关注时，正文只保留 `## 项目更新`，不再渲染社交空态。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("## 项目更新")).toBeVisible();
		await expect(canvas.queryByText("## 获星与关注")).not.toBeInTheDocument();
		await expect(canvas.queryByText("### 获星")).not.toBeInTheDocument();
		await expect(canvas.queryByText("### 关注")).not.toBeInTheDocument();
	},
};

export const EvidenceBriefsProjectOnly: Story = {
	name: "Evidence / Briefs Project Only",
	render: () => (
		<DashboardPreview
			initialTab="briefs"
			briefs={projectOnlyBriefs}
			showFooter={false}
		/>
	),
	parameters: {
		docs: {
			disable: true,
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

export const MobileInboxTabWithoutSidebarQuickList: Story = {
	args: {
		initialTab: "inbox",
	},
	parameters: {
		viewport: {
			defaultViewport: "dashboardMobileDivider375",
		},
		docs: {
			description: {
				story:
					"移动端只保留收件箱 tab 内的主列表，不再在折叠后的侧栏位置重复渲染 Inbox Quick List。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole("heading", { name: "Inbox" })).toBeVisible();
		await expect(
			canvasElement.querySelector("[data-dashboard-sidebar-inbox='true']"),
		).toBeNull();
		await expect(canvas.getByText("Build failed on main")).toBeVisible();
		const syncButton = canvas.getByRole("button", { name: "Sync inbox" });
		const githubLink = canvas.getByRole("link", { name: "GitHub" });
		const syncRect = syncButton.getBoundingClientRect();
		const githubRect = githubLink.getBoundingClientRect();
		expect(Math.round(syncRect.width)).toBeLessThanOrEqual(36);
		expect(Math.round(githubRect.width)).toBeLessThanOrEqual(36);
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
				story: "直接展示 reaction fallback 快速补录 GitHub PAT 的弹层状态。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByLabelText("GitHub PAT")).toHaveAttribute(
			"autocomplete",
			"new-password",
		);
	},
};

export const ReactionCompact: Story = {
	render: () => (
		<DashboardPreview
			initialTab="releases"
			feedItems={makeReactionCompactFeed()}
		/>
	),
	parameters: {
		docs: {
			description: {
				story:
					"Release 底部反馈区轻微收紧：按钮收敛到 36px、图标约 18px、badge 同步下调并保持外置，不再像上一版那样过于抢眼。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole("button", { name: "赞 2" })).toBeVisible();
		await expect(canvas.getByRole("button", { name: "关注" })).toBeVisible();
		const reactionFooter = canvasElement.querySelector<HTMLElement>(
			'[data-reaction-footer="true"]',
		);
		expect(reactionFooter).not.toBeNull();
		if (!reactionFooter) {
			throw new Error("Expected reaction footer to exist");
		}

		const plusOneButton = reactionFooter.querySelector<HTMLButtonElement>(
			'[data-reaction-trigger="plus1"]',
		);
		const plusOneBadge = reactionFooter.querySelector<HTMLElement>(
			'[data-reaction-count-badge="plus1"]',
		);
		const plusOneIcon = reactionFooter.querySelector<HTMLImageElement>(
			'[data-reaction-icon="plus1"]',
		);
		const eyesBadge = reactionFooter.querySelector(
			'[data-reaction-count-badge="eyes"]',
		);
		expect(plusOneButton).not.toBeNull();
		expect(plusOneBadge).not.toBeNull();
		expect(plusOneIcon).not.toBeNull();
		expect(eyesBadge).toBeNull();
		if (!plusOneButton || !plusOneBadge || !plusOneIcon) {
			throw new Error("Expected compact reaction button, badge, and icon");
		}

		const buttonRect = plusOneButton.getBoundingClientRect();
		const badgeRect = plusOneBadge.getBoundingClientRect();
		const iconRect = plusOneIcon.getBoundingClientRect();
		expect(buttonRect.width).toBeGreaterThanOrEqual(35);
		expect(buttonRect.width).toBeLessThanOrEqual(37);
		expect(buttonRect.height).toBeGreaterThanOrEqual(35);
		expect(buttonRect.height).toBeLessThanOrEqual(37);
		expect(Math.abs(buttonRect.width - buttonRect.height)).toBeLessThanOrEqual(
			1,
		);
		expect(iconRect.width).toBeGreaterThanOrEqual(17);
		expect(iconRect.width).toBeLessThanOrEqual(19);
		expect(iconRect.height).toBeGreaterThanOrEqual(17);
		expect(iconRect.height).toBeLessThanOrEqual(19);
		expect(badgeRect.height).toBeGreaterThanOrEqual(17);
		expect(badgeRect.height).toBeLessThanOrEqual(19.5);
		expect(badgeRect.width).toBeLessThanOrEqual(24);
		expect(badgeRect.right).toBeGreaterThan(buttonRect.right);
		expect(badgeRect.top).toBeLessThan(buttonRect.top + 2);
	},
};

export const EvidenceReactionCompact: Story = {
	name: "Evidence / Reaction Compact",
	render: ReactionCompact.render,
	parameters: {
		docs: {
			disable: true,
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

export const LongBodyTranslation: Story = {
	args: {
		initialTab: "releases",
		feedMode: "long-body-translation",
	},
	parameters: {
		docs: {
			description: {
				story:
					"超长 Release 正文在列表中仍然展示截断原文，但翻译标签会直接展示分块翻译结果。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "v3.0.0（长正文）" }),
		).toBeVisible();
		await canvas.getByRole("tab", { name: "翻译" }).click();
		await expect(
			canvas.getByText(/长正文会走完整 release 详情翻译链路/),
		).toBeVisible();
		await expect(canvas.queryByText(/正文过长，无法直接翻译/)).toBeNull();
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

export const MobileReleaseCardActionPolish: Story = {
	args: {
		initialTab: "releases",
		feedMode: "default",
	},
	parameters: {
		viewport: {
			defaultViewport: "dashboardMobile390",
		},
		docs: {
			description: {
				story:
					"移动端 release 卡片收起单卡内容切换，统一只保留顶部页面级阅读模式入口；GitHub 打开方式收敛到卡片右上角的 icon-only 链接。",
			},
		},
	},
	play: async ({ canvasElement, userEvent }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "v2.63.0 · 版本变化" }),
		).toBeVisible();
		const releaseCard =
			canvasElement.querySelector<HTMLElement>('[data-slot="card"]');
		if (!releaseCard) {
			throw new Error("Expected a release card in mobile story");
		}
		const releaseCardCanvas = within(releaseCard);
		await expect(
			releaseCardCanvas.queryByRole("tab", { name: "翻译" }),
		).not.toBeInTheDocument();
		await expect(
			releaseCardCanvas.queryByRole("button", { name: "GitHub" }),
		).not.toBeInTheDocument();

		const mobileGithubLink = releaseCard.querySelector<HTMLElement>(
			'[data-feed-mobile-github-link="true"]',
		);
		const title = releaseCardCanvas.getByRole("heading", {
			name: "v2.63.0 · 版本变化",
		});
		if (!mobileGithubLink) {
			throw new Error("Expected mobile GitHub icon link");
		}
		const githubRect = mobileGithubLink.getBoundingClientRect();
		const titleRect = title.getBoundingClientRect();
		expect(Math.round(githubRect.width)).toBeLessThanOrEqual(36);
		expect(githubRect.top).toBeLessThan(titleRect.top);

		const laneMenuTrigger = canvasElement.querySelector<HTMLElement>(
			"[data-dashboard-mobile-lane-menu-trigger]",
		);
		if (!laneMenuTrigger) {
			throw new Error("Expected mobile lane menu trigger");
		}
		await userEvent.click(laneMenuTrigger);
		await userEvent.click(
			await canvas.findByRole("menuitemradio", { name: "翻译" }),
		);
		await expect(
			releaseCardCanvas.getByRole("heading", { name: "v2.63.0（稳定版）" }),
		).toBeVisible();
	},
};

export const PageDefaultLaneSwitchingMobile: Story = {
	name: "Evidence / Mobile lane switching",
	args: {
		initialTab: "releases",
		feedMode: "default",
	},
	globals: {
		viewport: {
			value: "dashboardMobile390",
			isRotated: false,
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"移动端页面级阅读模式切换入口：右上菜单按钮的触摸不会再误触发 header drag，选择“翻译”后当前 release feed 会立即切到翻译态。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const appShellHeader = canvasElement.querySelector<HTMLElement>(
			"[data-app-shell-header-interacting]",
		);
		expect(appShellHeader).not.toBeNull();
		if (!appShellHeader) {
			throw new Error("Expected app shell interaction state to exist");
		}

		await expect(
			canvas.getByRole("heading", { name: "v2.63.0 · 版本变化" }),
		).toBeVisible();
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);

		const laneMenuTrigger = canvas.getByRole("button", {
			name: "当前阅读模式：智能",
		}) as HTMLButtonElement;
		dispatchSyntheticTouchEvent(laneMenuTrigger, "touchstart");
		dispatchSyntheticTouchEvent(laneMenuTrigger, "touchmove", 10, 0);
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);
		dispatchSyntheticTouchEvent(laneMenuTrigger, "touchmove", 0, -10);
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);

		await laneMenuTrigger.click();
		await expect(
			canvas.getByRole("menu", { name: "选择阅读模式" }),
		).toBeVisible();
		const laneMenu = canvas.getByRole("menu", { name: "选择阅读模式" });
		const translatedOption = within(laneMenu).getByRole("menuitemradio", {
			name: "翻译",
		}) as HTMLButtonElement;
		dispatchSyntheticTouchEvent(translatedOption, "touchstart");
		dispatchSyntheticTouchEvent(translatedOption, "touchmove", 10, 0);
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);
		dispatchSyntheticTouchEvent(translatedOption, "touchmove", 0, -10);
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);
		await translatedOption.click();
		await expect(
			canvas.queryByRole("menu", { name: "选择阅读模式" }),
		).not.toBeInTheDocument();
		await expect(
			canvas.getByRole("heading", { name: "v2.63.0（稳定版）" }),
		).toBeVisible();
		await expect(appShellHeader).toHaveAttribute(
			"data-app-shell-header-interacting",
			"false",
		);
	},
};

export const VerificationMobileLaneSwitching: Story = {
	name: "Verification / Mobile lane switching",
	args: {
		initialTab: "releases",
		feedMode: "default",
	},
	globals: {
		viewport: {
			value: "dashboardMobile390",
			isRotated: false,
		},
	},
	parameters: {
		docs: {
			disable: true,
		},
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

export const AllMixedSocialActivity: Story = {
	render: () => <DashboardPreview feedItems={makeMixedSocialFeed()} />,
	parameters: {
		docs: {
			description: {
				story:
					"`全部` tab 会把 release、仓库被加星和账号被关注三类记录按统一时间线混排显示。",
			},
		},
	},
	play: async ({ canvasElement, step }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("torvalds", { exact: true })).toBeVisible();
		await expect(canvas.getByText("gaearon", { exact: true })).toBeVisible();
		await expect(canvas.getByText("标星", { exact: true })).toBeVisible();
		await expect(
			canvas.getByRole("heading", { name: "v2.63.0 · 版本变化" }),
		).toBeVisible();
		await step(
			"historical list toggle keeps mixed activity visible",
			async () => {
				const historicalGroup = canvasElement.querySelector<HTMLElement>(
					'[data-feed-group-type="historical"][data-feed-brief-date="2026-04-03"]',
				);
				expect(historicalGroup).toBeTruthy();
				if (!historicalGroup) {
					throw new Error("Expected 2026-04-03 historical group to exist");
				}
				const actionSlot = historicalGroup.querySelector<HTMLElement>(
					"[data-feed-day-action-slot]",
				);
				const listButton =
					actionSlot?.querySelector<HTMLButtonElement>("button");
				expect(listButton).toBeTruthy();
				if (!listButton) {
					throw new Error("Expected historical list button to exist");
				}
				await listButton.click();
				await expect(canvas.getByText(HISTORY_RAW_MARKER)).toBeVisible();
				await expect(canvas.getByText("linus", { exact: true })).toBeVisible();
			},
		);
		await step(
			"social cards expose actor and target links intentionally",
			async () => {
				const socialCards = canvasElement.querySelectorAll<HTMLElement>(
					"[data-social-card-kind]",
				);
				expect(socialCards.length).toBeGreaterThan(0);
				for (const card of socialCards) {
					const expectedLinks =
						card.dataset.socialCardKind === "repo_star_received" ? 2 : 1;
					expect(countVisibleGithubLinks(card)).toBe(expectedLinks);
				}
			},
		);
	},
};

export const StarsTab: Story = {
	render: () => (
		<DashboardPreview initialTab="stars" feedItems={makeMixedSocialFeed()} />
	),
	parameters: {
		docs: {
			description: {
				story:
					"`加星` tab 只显示 repo star 收到记录，不混入 release 或 follower。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("torvalds", { exact: true })).toBeVisible();
		await expect(
			canvas.getByText(PROJECT_REPO_FULL_NAME, { exact: true }),
		).toBeVisible();
		await expect(canvas.getByText("标星", { exact: true })).toBeVisible();
		const socialCards = canvasElement.querySelectorAll<HTMLElement>(
			'[data-social-card-kind="repo_star_received"]',
		);
		expect(socialCards.length).toBeGreaterThan(0);
		for (const card of socialCards) {
			expect(countVisibleGithubLinks(card)).toBe(2);
			expect(card.dataset.socialCardTimeVisible).toBe("true");
			expect(card.querySelector("[data-social-card-timestamp]")).not.toBeNull();
		}
		await expect(
			canvas.queryByText("gaearon", { exact: true }),
		).not.toBeInTheDocument();
	},
};

export const PostBootStarsTabSwitchKeepsShell: Story = {
	render: () => (
		<DashboardPreview
			feedItems={makeMixedSocialFeed()}
			deferredFeedTabs={["stars"]}
		/>
	),
	parameters: {
		docs: {
			description: {
				story:
					"模拟页面已完成首次 hydration 后，用户从 `全部` 切到 `加星`，主壳层保持不卸载，仅主列切到局部 loading 骨架，待数据返回后再展示星标记录。",
			},
		},
	},
	play: async ({ canvasElement, step }) => {
		const canvas = within(canvasElement);
		const secondaryControls = canvasElement.querySelector(
			"[data-dashboard-secondary-controls]",
		);
		expect(secondaryControls).not.toBeNull();
		await expect(
			canvas.queryByRole("button", { name: "日报设置" }),
		).not.toBeInTheDocument();
		await step("switch to stars without dropping the app shell", async () => {
			await canvas.getByRole("tab", { name: "加星" }).click();
			expect(
				canvasElement.querySelector('[data-feed-loading-skeleton="true"]'),
			).not.toBeNull();
			await expect(
				canvas.queryByRole("button", { name: "日报设置" }),
			).not.toBeInTheDocument();
			expect(
				canvasElement.querySelector("[data-dashboard-boot-header]"),
			).toBeNull();
		});
		await step(
			"resolve the local skeleton into the new tab dataset",
			async () => {
				await expect(
					canvas.getByText("torvalds", { exact: true }),
				).toBeVisible();
				expect(
					canvasElement.querySelector('[data-feed-loading-skeleton="true"]'),
				).toBeNull();
			},
		);
	},
};

export const EvidencePostBootStarsTabLoading: Story = {
	name: "Evidence / Post-Boot Stars Tab Loading",
	render: () => (
		<DashboardPreview
			initialTab="stars"
			feedItems={makeMixedSocialFeed()}
			initialFeedTabLoading="stars"
			deferredFeedLoadDelayMs={60_000}
			showFooter={false}
		/>
	),
	parameters: {
		docs: {
			disable: true,
		},
	},
};

export const FollowersTab: Story = {
	render: () => (
		<DashboardPreview
			initialTab="followers"
			feedItems={makeMixedSocialFeed()}
		/>
	),
	parameters: {
		docs: {
			description: {
				story:
					"`关注` tab 只显示 follower 收到记录，并保留头像 + GitHub CTA 的轻量卡片样式；followers 不展示时间文案。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("gaearon", { exact: true })).toBeVisible();
		await expect(
			within(
				canvasElement.querySelector<HTMLElement>(
					'[data-social-card-kind="follower_received"]',
				) ?? canvasElement,
			).getByText("关注", { exact: true }),
		).toBeVisible();
		const socialCards = canvasElement.querySelectorAll<HTMLElement>(
			'[data-social-card-kind="follower_received"]',
		);
		expect(socialCards.length).toBeGreaterThan(0);
		for (const card of socialCards) {
			expect(countVisibleGithubLinks(card)).toBe(1);
			expect(card.dataset.socialCardTimeVisible).toBe("false");
			expect(card.querySelector("[data-social-card-timestamp]")).toBeNull();
		}
		await expect(
			canvas.queryByRole("heading", { name: "v2.63.0 · 版本变化" }),
		).not.toBeInTheDocument();
	},
};

export const MobileSocialCompact: Story = {
	name: "Evidence / Mobile Social Compact",
	render: () => (
		<DashboardPreview
			initialTab="all"
			feedItems={makeMobileCompactSocialFeed()}
		/>
	),
	globals: {
		viewport: {
			value: "dashboardMobile390",
			isRotated: false,
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"移动端证据入口：390px 宽度下把 star / follower 社交卡片改成单条横向信息流，动作区只保留图标，左右信息块维持更均衡的视觉重量。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "移动端社交卡片重设计验证版" }),
		).toBeVisible();

		const socialCards = canvasElement.querySelectorAll<HTMLElement>(
			"[data-social-card-kind]",
		);
		expect(socialCards.length).toBeGreaterThanOrEqual(2);

		const starActorMobile = canvasElement.querySelector<HTMLElement>(
			'[data-social-card-kind="repo_star_received"] [data-social-card-segment="actor"] [data-social-card-primary-mobile-label]',
		);
		const starRepoMobile = canvasElement.querySelector<HTMLElement>(
			'[data-social-card-kind="repo_star_received"] [data-social-card-segment="target"] [data-social-card-primary-mobile-label]',
		);
		const followerActorMobile = canvasElement.querySelector<HTMLElement>(
			'[data-social-card-kind="follower_received"] [data-social-card-segment="actor"] [data-social-card-primary-mobile-label]',
		);
		const followerTargetMobile = canvasElement.querySelector<HTMLElement>(
			'[data-social-card-kind="follower_received"] [data-social-card-segment="target"] [data-social-card-primary-mobile-label]',
		);
		expect(starActorMobile?.textContent?.trim()).toBe(
			"frontend-systems-maintainer",
		);
		expect(starRepoMobile?.textContent?.trim()).toBe(PROJECT_REPO_FULL_NAME);
		expect(starRepoMobile?.scrollWidth).toBeLessThanOrEqual(
			starRepoMobile?.clientWidth ?? 0,
		);
		expect(followerActorMobile?.textContent?.trim()).toBe(
			"design-ops-collaborator",
		);
		expect(followerTargetMobile?.textContent?.trim()).toBe(PROJECT_OWNER_LOGIN);
		for (const card of socialCards) {
			const row = card.querySelector<HTMLElement>("[data-social-card-row]");
			const action = row?.querySelector<HTMLElement>(
				'[data-social-card-segment="action"]',
			);
			expect(action?.textContent?.trim() ?? "").toBe("");
			expect(row?.querySelector(".lucide-arrow-up-right")).toBeNull();
		}
		expect(
			canvasElement.querySelector("[data-social-card-secondary-mobile-label]"),
		).toBeNull();

		for (const card of socialCards) {
			assertInlineSocialCardLayout(card);
		}
	},
};

export const MobileSocialEdgeCases: Story = {
	name: "Evidence / Mobile Social Edge Cases",
	render: () => (
		<DashboardPreview
			initialTab="all"
			feedItems={makeMobileSocialEdgeCaseFeed()}
		/>
	),
	globals: {
		viewport: {
			value: "dashboardMobile390",
			isRotated: false,
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"移动端社交卡片边界条件入口：集中展示右长、左长、双边都长与常规平衡四种场景，便于直接检查宽度分配与图标位置是否符合预期。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const socialCards = canvasElement.querySelectorAll<HTMLElement>(
			"[data-social-card-kind]",
		);
		expect(socialCards.length).toBe(4);

		for (const card of socialCards) {
			assertInlineSocialCardLayout(card);
		}

		const rightLongCard = canvasElement.querySelector<HTMLElement>(
			'[data-social-card-kind="repo_star_received"][data-feed-item-id="mobile-edge-right-long"]',
		);
		const leftLongCard = canvasElement.querySelector<HTMLElement>(
			'[data-social-card-kind="follower_received"][data-feed-item-id="mobile-edge-left-long"]',
		);
		const bilateralLongCard = canvasElement.querySelector<HTMLElement>(
			'[data-social-card-kind="repo_star_received"][data-feed-item-id="mobile-edge-bilateral-long"]',
		);
		expect(rightLongCard).toBeTruthy();
		expect(leftLongCard).toBeTruthy();
		expect(bilateralLongCard).toBeTruthy();
		if (!rightLongCard || !leftLongCard || !bilateralLongCard) {
			throw new Error("Expected all mobile social edge-case cards to render");
		}

		const measureWidths = (card: HTMLElement) => {
			const actor = card.querySelector<HTMLElement>(
				'[data-social-card-segment="actor"]',
			);
			const target = card.querySelector<HTMLElement>(
				'[data-social-card-segment="target"]',
			);
			const actorGroup =
				actor?.querySelector<HTMLElement>(
					'[data-social-card-entity-group="actor"]',
				) ?? actor;
			const targetGroup =
				target?.querySelector<HTMLElement>(
					'[data-social-card-entity-group="target"]',
				) ?? target;
			const actorLabel = actorGroup?.querySelector<HTMLElement>(
				"[data-social-card-primary]",
			);
			const targetLabel = targetGroup?.querySelector<HTMLElement>(
				"[data-social-card-primary]",
			);
			const action = card.querySelector<HTMLElement>(
				'[data-social-card-segment="action"]',
			);
			return {
				actorWidth: actorGroup?.getBoundingClientRect().width ?? 0,
				targetWidth: targetGroup?.getBoundingClientRect().width ?? 0,
				actorOverflow:
					(actorLabel?.scrollWidth ?? 0) - (actorLabel?.clientWidth ?? 0),
				targetOverflow:
					(targetLabel?.scrollWidth ?? 0) - (targetLabel?.clientWidth ?? 0),
				gapLeft:
					action && actorGroup
						? action.getBoundingClientRect().left -
							actorGroup.getBoundingClientRect().right
						: 0,
				gapRight:
					action && targetGroup
						? targetGroup.getBoundingClientRect().left -
							action.getBoundingClientRect().right
						: 0,
			};
		};

		const rightLongWidths = measureWidths(rightLongCard);
		const leftLongWidths = measureWidths(leftLongCard);
		const bilateralLongWidths = measureWidths(bilateralLongCard);

		const rightLongBalanceMode = rightLongCard.querySelector<HTMLElement>(
			"[data-social-card-row]",
		)?.dataset.socialCardBalanceMode;
		expect(
			leftLongCard.querySelector<HTMLElement>("[data-social-card-row]")?.dataset
				.socialCardBalanceMode,
		).toBe("adaptive");
		expect(
			bilateralLongCard.querySelector<HTMLElement>("[data-social-card-row]")
				?.dataset.socialCardBalanceMode,
		).toBe("centered");
		expect(leftLongWidths.actorWidth).toBeGreaterThanOrEqual(
			leftLongWidths.targetWidth,
		);
		expect(["centered", "adaptive"]).toContain(rightLongBalanceMode);
		if (rightLongBalanceMode === "adaptive") {
			expect(
				Math.abs(rightLongWidths.gapLeft - rightLongWidths.gapRight),
			).toBeLessThanOrEqual(2);
		} else {
			expect(rightLongWidths.targetOverflow).toBeGreaterThan(1);
		}
		expect(
			Math.abs(leftLongWidths.gapLeft - leftLongWidths.gapRight),
		).toBeLessThanOrEqual(2);
		expect(
			Math.abs(
				bilateralLongWidths.actorWidth - bilateralLongWidths.targetWidth,
			),
		).toBeLessThanOrEqual(18);
		expect(
			Math.max(
				bilateralLongWidths.actorOverflow,
				bilateralLongWidths.targetOverflow,
			),
		).toBeGreaterThan(1);
	},
};

export const MobileSocialEdgeCaseMatrix: Story = {
	name: "Evidence / Mobile Social Edge Case Matrix",
	render: () => (
		<SocialCardsMatrixPreview
			items={[
				...makeMobileSocialEdgeCaseFeed().filter(isSocialFeedItem),
				...makeMobileShortFollowerFeed().filter(isSocialFeedItem),
			]}
		/>
	),
	globals: {
		viewport: {
			value: "dashboardMobile390",
			isRotated: false,
		},
	},
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				story:
					"纯社交卡片矩阵入口：不混入 header、release、inbox，集中展示右长、左长、双长与短文案 follower 连续列表，方便直接检查实体组贴边、动作图标位置与 trailing whitespace。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const socialCards = canvasElement.querySelectorAll<HTMLElement>(
			"[data-social-card-kind]",
		);
		expect(socialCards.length).toBe(9);
		for (const card of socialCards) {
			assertInlineSocialCardLayout(card);
		}
		const shortFollowerCards = Array.from(
			canvasElement.querySelectorAll<HTMLElement>(
				'[data-feed-item-id^="mobile-short-follow-"]',
			),
		);
		expect(shortFollowerCards.length).toBe(5);
		for (const card of shortFollowerCards) {
			const row = card.querySelector<HTMLElement>("[data-social-card-row]");
			const targetGroup = card.querySelector<HTMLElement>(
				'[data-social-card-entity-group="target"]',
			);
			expect(row).toBeTruthy();
			expect(targetGroup).toBeTruthy();
			if (!row || !targetGroup) {
				throw new Error("Expected short follower row and target entity group");
			}
			const rowRect = row.getBoundingClientRect();
			const targetRect = targetGroup.getBoundingClientRect();
			expect(rowRect.right - targetRect.right).toBeLessThanOrEqual(14);
		}
	},
};

export const LegacyRecoveredAllSocialActivity: Story = {
	...AllMixedSocialActivity,
	name: "Evidence / Legacy Recovered All Social Activity",
	parameters: {
		...AllMixedSocialActivity.parameters,
		docs: {
			description: {
				story:
					"旧账号在下一次 social sync 后补齐可见事件时，`全部` tab 仍按统一时间线混排 release、repo stars 与 followers。",
			},
		},
	},
};

export const LegacyRecoveredStarsTab: Story = {
	...StarsTab,
	name: "Evidence / Legacy Recovered Stars Tab",
	parameters: {
		...StarsTab.parameters,
		docs: {
			description: {
				story:
					"旧账号恢复可见性后，`加星` tab 继续只显示 repo star 记录，并保留真实时间。",
			},
		},
	},
};

export const LegacyRecoveredFollowersTab: Story = {
	...FollowersTab,
	name: "Evidence / Legacy Recovered Followers Tab",
	parameters: {
		...FollowersTab.parameters,
		docs: {
			description: {
				story:
					"旧账号恢复可见性后，`关注` tab 继续只显示 follower 记录，且不展示时间文案。",
			},
		},
	},
};

export const SocialAvatarFallback: Story = {
	render: () => {
		const items = makeMixedSocialFeed().map((item) =>
			item.kind === "follower_received" && item.actor.login === "yyx990803"
				? {
						...item,
						actor: {
							...item.actor,
							avatar_url: null,
						},
					}
				: item,
		);
		return <DashboardPreview initialTab="followers" feedItems={items} />;
	},
	parameters: {
		docs: {
			description: {
				story:
					"当 actor 头像缺失或加载失败时，社交卡片回退到稳定占位头像，避免留出空白。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const fallback = canvasElement.querySelector(
			'[data-social-avatar-fallback="true"]',
		);
		expect(fallback).not.toBeNull();
	},
};

export const OwnerReleasesOptInOff: Story = {
	args: {
		initialTab: "all",
		feedItems: makeOwnReleaseOptInFeed(false),
	},
	parameters: {
		docs: {
			description: {
				story:
					"“我的发布”关闭时，owner-only release 不会出现在 `全部 / 发布`，页面仍只展示真实已加星仓库的发布与社交动态。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.queryByText(OWNER_RELEASE_OPT_IN_TITLE),
		).not.toBeInTheDocument();
		await expect(canvas.getByText("桌面版 Stable v2.1.47")).toBeVisible();
	},
};

export const OwnerReleasesOptInOn: Story = {
	args: {
		initialTab: "all",
		feedItems: makeOwnReleaseOptInFeed(true),
	},
	parameters: {
		docs: {
			description: {
				story:
					"“我的发布”开启后，owner-only release 会进入 `全部 / 发布`，但切到 `加星` 时仍只显示真实社交动态。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText(OWNER_RELEASE_OPT_IN_TITLE)).toBeVisible();

		await userEvent.click(canvas.getAllByRole("tab", { name: "发布" })[0]);
		await expect(canvas.getByText(OWNER_RELEASE_OPT_IN_TITLE)).toBeVisible();

		await userEvent.click(canvas.getAllByRole("tab", { name: "加星" })[0]);
		await expect(
			canvas.queryByText(OWNER_RELEASE_OPT_IN_TITLE),
		).not.toBeInTheDocument();
		await expect(canvas.getByText("lobehub/lobe-chat")).toBeVisible();
	},
};

export const OwnerReleasesEvidenceAllTab: Story = {
	name: "Evidence / Owner Releases All Tab",
	args: {
		initialTab: "all",
		feedItems: makeOwnReleaseOptInFeed(true),
	},
	parameters: {
		docs: {
			description: {
				story:
					"用于视觉验收：`我的发布` 开启后，owner-only release 会直接出现在 `全部` 时间线。",
			},
		},
	},
};

const releaseDetailTranslationError: ReleaseDetailResponse = {
	...longReleaseDetail,
	translated: {
		lang: "zh-CN",
		status: "error",
		title: null,
		summary: null,
		error_code: "markdown_structure_mismatch",
		error_summary: "Markdown 结构校验失败",
		error_detail:
			"release detail translation failed to preserve markdown structure",
	},
};

export const ErrorFeedInitialFailure: Story = {
	name: "Evidence / Feed Initial Failure Surface",
	render: () => (
		<DashboardPreview
			initialTab="releases"
			feedItems={[]}
			feedError={{
				phase: "initial",
				message: "动态列表拉取失败，请稍后重试。",
				at: 1,
			}}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("动态加载失败")).toBeVisible();
		await expect(canvas.getByRole("button", { name: "重试" })).toBeVisible();
	},
};

export const ErrorReactionBubble: Story = {
	name: "Evidence / Reaction Error Bubble",
	render: () => {
		const item = buildFeedItem("error-reaction", {
			reactions: {
				counts: {
					plus1: 2,
					laugh: 0,
					heart: 1,
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
		});
		return (
			<DashboardPreview
				initialTab="releases"
				feedItems={[item]}
				reactionErrorByKey={{
					[feedItemKey(item)]: "该仓库限制了站内反馈，请在 GitHub 页面操作。",
				}}
			/>
		);
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const body = within(canvasElement.ownerDocument.body);
		await expect(
			canvas.getByRole("button", { name: "反馈失败" }),
		).toBeVisible();
		await expect(body.getByText("反馈提交失败")).toBeVisible();
		await expect(
			body.getByText("该仓库限制了站内反馈，请在 GitHub 页面操作。"),
		).toBeVisible();
	},
};

export const ErrorReleaseDetailTranslationFailure: Story = {
	name: "Evidence / Release Detail Translation Failure",
	render: () => (
		<DashboardPreview
			initialTab="briefs"
			initialReleaseId={LONG_BRIEF_RELEASE_ID}
			releaseDetail={releaseDetailTranslationError}
		/>
	),
	play: async ({ canvasElement }) => {
		const body = within(canvasElement.ownerDocument.body);
		await expect(body.getByText("翻译失败")).toBeVisible();
		await expect(body.getByText("Markdown 结构校验失败")).toBeVisible();
		await expect(body.getByRole("button", { name: "重试翻译" })).toBeVisible();
	},
};
