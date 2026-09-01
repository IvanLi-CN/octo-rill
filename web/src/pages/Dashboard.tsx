import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	ArrowDownToLine,
	Eye,
	EyeOff,
	RefreshCcw,
	WifiOff,
} from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import {
	type MeResponse,
	type FollowingRepoItem,
	type FollowingReposResponse,
	type PersonalReposResponse,
	type RepoPublicReleasePublicationStatusResponse,
	ApiError,
	apiGet,
	apiGetFollowingRepos,
	apiHead,
	apiFollowRepo,
	apiGetRepoPublicReleasePublication,
	apiPost,
	apiPostJson,
	apiUnfollowRepo,
	apiPublishRepoPublicRelease,
	apiUnpublishRepoPublicRelease,
} from "@/api";
import { openAppEventSource } from "@/demo/eventSource";
import {
	buildDashboardWarmViewerStateKey,
	persistDashboardWarmSnapshot,
	type DashboardWarmSnapshot,
} from "@/auth/startupCache";
import { useAppToast } from "@/components/feedback/AppToast";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Tabs, TabsContent } from "@/components/ui/tabs";
import { FeedPageLaneSelector } from "@/feed/FeedPageLaneSelector";
import { FeedGroupedList } from "@/feed/FeedGroupedList";
import { FeedReadableSectionList } from "@/feed/FeedReadableSectionList";
import {
	DEFAULT_PAGE_LANE,
	isFeedLane,
	PAGE_DEFAULT_LANE_STORAGE_KEY,
	resolveDisplayLaneForFeed,
	resolvePreferredLaneForItem,
} from "@/feed/laneOptions";
import type {
	FeedReactionRefreshResponse,
	FeedItem,
	FeedLane,
	SmartItem,
	ReactionContent,
	ReleaseReactions,
	TranslatedItem,
	ToggleReleaseReactionResponse,
} from "@/feed/types";
import { isLaneCapableFeedItem, isReleaseFeedItem } from "@/feed/types";
import { useAutoSmart } from "@/feed/useAutoSmart";
import { useAutoTranslate } from "@/feed/useAutoTranslate";
import { type FeedRequestType, useFeed } from "@/feed/useFeed";
import { useDashboardReadableSections } from "@/feed/useDashboardReadableSections";
import { InboxList } from "@/inbox/InboxList";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";
import { InternalLink } from "@/lib/internalNavigation";
import {
	describeUnknownError,
	type NetworkErrorKind,
} from "@/lib/errorPresentation";
import {
	buildRichClipboardPayload,
	writeRichClipboard,
} from "@/lib/richClipboard";
import { useMediaQuery } from "@/lib/useMediaQuery";
import {
	buildDashboardReleaseTarget,
	buildDashboardRouteUrl,
	buildDashboardScopeHref,
	buildDashboardScopeSignature,
	buildDashboardWarmRouteState,
	isScopedDashboardRouteState,
	parseDashboardRouteStateFromLocation,
	releaseLocatorFromReleaseDetail,
	type DashboardScope,
	type DashboardReleaseTarget,
	type DashboardRouteState,
} from "@/dashboard/routeState";
import { AnnouncementDetailPage } from "@/dashboard/AnnouncementDetailPage";
import {
	DASHBOARD_FOLLOWING_ENTRY_LABEL,
	buildDashboardScopeSummary,
	DASHBOARD_MINE_ENTRY_LABEL,
	resolveDashboardScopeRepoNames,
} from "@/dashboard/scopeSummary";
import { RepoPublicReleaseControls } from "@/dashboard/RepoPublicReleaseControls";
import {
	type DashboardLiveUpdateNotice,
	useDashboardLiveUpdates,
} from "@/dashboard/useDashboardLiveUpdates";
import {
	DashboardMobileControlBand,
	DASHBOARD_TAB_OPTIONS,
	type DashboardTab as Tab,
	DashboardTabsList,
} from "@/pages/DashboardControlBand";
import { DashboardStartupSkeleton } from "@/pages/AppBoot";
import {
	DashboardHeader,
	type DashboardSyncProgress,
} from "@/pages/DashboardHeader";
import { buildSettingsHref, buildSettingsSearch } from "@/settings/routeState";
import {
	isReactionTokenUsable,
	useReactionTokenEditor,
} from "@/settings/reactionTokenEditor";
import { GitHubPatInput } from "@/settings/GitHubPatInput";
import { BriefListCard } from "@/sidebar/BriefListCard";
import {
	InboxQuickList,
	type NotificationItem,
} from "@/sidebar/InboxQuickList";
import { type BriefItem, ReleaseDailyCard } from "@/sidebar/ReleaseDailyCard";
import { ReleaseDetailCard } from "@/sidebar/ReleaseDetailCard";
import {
	dashboardBriefsQueryKey,
	dashboardNotificationsQueryKey,
	dashboardReactionTokenQueryKey,
	type DashboardBriefsQueryData,
	type DashboardNotificationsQueryData,
	type DashboardReactionTokenQueryData,
} from "@/query/dashboardQueryKeys";

type TaskAcceptedResponse = {
	mode: "task_id";
	task_id: string;
	task_type: string;
	status: string;
};

type TaskStreamMode = "access" | "refresh";

type TaskStreamState = {
	taskId: string;
	eventPath: string;
};

type DashboardSectionError = {
	phase: "initial" | "refresh";
	message: string;
	at: number;
};

type SidebarBootstrapNotificationsError = {
	kind: "sidebar-bootstrap-notifications";
	cause: unknown;
};

type FeedLiveBoundaryNotice = DashboardLiveUpdateNotice & {
	boundaryId: string;
	boundaryKeys: string[];
	boundaryAfterKey?: string | null;
	hydrated?: boolean;
	sealed?: boolean;
};

const SCOPED_TAB_OPTIONS = DASHBOARD_TAB_OPTIONS.filter(
	(option) => option.value === "all" || option.value === "releases",
);

type DashboardLiveNoticeState = {
	feed?: Partial<Record<FeedRequestType, FeedLiveBoundaryNotice[]>>;
	briefs?: DashboardLiveUpdateNotice;
	notifications?: DashboardLiveUpdateNotice;
};

function feedItemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

function mergeDashboardLiveNotice(
	current: DashboardLiveUpdateNotice | undefined,
	incoming: DashboardLiveUpdateNotice,
) {
	if (!current) return incoming;
	const latestKeys = Array.from(
		new Set([...incoming.latestKeys, ...current.latestKeys]),
	);
	return {
		...incoming,
		newCount: latestKeys.length,
		latestKeys,
	};
}

function makeFeedBoundaryNotice(
	notice: DashboardLiveUpdateNotice,
	feedType: FeedRequestType,
): FeedLiveBoundaryNotice {
	const latestKeys = notice.latestKeys.slice(0, notice.newCount);
	return {
		...notice,
		feedType,
		boundaryId: `${feedType}:${notice.newCount}:${latestKeys.join("|")}`,
		boundaryKeys: latestKeys,
		latestKeys,
	};
}

function mergeFeedBoundaryNotices(
	currentFeedNotices: FeedLiveBoundaryNotice[],
	incoming: FeedLiveBoundaryNotice,
) {
	if (
		currentFeedNotices.some(
			(currentNotice) => currentNotice.boundaryId === incoming.boundaryId,
		)
	) {
		return currentFeedNotices;
	}
	const latestNotice = currentFeedNotices[0];
	if (latestNotice && !latestNotice.sealed) {
		const latestKeys = Array.from(
			new Set([...incoming.latestKeys, ...latestNotice.latestKeys]),
		);
		const boundaryKeys = Array.from(
			new Set([...incoming.boundaryKeys, ...latestNotice.boundaryKeys]),
		);
		return [
			{
				...latestNotice,
				boundaryKeys,
				boundaryAfterKey: undefined,
				hydrated: true,
				newCount: latestKeys.length,
				latestKeys,
			},
			...currentFeedNotices.slice(1),
		];
	}
	return [incoming, ...currentFeedNotices];
}

type FeedScrollAnchor = {
	key: string;
	top: number;
};

function captureFeedScrollAnchor(): FeedScrollAnchor | null {
	if (typeof window === "undefined") return null;
	const viewportTop = 88;
	const viewportBottom = window.innerHeight;
	const itemElements = Array.from(
		document.querySelectorAll<HTMLElement>("[data-feed-item-key]"),
	);
	for (const element of itemElements) {
		const key = element.dataset.feedItemKey;
		if (!key) continue;
		const rect = element.getBoundingClientRect();
		if (rect.bottom <= viewportTop || rect.top >= viewportBottom) continue;
		return { key, top: rect.top };
	}
	return null;
}

function restoreFeedScrollAnchor(anchor: FeedScrollAnchor | null) {
	if (!anchor || typeof window === "undefined") return;
	window.requestAnimationFrame(() => {
		const element = document.querySelector<HTMLElement>(
			`[data-feed-item-key="${CSS.escape(anchor.key)}"]`,
		);
		if (!element) return;
		const delta = element.getBoundingClientRect().top - anchor.top;
		if (Math.abs(delta) < 0.5) return;
		window.scrollBy({ top: delta, behavior: "auto" });
	});
}

function NewContentNotice(props: {
	count: number;
	label: string;
	onReveal: () => void;
}) {
	const { count, label, onReveal } = props;
	if (count <= 0) return null;
	const countLabel = `${count} 条${label}`;
	return (
		<button
			type="button"
			className="dashboard-new-content-hint group mb-3 grid w-full grid-cols-[minmax(20px,1fr)_auto_minmax(20px,1fr)] items-center gap-3 py-1.5 text-left"
			data-dashboard-new-content-notice="true"
			onClick={onReveal}
		>
			<span className="dashboard-new-content-rule" aria-hidden="true" />
			<span className="dashboard-new-content-chip inline-flex items-center gap-2 rounded-full border px-3 py-1 font-mono text-[11px] font-medium">
				<ArrowDownToLine className="size-3.5" />
				<span>刚刚同步 · {countLabel}</span>
			</span>
			<span className="dashboard-new-content-rule" aria-hidden="true" />
		</button>
	);
}

function resolveLaneForItem(
	item: FeedItem,
	selectedLaneByKey: Record<string, FeedLane>,
	pageDefaultLane: FeedLane,
	allowItemOverride = true,
): FeedLane {
	const selected = allowItemOverride
		? selectedLaneByKey[feedItemKey(item)]
		: undefined;
	if (selected) {
		return resolvePreferredLaneForItem(item, selected);
	}
	return resolvePreferredLaneForItem(item, pageDefaultLane);
}

function filterFeedItemsForTab(
	items: FeedItem[],
	tab: "all" | "releases" | "stars" | "followers",
	options?: {
		scoped?: boolean;
	},
) {
	const scoped = options?.scoped ?? false;
	switch (tab) {
		case "releases":
			return items.filter((item) => item.kind === "release");
		case "stars":
			return items.filter((item) => item.kind === "repo_star_received");
		case "followers":
			return items.filter((item) => item.kind === "follower_received");
		default:
			return scoped
				? items.filter(
						(item) =>
							item.kind === "release" ||
							item.kind === "repo_star_received" ||
							item.kind === "announcement" ||
							item.kind === "release_update" ||
							item.kind === "repo_forked",
					)
				: items;
	}
}

type TaskEventPayload = {
	stage?: string;
	status?: string;
	error?: string;
	[key: string]: unknown;
};
type BriefGenerateResponse = {
	id: string;
	date: string;
	window_start: string | null;
	window_end: string | null;
	effective_time_zone: string | null;
	effective_local_boundary: string | null;
	release_count: number;
	release_ids: string[];
	content_markdown: string;
};

const SYNC_ALL_LABEL = "同步";
const TASK_STREAM_RECOVERY_GRACE_MS = 5000;
const ACCESS_SYNC_TOTAL_STEPS = 4;
const FEED_REACTION_REFRESH_TTL_MS = 15_000;
const FEED_REACTION_REFRESH_BATCH_SIZE = 100;
const REACTION_CONTENTS: ReactionContent[] = [
	"plus1",
	"laugh",
	"heart",
	"hooray",
	"rocket",
	"eyes",
];

type DashboardSessionState = {
	scopeSignature: string | null;
	notifications: NotificationItem[];
	briefs: BriefItem[];
	selectedBriefId: string | null;
	shellHydrated: boolean;
	sidebarBootstrapped: boolean;
	notificationsBootstrapped: boolean;
	reactionTokenBootstrapped: boolean;
	reactionTokenConfigured: boolean | null;
};

const dashboardSessionStateByUser = new Map<string, DashboardSessionState>();

function mergeBriefSummariesWithCachedDetails(
	summaries: BriefItem[],
	cached: BriefItem[],
) {
	const cachedById = new Map(cached.map((brief) => [brief.id, brief]));
	return summaries.map((summary) => {
		const existing = cachedById.get(summary.id);
		if (
			existing?.content_markdown &&
			existing.updated_at &&
			summary.updated_at &&
			existing.updated_at === summary.updated_at
		) {
			return {
				...summary,
				content_markdown: existing.content_markdown,
			};
		}
		return summary;
	});
}

function resolveSelectedBrief(briefs: BriefItem[], selectedId: string | null) {
	if (selectedId) {
		const found = briefs.find((brief) => brief.id === selectedId);
		if (found) return found;
	}
	return briefs[0] ?? null;
}

function waitForNextPaint() {
	return new Promise<void>((resolve) => {
		if (typeof window === "undefined") {
			resolve();
			return;
		}
		window.requestAnimationFrame(() => resolve());
	});
}

function resolveRepoPublicReleaseUrl(
	status: RepoPublicReleasePublicationStatusResponse | null,
	scope: DashboardScope,
) {
	if (scope.kind !== "repo") return null;
	const path = status?.public_path ?? `/${scope.owner}/${scope.repo}/releases`;
	if (typeof window === "undefined") return path;
	return new URL(path, window.location.origin).toString();
}

function followingRepoSourceText(repo: FollowingRepoItem) {
	const labels: string[] = [];
	if (repo.sources.personal_owned) labels.push("个人仓库");
	if (repo.sources.github_star) labels.push("GitHub Star");
	if (repo.sources.manual_feed) labels.push("手动 Feed");
	return labels.join(" · ");
}

type FollowingListView = "following" | "associated";
type FollowingRepoFollowOverrides = Record<string, boolean>;

function normalizeFollowingRepoKey(fullName: string) {
	return fullName.trim().toLowerCase();
}

function applyFollowingRepoOverrides(
	response: FollowingReposResponse | null,
	overrides: FollowingRepoFollowOverrides,
): FollowingReposResponse | null {
	if (!response) {
		return null;
	}
	if (Object.keys(overrides).length === 0) {
		return response;
	}
	const associatedItems = response.associated_items.map((repo) => {
		const override = overrides[normalizeFollowingRepoKey(repo.full_name)];
		if (override === undefined) {
			return repo;
		}
		return {
			...repo,
			is_following: override,
			follow_state_source: "user_explicit",
		};
	});
	const followingItems = associatedItems.filter((repo) => repo.is_following);
	return {
		...response,
		items: followingItems,
		associated_items: associatedItems,
		following_count: followingItems.length,
		associated_count: associatedItems.length,
	};
}

function updateFollowingRepoSnapshot(
	items: FollowingRepoItem[],
	fullName: string,
	isFollowing: boolean,
) {
	const normalizedFullName = normalizeFollowingRepoKey(fullName);
	return items.map((repo) =>
		normalizeFollowingRepoKey(repo.full_name) === normalizedFullName
			? {
					...repo,
					is_following: isFollowing,
					follow_state_source: "user_explicit",
				}
			: repo,
	);
}

function mergeFollowingRepoSnapshot(
	items: FollowingRepoItem[] | null,
	response: FollowingReposResponse | null,
	overrides: FollowingRepoFollowOverrides,
): FollowingRepoItem[] | null {
	if (!items) {
		return null;
	}
	return items.map((repo) => {
		const normalizedFullName = normalizeFollowingRepoKey(repo.full_name);
		const canonicalRepo =
			response?.associated_items.find(
				(item) =>
					normalizeFollowingRepoKey(item.full_name) === normalizedFullName,
			) ?? null;
		if (canonicalRepo) {
			return canonicalRepo;
		}
		const override = overrides[normalizedFullName];
		if (override === undefined) {
			return repo;
		}
		return {
			...repo,
			is_following: override,
			follow_state_source: "user_explicit",
		};
	});
}

function ScopedSummaryCard(props: {
	scope: DashboardScope;
	feedItems: FeedItem[];
	desktop?: boolean;
	personalRepos?: PersonalReposResponse | null;
	personalReposLoading?: boolean;
	personalReposError?: string | null;
	followingRepos?: FollowingReposResponse | null;
	followingReposLoading?: boolean;
	followingReposError?: string | null;
	reloadFollowingRepos?: () => Promise<unknown>;
}) {
	const {
		scope,
		feedItems,
		desktop = false,
		personalRepos = null,
		personalReposLoading = false,
		personalReposError = null,
		followingRepos = null,
		followingReposLoading = false,
		followingReposError = null,
		reloadFollowingRepos,
	} = props;
	const { pushErrorToast, pushToast } = useAppToast();
	const [publicationStatus, setPublicationStatus] =
		useState<RepoPublicReleasePublicationStatusResponse | null>(null);
	const [publicationLoading, setPublicationLoading] = useState(false);
	const [publicationBusy, setPublicationBusy] = useState<
		"publish" | "unpublish" | null
	>(null);
	const [publicationError, setPublicationError] = useState<string | null>(null);
	const [followBusy, setFollowBusy] = useState<"follow" | "unfollow" | null>(
		null,
	);
	const [followingListView, setFollowingListView] =
		useState<FollowingListView>("following");
	const [followingRepoOverrides, setFollowingRepoOverrides] =
		useState<FollowingRepoFollowOverrides>({});
	const [followingListSnapshots, setFollowingListSnapshots] = useState<
		Record<FollowingListView, FollowingRepoItem[] | null>
	>({
		following: null,
		associated: null,
	});
	const [repoWarmState, setRepoWarmState] = useState<
		"idle" | "pending" | "ready"
	>("idle");
	const effectiveFollowingRepos = useMemo(
		() => applyFollowingRepoOverrides(followingRepos, followingRepoOverrides),
		[followingRepoOverrides, followingRepos],
	);
	const feedRepoNames = Array.from(
		new Set(
			feedItems
				.map((item) => item.repo_full_name?.trim())
				.filter((value): value is string => Boolean(value)),
		),
	);
	const repoNames =
		scope.kind === "mine" && personalRepos
			? personalRepos.repos.map((repo) => repo.full_name)
			: scope.kind === "following" && effectiveFollowingRepos
				? effectiveFollowingRepos.items.map((repo) => repo.full_name)
				: resolveDashboardScopeRepoNames(scope, feedRepoNames);
	const repoCount =
		scope.kind === "mine" && personalRepos
			? personalRepos.total_count
			: scope.kind === "following" && effectiveFollowingRepos
				? effectiveFollowingRepos.following_count
				: repoNames.length;
	const releaseCount = feedItems.filter(
		(item) => item.kind === "release",
	).length;
	const activityCount = feedItems.length - releaseCount;
	const summary = buildDashboardScopeSummary(scope, repoCount);
	const repoChipLimit = desktop ? 8 : 6;
	const personalRepoItems =
		scope.kind === "mine" && personalRepos ? personalRepos.repos : [];
	const followingRepoItems =
		scope.kind === "following" && effectiveFollowingRepos
			? effectiveFollowingRepos.items
			: [];
	const associatedRepoItems =
		scope.kind === "following" && effectiveFollowingRepos
			? effectiveFollowingRepos.associated_items
			: [];
	const followingRepoCount =
		scope.kind === "following" && effectiveFollowingRepos
			? effectiveFollowingRepos.following_count
			: followingRepoItems.length;
	const associatedRepoCount =
		scope.kind === "following" && effectiveFollowingRepos
			? effectiveFollowingRepos.associated_count
			: associatedRepoItems.length;
	const stickyFollowingItems = mergeFollowingRepoSnapshot(
		followingListSnapshots.following,
		effectiveFollowingRepos,
		followingRepoOverrides,
	);
	const stickyAssociatedItems = mergeFollowingRepoSnapshot(
		followingListSnapshots.associated,
		effectiveFollowingRepos,
		followingRepoOverrides,
	);
	const followingListItems =
		followingListView === "associated"
			? (stickyAssociatedItems ?? associatedRepoItems)
			: (stickyFollowingItems ?? followingRepoItems);
	const visibleRepoNames = repoNames.slice(0, repoChipLimit);
	const publicReleaseUrl = resolveRepoPublicReleaseUrl(
		publicationStatus,
		scope,
	);
	const repoScope =
		scope.kind === "repo"
			? {
					owner: scope.owner,
					repo: scope.repo,
				}
			: null;
	const currentFollowingRepo = repoScope
		? (effectiveFollowingRepos?.associated_items.find(
				(repo) =>
					repo.full_name.toLowerCase() ===
					`${repoScope.owner}/${repoScope.repo}`.toLowerCase(),
			) ??
			effectiveFollowingRepos?.items.find(
				(repo) =>
					repo.full_name.toLowerCase() ===
					`${repoScope.owner}/${repoScope.repo}`.toLowerCase(),
			) ??
			null)
		: null;
	const repoIsFollowing = Boolean(currentFollowingRepo?.is_following);

	useEffect(() => {
		if (scope.kind !== "following") {
			setFollowingListView("following");
		}
		setFollowingListSnapshots({
			following: null,
			associated: null,
		});
	}, [scope.kind]);

	useEffect(() => {
		setFollowingRepoOverrides({});
	}, [followingRepos]);

	const handleFollowingListViewChange = useCallback(
		(nextView: FollowingListView) => {
			setFollowingListView(nextView);
			setFollowingListSnapshots({
				following: null,
				associated: null,
			});
		},
		[],
	);

	const preserveCurrentFollowingList = useCallback(
		(fullName: string, isFollowing: boolean) => {
			if (scope.kind !== "following") {
				return;
			}
			setFollowingListSnapshots((current) => ({
				...current,
				[followingListView]: updateFollowingRepoSnapshot(
					followingListItems,
					fullName,
					isFollowing,
				),
			}));
		},
		[followingListItems, followingListView, scope.kind],
	);

	useEffect(() => {
		if (!repoScope) {
			setPublicationStatus(null);
			setPublicationError(null);
			setPublicationLoading(false);
			return;
		}
		let cancelled = false;
		setPublicationLoading(true);
		setPublicationError(null);
		apiGetRepoPublicReleasePublication(repoScope)
			.then((response) => {
				if (cancelled) return;
				setPublicationStatus(response);
			})
			.catch((err: unknown) => {
				if (cancelled) return;
				const message =
					err instanceof Error ? err.message : "无法读取公开发布状态";
				setPublicationError(message);
			})
			.finally(() => {
				if (!cancelled) setPublicationLoading(false);
			});
		return () => {
			cancelled = true;
		};
	}, [repoScope?.owner, repoScope?.repo]);

	useEffect(() => {
		if (!repoScope) {
			setRepoWarmState("idle");
			return;
		}
		let cancelled = false;
		apiHead(
			`/api/feed?scope=repo&items=${encodeURIComponent(`${repoScope.owner}/${repoScope.repo}`)}`,
		)
			.then((status) => {
				if (cancelled) return;
				setRepoWarmState(status === 202 ? "pending" : "ready");
			})
			.catch(() => {
				if (cancelled) return;
				setRepoWarmState("idle");
			});
		return () => {
			cancelled = true;
		};
	}, [repoScope?.owner, repoScope?.repo]);

	const publishRepo = useCallback(() => {
		if (!repoScope) return;
		setPublicationBusy("publish");
		apiPublishRepoPublicRelease(repoScope)
			.then((response) => {
				setPublicationStatus(response);
				setPublicationError(null);
				pushToast({
					title: "公开页已发布",
					description: "这个私有仓库的 Release 页现在可以匿名访问。",
				});
			})
			.catch((err: unknown) => {
				pushErrorToast(
					"发布公开页失败",
					describeUnknownError(err, "请稍后重试"),
				);
			})
			.finally(() => setPublicationBusy(null));
	}, [pushErrorToast, pushToast, repoScope]);

	const unpublishRepo = useCallback(() => {
		if (!repoScope) return;
		setPublicationBusy("unpublish");
		apiUnpublishRepoPublicRelease(repoScope)
			.then((response) => {
				setPublicationStatus(response);
				setPublicationError(null);
				pushToast({
					title: "公开页已取消",
					description: "未登录用户将不再能访问这个私有仓库的 Release 页。",
				});
			})
			.catch((err: unknown) => {
				pushErrorToast("取消发布失败", describeUnknownError(err, "请稍后重试"));
			})
			.finally(() => setPublicationBusy(null));
	}, [pushErrorToast, pushToast, repoScope]);

	const copyPublicReleaseUrl = useCallback(() => {
		if (!publicReleaseUrl) return;
		void navigator.clipboard
			.writeText(publicReleaseUrl)
			.then(() => {
				pushToast({
					title: "地址已复制",
					description: publicReleaseUrl,
				});
			})
			.catch((err: unknown) => {
				pushErrorToast(
					"复制地址失败",
					describeUnknownError(err, "请手动复制地址"),
				);
			});
	}, [publicReleaseUrl, pushErrorToast, pushToast]);

	const followRepo = useCallback(
		(target?: { owner: string; repo: string; fullName?: string }) => {
			const effectiveTarget =
				target ??
				(repoScope ? { owner: repoScope.owner, repo: repoScope.repo } : null);
			if (!effectiveTarget) return;
			const fullName =
				effectiveTarget.fullName ??
				`${effectiveTarget.owner}/${effectiveTarget.repo}`;
			const normalizedFullName = normalizeFollowingRepoKey(fullName);
			setFollowingRepoOverrides((current) => ({
				...current,
				[normalizedFullName]: true,
			}));
			preserveCurrentFollowingList(fullName, true);
			setFollowBusy("follow");
			apiFollowRepo(effectiveTarget)
				.then(async () => {
					await reloadFollowingRepos?.();
					pushToast({
						title: "已关注仓库",
						description: `${fullName} 已加入发布关注范围。`,
					});
				})
				.catch((err: unknown) => {
					setFollowingRepoOverrides((current) => {
						const next = { ...current };
						delete next[normalizedFullName];
						return next;
					});
					setFollowingListSnapshots({
						following: null,
						associated: null,
					});
					pushErrorToast("关注失败", describeUnknownError(err, "请稍后重试"));
				})
				.finally(() => setFollowBusy(null));
		},
		[
			preserveCurrentFollowingList,
			pushErrorToast,
			pushToast,
			reloadFollowingRepos,
			repoScope,
		],
	);

	const unfollowRepo = useCallback(
		(target?: { owner: string; repo: string; fullName?: string }) => {
			const effectiveTarget =
				target ??
				(repoScope ? { owner: repoScope.owner, repo: repoScope.repo } : null);
			if (!effectiveTarget) return;
			const fullName =
				effectiveTarget.fullName ??
				`${effectiveTarget.owner}/${effectiveTarget.repo}`;
			const normalizedFullName = normalizeFollowingRepoKey(fullName);
			setFollowingRepoOverrides((current) => ({
				...current,
				[normalizedFullName]: false,
			}));
			preserveCurrentFollowingList(fullName, false);
			setFollowBusy("unfollow");
			apiUnfollowRepo(effectiveTarget)
				.then(async () => {
					await reloadFollowingRepos?.();
					pushToast({
						title: "已取消关注",
						description: fullName,
					});
				})
				.catch((err: unknown) => {
					setFollowingRepoOverrides((current) => {
						const next = { ...current };
						delete next[normalizedFullName];
						return next;
					});
					setFollowingListSnapshots({
						following: null,
						associated: null,
					});
					pushErrorToast(
						"取消关注失败",
						describeUnknownError(err, "请稍后重试"),
					);
				})
				.finally(() => setFollowBusy(null));
		},
		[
			preserveCurrentFollowingList,
			pushErrorToast,
			pushToast,
			reloadFollowingRepos,
			repoScope,
		],
	);

	return (
		<div
			className={[
				"rounded-[28px] border border-border/70 bg-card/82 shadow-sm backdrop-blur",
				desktop ? "p-5" : "mb-4 p-4 sm:p-5",
			].join(" ")}
			data-dashboard-scope-summary={scope.kind}
			data-dashboard-scope-summary-layout={desktop ? "desktop" : "mobile"}
		>
			<div className="flex items-start justify-between gap-3">
				<div className="min-w-0">
					<p className="font-mono text-[11px] uppercase tracking-[0.22em] text-muted-foreground">
						{summary.kicker}
					</p>
					<h2 className="mt-1 text-xl font-semibold tracking-tight text-foreground">
						{summary.title}
					</h2>
				</div>
				<div className="inline-flex shrink-0 items-center gap-2 rounded-full border border-border/70 bg-muted/35 px-3 py-1 font-mono text-[11px] text-muted-foreground">
					<span>{summary.chip}</span>
					<span aria-hidden="true">·</span>
					<span>{summary.secondary}</span>
				</div>
			</div>

			<p className="mt-3 text-sm leading-6 text-muted-foreground">
				{summary.description}
			</p>

			{scope.kind === "following" ? (
				<div
					className="mt-4 grid gap-2 sm:grid-cols-2"
					data-dashboard-following-stat-grid="true"
				>
					<button
						type="button"
						aria-pressed={followingListView === "following"}
						data-dashboard-following-stat="following"
						onClick={() => handleFollowingListViewChange("following")}
						className={[
							"rounded-xl px-4 py-3 text-left transition-colors",
							followingListView === "following"
								? "bg-muted/38 text-foreground"
								: "bg-transparent text-muted-foreground hover:bg-muted/20 hover:text-foreground",
						].join(" ")}
					>
						<p className="font-mono text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
							关注仓库
						</p>
						<p className="mt-1 text-lg font-semibold text-foreground">
							{followingRepoCount}
						</p>
					</button>
					<button
						type="button"
						aria-pressed={followingListView === "associated"}
						data-dashboard-following-stat="associated"
						onClick={() => handleFollowingListViewChange("associated")}
						className={[
							"rounded-xl px-4 py-3 text-left transition-colors",
							followingListView === "associated"
								? "bg-muted/38 text-foreground"
								: "bg-transparent text-muted-foreground hover:bg-muted/20 hover:text-foreground",
						].join(" ")}
					>
						<p className="font-mono text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
							关联仓库
						</p>
						<p className="mt-1 text-lg font-semibold text-foreground">
							{associatedRepoCount}
						</p>
					</button>
				</div>
			) : (
				<div className="mt-4 grid gap-3 sm:grid-cols-2">
					<div className="rounded-2xl border border-border/65 bg-background/72 px-4 py-3">
						<p className="font-mono text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
							发布
						</p>
						<p className="mt-1 text-lg font-semibold text-foreground">
							{releaseCount}
						</p>
					</div>
					<div className="rounded-2xl border border-border/65 bg-background/72 px-4 py-3">
						<p className="font-mono text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
							动态
						</p>
						<p className="mt-1 text-lg font-semibold text-foreground">
							{activityCount}
						</p>
					</div>
				</div>
			)}

			{scope.kind === "mine" && personalReposLoading && !personalRepos ? (
				<p className="mt-4 rounded-2xl border border-dashed border-border/65 px-3 py-2 text-sm text-muted-foreground">
					正在加载个人仓库…
				</p>
			) : null}

			{scope.kind === "mine" && personalReposError ? (
				<p className="mt-4 rounded-2xl border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
					个人仓库清单加载失败，当前动态仍可继续浏览。
				</p>
			) : null}

			{scope.kind === "following" &&
			followingReposLoading &&
			!followingRepos ? (
				<p className="mt-4 rounded-2xl border border-dashed border-border/65 px-3 py-2 text-sm text-muted-foreground">
					正在加载关注仓库…
				</p>
			) : null}

			{scope.kind === "following" && followingReposError ? (
				<p className="mt-4 rounded-2xl border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
					关注仓库清单加载失败，当前动态仍可继续浏览。
				</p>
			) : null}

			{scope.kind === "repo" ? (
				<div className="mt-4 rounded-2xl border border-border/65 bg-background/72 px-4 py-3">
					<div className="flex items-center justify-between gap-3">
						<div className="min-w-0">
							<p className="font-mono text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
								关注状态
							</p>
							<p className="mt-1 text-sm text-foreground">
								{repoIsFollowing
									? "已关注，进入全局发布范围"
									: "未关注，仅按需读取"}
							</p>
						</div>
						<Button
							type="button"
							variant="outline"
							size="icon"
							className="rounded-full border-border/70 bg-background/72 text-foreground hover:bg-background/72 hover:text-foreground"
							disabled={followBusy !== null}
							aria-label={
								followBusy === "follow"
									? "关注中"
									: followBusy === "unfollow"
										? "取消关注中"
										: repoIsFollowing
											? "取消关注"
											: "关注仓库"
							}
							title={repoIsFollowing ? "取消关注" : "关注仓库"}
							onClick={
								repoIsFollowing ? () => unfollowRepo() : () => followRepo()
							}
						>
							{repoIsFollowing ? (
								<Eye className="size-4" />
							) : (
								<EyeOff className="size-4" />
							)}
						</Button>
					</div>
				</div>
			) : null}

			{scope.kind === "repo" &&
			repoWarmState === "pending" &&
			releaseCount === 0 ? (
				<p className="mt-4 rounded-2xl border border-dashed border-border/65 px-3 py-2 text-sm text-muted-foreground">
					这个公开仓库正在预热本地 Release 缓存，稍后刷新即可看到发布内容。
				</p>
			) : null}

			{personalRepoItems.length > 0 ? (
				<div className="mt-4 overflow-hidden rounded-2xl border border-border/65 bg-background/72">
					<div className="flex items-center justify-between border-b border-border/60 px-3 py-2">
						<p className="text-sm font-medium text-foreground">仓库列表</p>
						<p className="font-mono text-[11px] text-muted-foreground">
							{personalRepos?.total_count ?? personalRepoItems.length} 个
						</p>
					</div>
					<ul
						className="max-h-80 scroll-pb-3 overflow-y-auto pb-3"
						data-dashboard-personal-repo-list="true"
					>
						{personalRepoItems.map((repo) => {
							const href = buildDashboardScopeHref({
								kind: "repo",
								owner: repo.owner_login,
								repo: repo.name,
							});
							const releaseLabel =
								repo.release_count > 0
									? `${repo.release_count} 个发布`
									: "暂无发布";
							return (
								<li
									key={repo.full_name}
									className="border-b border-border/50 last:border-b-0"
								>
									<InternalLink
										href={href}
										to={href}
										className="group flex min-w-0 items-center justify-between gap-3 px-3 py-2.5 text-left transition-colors hover:bg-muted/35 focus-visible:bg-muted/35 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/45"
									>
										<span className="min-w-0">
											<span className="block truncate font-mono text-[12px] font-medium text-foreground">
												{repo.full_name}
											</span>
											<span className="mt-0.5 block truncate text-xs text-muted-foreground">
												更新 {formatPersonalRepoUpdatedAt(repo.updated_at)}
											</span>
										</span>
										<span className="shrink-0 rounded-full border border-border/65 bg-card/70 px-2 py-0.5 text-[11px] text-muted-foreground group-hover:text-foreground">
											{releaseLabel}
										</span>
									</InternalLink>
								</li>
							);
						})}
					</ul>
				</div>
			) : scope.kind === "following" ? (
				<div className="mt-4 border-t border-border/60 pt-4">
					<div className="flex items-center justify-between gap-3">
						<p className="text-sm font-medium text-foreground">
							{followingListView === "following" ? "关注仓库" : "关联仓库"}
						</p>
						<p className="font-mono text-[11px] text-muted-foreground">
							{followingListItems.length} 个
						</p>
					</div>
					<ul
						className="mt-3 max-h-80 divide-y divide-border/50 overflow-y-auto"
						data-dashboard-following-repo-list={followingListView}
					>
						{followingListItems.map((repo) => {
							const href = buildDashboardScopeHref({
								kind: "repo",
								owner: repo.owner_login,
								repo: repo.name,
							});
							return (
								<li key={repo.full_name} className="py-3 first:pt-0 last:pb-0">
									<div className="flex items-start justify-between gap-3">
										<div className="min-w-0">
											<InternalLink
												href={href}
												to={href}
												className="block truncate font-mono text-[12px] font-medium text-foreground"
											>
												{repo.full_name}
											</InternalLink>
											<p className="mt-0.5 text-xs text-muted-foreground">
												{followingRepoSourceText(repo)}
											</p>
											<p className="mt-0.5 text-xs text-muted-foreground">
												首次关联{" "}
												{formatPersonalRepoUpdatedAt(repo.first_associated_at)}
											</p>
										</div>
										<Button
											type="button"
											size="icon"
											variant="outline"
											className="rounded-full border-border/70 bg-background/72 text-foreground hover:bg-background/72 hover:text-foreground"
											disabled={followBusy !== null}
											aria-label={repo.is_following ? "取消关注" : "关注仓库"}
											title={repo.is_following ? "取消关注" : "关注仓库"}
											onClick={() => {
												if (repo.is_following) {
													unfollowRepo({
														owner: repo.owner_login,
														repo: repo.name,
														fullName: repo.full_name,
													});
													return;
												}
												followRepo({
													owner: repo.owner_login,
													repo: repo.name,
													fullName: repo.full_name,
												});
											}}
										>
											{repo.is_following ? (
												<Eye className="size-4" />
											) : (
												<EyeOff className="size-4" />
											)}
										</Button>
									</div>
								</li>
							);
						})}
					</ul>
				</div>
			) : repoNames.length > 0 ? (
				<div className="mt-4 flex flex-wrap gap-2">
					{visibleRepoNames.map((repo) => {
						const [owner, repoName] = repo.split("/", 2);
						const href =
							owner && repoName
								? buildDashboardScopeHref({
										kind: "repo",
										owner,
										repo: repoName,
									})
								: null;
						const className =
							"inline-flex items-center rounded-full border border-border/65 bg-background/72 px-3 py-1 font-mono text-[11px] text-foreground/78";
						return href ? (
							<InternalLink
								key={repo}
								href={href}
								to={href}
								className={className}
							>
								{repo}
							</InternalLink>
						) : (
							<span key={repo} className={className}>
								{repo}
							</span>
						);
					})}
					{repoNames.length > repoChipLimit ? (
						<span className="inline-flex items-center rounded-full border border-dashed border-border/65 px-3 py-1 font-mono text-[11px] text-muted-foreground">
							+{repoNames.length - repoChipLimit} 个仓库
						</span>
					) : null}
				</div>
			) : null}

			{scope.kind === "repo" ? (
				<RepoPublicReleaseControls
					status={publicationStatus}
					publicUrl={publicReleaseUrl}
					loading={publicationLoading}
					busy={publicationBusy}
					error={publicationError}
					onPublish={publishRepo}
					onUnpublish={unpublishRepo}
					onCopy={copyPublicReleaseUrl}
				/>
			) : null}
		</div>
	);
}

function sortNotifications(items: NotificationItem[]) {
	return items.slice().sort((a, b) => {
		if (a.unread !== b.unread) return b.unread - a.unread;
		const at = a.updated_at ?? "";
		const bt = b.updated_at ?? "";
		return bt.localeCompare(at);
	});
}

function isSidebarBootstrapNotificationsError(
	error: unknown,
): error is SidebarBootstrapNotificationsError {
	return (
		typeof error === "object" &&
		error !== null &&
		"kind" in error &&
		error.kind === "sidebar-bootstrap-notifications"
	);
}

function parseDashboardQuery() {
	return parseDashboardRouteStateFromLocation(
		window.location.pathname,
		window.location.search,
	);
}

function readStoredPageDefaultLane() {
	if (typeof window === "undefined") {
		return DEFAULT_PAGE_LANE;
	}
	const stored = window.localStorage.getItem(PAGE_DEFAULT_LANE_STORAGE_KEY);
	return isFeedLane(stored) ? stored : DEFAULT_PAGE_LANE;
}

function itemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

function sessionExpiredHint() {
	return `当前页面（${window.location.origin}）的 OctoRill 登录已失效（不是 PAT 本身）。请先点右上角 Logout，再重新 Login with GitHub；若同时开了多个本地实例，请只保留这个端口。`;
}

function readPayloadNumber(payload: TaskEventPayload, key: string) {
	const value = payload[key];
	return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function pluralCount(count: number, label: string) {
	return `${count} ${label}`;
}

function joinSyncDetails(parts: string[]) {
	return parts.filter(Boolean).join(" · ");
}

function accessSyncProgressFromStage(
	stage:
		| "waiting"
		| "running"
		| "star_refreshed"
		| "release_summary"
		| "social_summary"
		| "notifications_summary",
	payload: TaskEventPayload = {},
): DashboardSyncProgress {
	switch (stage) {
		case "running":
			return {
				currentStep: 0,
				totalSteps: ACCESS_SYNC_TOTAL_STEPS,
				stageLabel: "后台任务已启动",
				detail: "正在准备 Star 阶段",
			};
		case "star_refreshed": {
			const repos = readPayloadNumber(payload, "repos");
			return {
				currentStep: 1,
				totalSteps: ACCESS_SYNC_TOTAL_STEPS,
				stageLabel: "Star 与仓库快照已同步",
				detail:
					repos !== null
						? `已刷新 ${pluralCount(repos, "个仓库")}`
						: "正在整理你的仓库快照",
			};
		}
		case "release_summary": {
			const releases = readPayloadNumber(payload, "releases");
			const repos = readPayloadNumber(payload, "repos");
			const failed = readPayloadNumber(payload, "failed");
			return {
				currentStep: 2,
				totalSteps: ACCESS_SYNC_TOTAL_STEPS,
				stageLabel: "Release 已同步",
				detail:
					joinSyncDetails([
						releases !== null
							? `写入 ${pluralCount(releases, "条 Release")}`
							: "",
						repos !== null ? `覆盖 ${pluralCount(repos, "个仓库")}` : "",
						failed !== null && failed > 0 ? `失败 ${failed}` : "",
					]) || "正在更新 Release 记录",
			};
		}
		case "social_summary": {
			const repoStars = readPayloadNumber(payload, "repo_stars");
			const followers = readPayloadNumber(payload, "followers");
			const events = readPayloadNumber(payload, "events");
			return {
				currentStep: 3,
				totalSteps: ACCESS_SYNC_TOTAL_STEPS,
				stageLabel: "社交动态已同步",
				detail:
					joinSyncDetails([
						repoStars !== null ? `仓库获星 ${repoStars}` : "",
						followers !== null ? `关注者 ${followers}` : "",
						events !== null ? `事件 ${events}` : "",
					]) || "正在整理 Star 与关注事件",
			};
		}
		case "notifications_summary": {
			const notifications = readPayloadNumber(payload, "notifications");
			return {
				currentStep: 4,
				totalSteps: ACCESS_SYNC_TOTAL_STEPS,
				stageLabel: "Inbox 已同步",
				detail:
					notifications !== null
						? `拉取 ${pluralCount(notifications, "条通知")}`
						: "正在刷新 Inbox 通知",
			};
		}
		default:
			return {
				currentStep: 0,
				totalSteps: ACCESS_SYNC_TOTAL_STEPS,
				stageLabel: "等待后台任务开始",
				detail: "正在连接任务事件流",
			};
	}
}

function formatDateTime(value: string | null | undefined) {
	if (!value) return "—";
	const date = new Date(value);
	return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function formatPersonalRepoUpdatedAt(value: string | null | undefined) {
	if (!value) return "—";
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return value;
	return date.toLocaleDateString();
}

function buildOptimisticReactions(
	current: ReleaseReactions,
	content: ReactionContent,
): ReleaseReactions {
	const viewer = { ...current.viewer };
	const counts = { ...current.counts };
	const hasReacted = viewer[content];
	viewer[content] = !hasReacted;
	counts[content] = Math.max(0, counts[content] + (hasReacted ? -1 : 1));
	return {
		...current,
		viewer,
		counts,
	};
}

function itemFromKey(key: string): Pick<FeedItem, "kind" | "id"> | null {
	const [kind, id] = key.split(":", 2);
	if (kind !== "release" || !id) return null;
	return { kind: "release", id };
}
function firstPendingReactionContent(
	server: ReleaseReactions,
	desired: ReleaseReactions,
): ReactionContent | null {
	return (
		REACTION_CONTENTS.find(
			(content) => server.viewer[content] !== desired.viewer[content],
		) ?? null
	);
}

export function Dashboard(props: {
	me: MeResponse;
	routeState?: DashboardRouteState;
	onRouteStateChange?: (
		nextRouteState: DashboardRouteState,
		options?: {
			replace?: boolean;
		},
	) => void;
	warmStart?: DashboardWarmSnapshot | null;
	bootError?: string | null;
	bootErrorKind?: NetworkErrorKind | null;
	bootErrorDetail?: string | null;
	onRetryBoot?: () => unknown | Promise<unknown>;
}) {
	const {
		me,
		routeState: controlledRouteState,
		onRouteStateChange,
		warmStart = null,
		bootError = null,
		bootErrorKind = null,
		bootErrorDetail = null,
		onRetryBoot,
	} = props;
	const { dismissToast, pushErrorToast, pushToast } = useAppToast();
	const queryClient = useQueryClient();
	const isRouteControlled = controlledRouteState !== undefined;
	const isAdmin = me.user.is_admin;
	const [dailyBoundaryLocal, _setDailyBoundaryLocal] = useState(
		me.dashboard.daily_boundary_local,
	);
	const [dailyBoundaryTimeZone, _setDailyBoundaryTimeZone] = useState(
		me.dashboard.daily_boundary_time_zone,
	);
	const [dailyBoundaryUtcOffsetMinutes, _setDailyBoundaryUtcOffsetMinutes] =
		useState(me.dashboard.daily_boundary_utc_offset_minutes);
	const accessSync = me.access_sync ?? {
		task_id: null,
		task_type: null,
		event_path: null,
		reason: "none" as const,
	};
	const initialAccessTask =
		accessSync.task_id && accessSync.event_path
			? {
					taskId: accessSync.task_id,
					eventPath: accessSync.event_path,
					mode: "access" as const,
				}
			: null;
	const initialRouteState = controlledRouteState ?? parseDashboardQuery();
	const scopeSignature = buildDashboardScopeSignature(initialRouteState.scope);
	const storedSessionState =
		dashboardSessionStateByUser.get(me.user.id) ?? null;
	const hasHydratedSessionShell = storedSessionState?.shellHydrated === true;
	const sessionState =
		storedSessionState?.scopeSignature === scopeSignature
			? storedSessionState
			: null;

	const [busy, setBusy] = useState<string | null>(null);
	const [hydrationSource] = useState<"warm-cache" | "network">(() =>
		warmStart ? "warm-cache" : "network",
	);
	const [bootedFromWarmStart] = useState(
		() => warmStart !== null || hasHydratedSessionShell,
	);
	const [shellHydrated, setShellHydrated] = useState(
		() => warmStart !== null || hasHydratedSessionShell,
	);
	const [accessTaskStream, setAccessTaskStream] =
		useState<TaskStreamState | null>(initialAccessTask);
	const [refreshTaskStreams, setRefreshTaskStreams] = useState<
		TaskStreamState[]
	>([]);
	const [accessSyncStage, setAccessSyncStage] = useState<
		"idle" | "waiting" | "running" | "star_refreshed" | "completed" | "failed"
	>(initialAccessTask ? "waiting" : "idle");
	const [accessSyncProgress, setAccessSyncProgress] =
		useState<DashboardSyncProgress | null>(
			initialAccessTask ? accessSyncProgressFromStage("waiting") : null,
		);
	const refreshTaskSourcesRef = useRef<Map<string, EventSource>>(new Map());
	const syncAllInFlightRef = useRef(false);
	const taskWaitersRef = useRef<
		Map<
			string,
			{
				promise: Promise<void>;
				settle: (error?: Error) => void;
			}
		>
	>(new Map());

	const [uncontrolledRouteState, setUncontrolledRouteState] =
		useState<DashboardRouteState>(() => initialRouteState);
	const routeState = controlledRouteState ?? uncontrolledRouteState;
	const tab = routeState.tab;
	const scope = routeState.scope;
	const scopedMode = isScopedDashboardRouteState(routeState);
	const currentScopeSignature = buildDashboardScopeSignature(scope);
	const briefsQueryKey = useMemo(
		() => dashboardBriefsQueryKey(me.user.id),
		[me.user.id],
	);
	const notificationsQueryKey = useMemo(
		() => dashboardNotificationsQueryKey(me.user.id),
		[me.user.id],
	);
	const reactionTokenQueryKey = useMemo(
		() => dashboardReactionTokenQueryKey(me.user.id),
		[me.user.id],
	);
	const activeReleaseId = routeState.activeReleaseId;
	const activeReleaseLocator = routeState.activeReleaseLocator;
	const activeAnnouncementLocator = routeState.activeAnnouncementLocator;
	const releaseReturnTab = routeState.releaseReturnTab;
	const routeSelectedBriefId = routeState.selectedBriefId;
	const activeReleaseTarget = useMemo(
		() =>
			activeReleaseId || activeReleaseLocator
				? buildDashboardReleaseTarget({
						releaseId: activeReleaseId,
						locator: activeReleaseLocator,
						fromTab: releaseReturnTab,
						scope,
						selectedBriefId: routeSelectedBriefId,
					})
				: null,
		[
			activeReleaseId,
			activeReleaseLocator,
			releaseReturnTab,
			routeSelectedBriefId,
			scope,
		],
	);
	const setRouteState = useCallback(
		(
			nextRouteState: DashboardRouteState,
			options?: {
				replace?: boolean;
			},
		) => {
			if (onRouteStateChange) {
				onRouteStateChange(nextRouteState, options);
				return;
			}
			setUncontrolledRouteState(nextRouteState);
		},
		[onRouteStateChange],
	);

	const feedRequestType: FeedRequestType =
		tab === "releases"
			? "releases"
			: tab === "stars"
				? "stars"
				: tab === "followers"
					? "followers"
					: "all";

	const warmFeedData =
		warmStart && warmStart.feedRequestType === feedRequestType
			? {
					type: warmStart.feedRequestType,
					items: warmStart.feedItems,
					nextCursor: warmStart.nextCursor,
					updatedAt: 0,
				}
			: null;
	const viewerStateKey = buildDashboardWarmViewerStateKey(me);
	const readableMode = !scopedMode && tab === "all";
	const readableSections = useDashboardReadableSections({
		userId: me.user.id,
		viewerStateKey,
		enabled: readableMode,
	});
	const readableSectionsActive =
		readableMode && !readableSections.legacyFallback;
	const feed = useFeed(feedRequestType, {
		userId: me.user.id,
		viewerStateKey,
		initialData: readableSectionsActive ? null : warmFeedData,
		scope,
		enabled: !readableSectionsActive,
	});
	const applyTranslationToActive = useCallback(
		(item: Pick<FeedItem, "kind" | "id">, translated: TranslatedItem) => {
			if (readableSectionsActive) {
				readableSections.applyTranslation(item, translated);
			} else {
				feed.applyTranslation(item, translated);
			}
		},
		[
			feed.applyTranslation,
			readableSectionsActive,
			readableSections.applyTranslation,
		],
	);
	const applySmartToActive = useCallback(
		(item: Pick<FeedItem, "kind" | "id">, smart: SmartItem) => {
			if (readableSectionsActive) readableSections.applySmart(item, smart);
			else feed.applySmart(item, smart);
		},
		[feed.applySmart, readableSectionsActive, readableSections.applySmart],
	);
	const applyReactionsToActive = useCallback(
		(item: Pick<FeedItem, "kind" | "id">, reactions: ReleaseReactions) => {
			if (readableSectionsActive)
				readableSections.applyReactions(item, reactions);
			else feed.applyReactions(item, reactions);
		},
		[
			feed.applyReactions,
			readableSectionsActive,
			readableSections.applyReactions,
		],
	);
	const activeFeedItems = useMemo(() => {
		if (!readableSectionsActive) return feed.items;
		return readableSections.sections.flatMap((section) => [
			...(section.items ?? []),
			...(section.supplemental_items ?? []),
			...(readableSections.details[section.id]?.items ?? []),
		]);
	}, [
		feed.items,
		readableSectionsActive,
		readableSections.details,
		readableSections.sections,
	]);
	const feedItemsRef = useRef(activeFeedItems);
	feedItemsRef.current = activeFeedItems;
	const followingReposQuery = useQuery<FollowingReposResponse>({
		queryKey: ["dashboard", "following-repos", me.user.id],
		queryFn: apiGetFollowingRepos,
		enabled: scope?.kind === "following" || scope?.kind === "repo",
	});
	const refreshFeed = readableSectionsActive
		? readableSections.loadInitial
		: feed.refresh;
	const followingRepos = followingReposQuery.data ?? null;
	const followingReposLoading = followingReposQuery.isLoading;
	const followingReposError = followingReposQuery.error
		? describeUnknownError(followingReposQuery.error, "关注仓库清单加载失败。")
		: null;
	const [personalRepos, setPersonalRepos] =
		useState<PersonalReposResponse | null>(null);
	const [personalReposLoading, setPersonalReposLoading] = useState(false);
	const [personalReposError, setPersonalReposError] = useState<string | null>(
		null,
	);
	const [liveNotices, setLiveNotices] = useState<DashboardLiveNoticeState>({});
	const activeFeedNotices = liveNotices.feed?.[feedRequestType] ?? [];
	const activeFeedNotice = activeFeedNotices[0];
	const hydratedFeedNoticeRef = useRef<Map<string, string>>(new Map());
	const [freshBriefKeys, setFreshBriefKeys] = useState<Set<string>>(
		() => new Set(),
	);
	const [freshNotificationKeys, setFreshNotificationKeys] = useState<
		Set<string>
	>(() => new Set());

	const [selectedLaneByKey, setSelectedLaneByKey] = useState<
		Record<string, FeedLane>
	>({});
	const cachedReactionTokenStatus =
		queryClient.getQueryData<DashboardReactionTokenQueryData>(
			reactionTokenQueryKey,
		);
	const briefsCacheQuery = useQuery<DashboardBriefsQueryData>({
		queryKey: briefsQueryKey,
		queryFn: () =>
			queryClient.getQueryData<DashboardBriefsQueryData>(briefsQueryKey) ?? {
				items: [],
				selectedBriefId: null,
			},
		enabled: false,
	});
	const notificationsCacheQuery = useQuery<DashboardNotificationsQueryData>({
		queryKey: notificationsQueryKey,
		queryFn: () =>
			queryClient.getQueryData<DashboardNotificationsQueryData>(
				notificationsQueryKey,
			) ?? { items: [] },
		enabled: false,
	});
	const reactionTokenCacheQuery = useQuery<DashboardReactionTokenQueryData>({
		queryKey: reactionTokenQueryKey,
		queryFn: () =>
			queryClient.getQueryData<DashboardReactionTokenQueryData>(
				reactionTokenQueryKey,
			) ?? {
				configured: false,
				masked_token: null,
				owner: null,
				check: {
					state: "idle",
					message: "未配置 GitHub PAT。",
					checked_at: null,
				},
			},
		enabled: false,
	});
	const [pageDefaultLane, setPageDefaultLane] = useState<FeedLane>(
		readStoredPageDefaultLane,
	);
	const effectivePageDefaultLane = useMemo(
		() => resolveDisplayLaneForFeed(feed.items, pageDefaultLane),
		[feed.items, pageDefaultLane],
	);
	const [fallbackSelectedBriefId, setFallbackSelectedBriefId] = useState<
		string | null
	>(
		() =>
			queryClient.getQueryData<DashboardBriefsQueryData>(briefsQueryKey)
				?.selectedBriefId ??
			warmStart?.selectedBriefId ??
			sessionState?.selectedBriefId ??
			null,
	);
	const [reactionBusyKeys, setReactionBusyKeys] = useState<Set<string>>(
		() => new Set<string>(),
	);
	const reactionBusyKeysRef = useRef<Set<string>>(new Set<string>());
	const reactionDesiredByKeyRef = useRef<Map<string, ReleaseReactions>>(
		new Map<string, ReleaseReactions>(),
	);
	const reactionServerByKeyRef = useRef<Map<string, ReleaseReactions>>(
		new Map<string, ReleaseReactions>(),
	);
	const reactionFlushTimerByKeyRef = useRef<Map<string, number>>(
		new Map<string, number>(),
	);
	const lastFeedReactionRefreshByKeyRef = useRef<Map<string, number>>(
		new Map<string, number>(),
	);
	const [reactionErrorByKey, setReactionErrorByKey] = useState<
		Record<string, string>
	>({});
	const reactionRefreshingKeysRef = useRef<Set<string>>(new Set<string>());
	const [reactionTokenConfigured, setReactionTokenConfigured] = useState<
		boolean | null
	>(
		() =>
			(cachedReactionTokenStatus
				? isReactionTokenUsable(cachedReactionTokenStatus)
				: sessionState?.reactionTokenConfigured) ?? null,
	);
	const [patGuideOpen, setPatGuideOpen] = useState<boolean>(false);
	const [patGuideMessage, setPatGuideMessage] = useState<string | null>(null);
	const pendingReactionRef = useRef<{
		releaseId: string;
		content: ReactionContent;
	} | null>(null);
	const handleReactionTokenStatusLoaded = useCallback(
		(status: Parameters<typeof isReactionTokenUsable>[0]) => {
			queryClient.setQueryData<DashboardReactionTokenQueryData>(
				reactionTokenQueryKey,
				status,
			);
			setReactionTokenConfigured(isReactionTokenUsable(status));
		},
		[queryClient, reactionTokenQueryKey],
	);
	const handleReactionTokenSaved = useCallback(
		(status: Parameters<typeof isReactionTokenUsable>[0]) => {
			queryClient.setQueryData<DashboardReactionTokenQueryData>(
				reactionTokenQueryKey,
				status,
			);
			setReactionTokenConfigured(isReactionTokenUsable(status));
		},
		[queryClient, reactionTokenQueryKey],
	);

	useEffect(() => {
		if (scope?.kind !== "mine") {
			setPersonalRepos(null);
			setPersonalReposLoading(false);
			setPersonalReposError(null);
			return;
		}

		let cancelled = false;
		setPersonalReposLoading(true);
		setPersonalReposError(null);
		void apiGet<PersonalReposResponse>("/api/me/personal-repos")
			.then((response) => {
				if (cancelled) return;
				setPersonalRepos(response);
			})
			.catch((error) => {
				if (cancelled) return;
				setPersonalReposError(
					describeUnknownError(error, "个人仓库清单加载失败。"),
				);
			})
			.finally(() => {
				if (cancelled) return;
				setPersonalReposLoading(false);
			});

		return () => {
			cancelled = true;
		};
	}, [me.user.id, scope?.kind]);

	const {
		reactionTokenMasked,
		patInput,
		setPatInput,
		patCheckState,
		patCheckMessage,
		patCheckedAt,
		patSaving,
		canSavePat,
		loadReactionToken,
		savePat,
		clearPatDraft,
	} = useReactionTokenEditor({
		autoLoad: false,
		initialStatus: cachedReactionTokenStatus,
		onStatusLoaded: handleReactionTokenStatusLoaded,
		onPatSaved: handleReactionTokenSaved,
	});

	const [notifications, setNotifications] = useState<NotificationItem[]>(
		() =>
			queryClient.getQueryData<DashboardNotificationsQueryData>(
				notificationsQueryKey,
			)?.items ??
			sessionState?.notifications ??
			warmStart?.notifications ??
			[],
	);
	const [briefs, setBriefs] = useState<BriefItem[]>(
		() =>
			queryClient.getQueryData<DashboardBriefsQueryData>(briefsQueryKey)
				?.items ??
			sessionState?.briefs ??
			warmStart?.briefs ??
			[],
	);
	const briefDetailRequestInFlightRef = useRef<
		Map<string, Promise<BriefItem | null>>
	>(new Map());
	const [briefDetailLoadingIds, setBriefDetailLoadingIds] = useState<
		Set<string>
	>(() => new Set());
	const [briefDetailErrors, setBriefDetailErrors] = useState<
		Record<string, string | undefined>
	>({});
	const [copyingBriefId, setCopyingBriefId] = useState<string | null>(null);
	const allowReleaseItemLaneOverride = useMediaQuery("(min-width: 640px)");
	const [briefsError, setBriefsError] = useState<DashboardSectionError | null>(
		null,
	);
	const [notificationsError, setNotificationsError] =
		useState<DashboardSectionError | null>(null);
	const hasTabletSidebar = useMediaQuery("(min-width: 1024px)");
	const hasDesktopSidebarInbox = useMediaQuery("(min-width: 1024px)");
	const initialNotificationBootstrapRef = useRef(
		hasDesktopSidebarInbox || tab === "inbox",
	);
	const hasCachedBriefs =
		queryClient.getQueryData<DashboardBriefsQueryData>(briefsQueryKey) !==
		undefined;
	const hasCachedNotifications =
		queryClient.getQueryData<DashboardNotificationsQueryData>(
			notificationsQueryKey,
		) !== undefined;
	const sidebarBootstrapCompletedRef = useRef(
		sessionState?.sidebarBootstrapped ?? hasCachedBriefs,
	);
	const notificationsBootstrapCompletedRef = useRef(
		sessionState?.notificationsBootstrapped ??
			(hasCachedNotifications ||
				Boolean(warmStart && warmStart.notifications.length > 0)),
	);
	const reactionTokenBootstrapCompletedRef = useRef(
		sessionState?.reactionTokenBootstrapped ??
			cachedReactionTokenStatus !== undefined,
	);
	const startupBootstrapRequestedRef = useRef(
		sessionState?.sidebarBootstrapped === true,
	);
	const startupSidebarRetriedRef = useRef(false);
	const notificationsBootstrapRequestedRef = useRef(
		sessionState?.notificationsBootstrapped === true ||
			(initialNotificationBootstrapRef.current &&
				!startupBootstrapRequestedRef.current),
	);
	const reactionTokenBootstrapRequestedRef = useRef(
		sessionState?.reactionTokenBootstrapped === true,
	);
	const notificationsRequestInFlightRef = useRef(false);
	const [sidebarLoading, setSidebarLoading] = useState(
		() => !bootedFromWarmStart && !hasCachedBriefs,
	);
	const [notificationsLoading, setNotificationsLoading] = useState(false);
	useEffect(() => {
		if (!notificationsCacheQuery.data) return;
		setNotifications(notificationsCacheQuery.data.items);
		notificationsBootstrapCompletedRef.current = true;
	}, [notificationsCacheQuery.data]);
	useEffect(() => {
		if (!briefsCacheQuery.data) return;
		setBriefs(briefsCacheQuery.data.items);
		setFallbackSelectedBriefId((current) => {
			if (
				routeSelectedBriefId &&
				briefsCacheQuery.data.items.some(
					(brief) => brief.id === routeSelectedBriefId,
				)
			) {
				return routeSelectedBriefId;
			}
			const cachedSelectedId = briefsCacheQuery.data.selectedBriefId;
			if (
				cachedSelectedId &&
				briefsCacheQuery.data.items.some(
					(brief) => brief.id === cachedSelectedId,
				)
			) {
				return cachedSelectedId;
			}
			if (
				current &&
				briefsCacheQuery.data.items.some((brief) => brief.id === current)
			) {
				return current;
			}
			return briefsCacheQuery.data.items[0]?.id ?? null;
		});
		sidebarBootstrapCompletedRef.current = true;
	}, [briefsCacheQuery.data, routeSelectedBriefId]);
	useEffect(() => {
		if (!routeSelectedBriefId) return;
		setFallbackSelectedBriefId(routeSelectedBriefId);
	}, [routeSelectedBriefId]);
	useEffect(() => {
		if (!reactionTokenCacheQuery.data) return;
		setReactionTokenConfigured(
			isReactionTokenUsable(reactionTokenCacheQuery.data),
		);
		reactionTokenBootstrapCompletedRef.current = true;
	}, [reactionTokenCacheQuery.data]);
	useEffect(() => {
		if (
			!notificationsBootstrapCompletedRef.current &&
			notifications.length === 0
		) {
			return;
		}
		queryClient.setQueryData<DashboardNotificationsQueryData>(
			notificationsQueryKey,
			{ items: notifications },
		);
	}, [notifications, notificationsQueryKey, queryClient]);
	useEffect(() => {
		if (!sidebarBootstrapCompletedRef.current && briefs.length === 0) {
			return;
		}
		queryClient.setQueryData<DashboardBriefsQueryData>(briefsQueryKey, {
			items: briefs,
			selectedBriefId: routeSelectedBriefId ?? fallbackSelectedBriefId,
		});
	}, [
		briefs,
		briefsQueryKey,
		fallbackSelectedBriefId,
		queryClient,
		routeSelectedBriefId,
	]);
	const focusFeedItem = useCallback((key: string) => {
		window.requestAnimationFrame(() => {
			const element = Array.from(
				document.querySelectorAll<HTMLElement>("[data-feed-item-key]"),
			).find((item) => item.dataset.feedItemKey === key);
			if (!element) return;
			element.scrollIntoView({ block: "center", behavior: "smooth" });
			element.focus({ preventScroll: true });
		});
	}, []);
	const notifyGlobalError = useCallback(
		(
			title: string,
			error: unknown,
			fallback: string,
			options?: {
				actionLabel?: string;
				onAction?: () => void;
				secondaryActionLabel?: string;
				onSecondaryAction?: () => void;
				detail?: string | null;
				dedupeKey?: string;
			},
		) => {
			return pushErrorToast(title, describeUnknownError(error, fallback), {
				dedupeKey: options?.dedupeKey,
				actionLabel: options?.actionLabel,
				onAction: options?.onAction,
				secondaryActionLabel: options?.secondaryActionLabel,
				onSecondaryAction: options?.onSecondaryAction,
				detail:
					options?.detail ?? (error instanceof Error ? error.message : null),
			});
		},
		[pushErrorToast],
	);
	const notifyFeedLaneError = useCallback(
		(
			item: FeedItem,
			lane: Extract<FeedLane, "translated" | "smart">,
			error: unknown,
			retry: (item: FeedItem) => Promise<unknown>,
		) => {
			const isSmart = lane === "smart";
			const key = feedItemKey(item);
			let toastId = "";
			toastId = notifyGlobalError(
				isSmart ? "润色触发失败" : "翻译触发失败",
				error,
				isSmart ? "润色触发失败，请稍后重试。" : "翻译触发失败，请稍后重试。",
				{
					dedupeKey: `dashboard-feed:${lane}:${key}`,
					actionLabel: isSmart ? "重试润色" : "重试翻译",
					onAction: async () => {
						const currentItem = feedItemsRef.current.find(
							(feedItem) => feedItemKey(feedItem) === key,
						);
						if (!currentItem) {
							dismissToast(toastId);
							return;
						}
						try {
							await retry(currentItem);
							dismissToast(toastId);
						} catch (nextError) {
							notifyFeedLaneError(currentItem, lane, nextError, retry);
						}
					},
					secondaryActionLabel: "定位到卡片",
					onSecondaryAction: () => focusFeedItem(key),
				},
			);
		},
		[dismissToast, feedItemsRef, focusFeedItem, notifyGlobalError],
	);

	const loadNotifications = useCallback(
		async (phase: DashboardSectionError["phase"] = "initial") => {
			if (notificationsRequestInFlightRef.current) {
				return;
			}
			notificationsRequestInFlightRef.current = true;
			setNotificationsError(null);
			try {
				const items = await apiGet<NotificationItem[]>("/api/notifications");
				setNotifications(sortNotifications(items));
			} catch (error) {
				const message = describeUnknownError(
					error,
					"Inbox 加载失败，请稍后重试。",
				);
				setNotificationsError({ phase, message, at: Date.now() });
				if (phase === "refresh" || notifications.length > 0) {
					notifyGlobalError("Inbox 刷新失败", error, message);
				}
				throw error;
			} finally {
				notificationsRequestInFlightRef.current = false;
			}
		},
		[notifications.length, notifyGlobalError],
	);
	const refreshSidebar = useCallback(
		async (options?: {
			background?: boolean;
			includeNotifications?: boolean;
			preferredBriefId?: string | null;
		}) => {
			if (!options?.background) {
				setSidebarLoading(true);
			}
			setBriefsError(null);
			try {
				const phase: DashboardSectionError["phase"] = options?.background
					? "refresh"
					: "initial";
				const [briefsResult, notificationsResult] = await Promise.allSettled([
					apiGet<BriefItem[]>("/api/briefs"),
					options?.includeNotifications
						? loadNotifications(phase)
						: Promise.resolve(),
				]);
				if (
					options?.includeNotifications &&
					notificationsResult.status === "rejected"
				) {
					// `loadNotifications` has already updated inline/global feedback.
					if (phase === "initial") {
						throw {
							kind: "sidebar-bootstrap-notifications",
							cause: notificationsResult.reason,
						} satisfies SidebarBootstrapNotificationsError;
					}
				}
				if (briefsResult.status === "rejected") {
					throw briefsResult.reason;
				}
				const b = briefsResult.value;
				setBriefs((current) =>
					mergeBriefSummariesWithCachedDetails(b, current),
				);
				setFallbackSelectedBriefId((prev) => {
					if (
						routeSelectedBriefId &&
						b.some((brief) => brief.id === routeSelectedBriefId)
					) {
						return routeSelectedBriefId;
					}
					if (
						options?.preferredBriefId &&
						b.some((x) => x.id === options.preferredBriefId)
					) {
						return options.preferredBriefId;
					}
					if (prev && b.some((x) => x.id === prev)) return prev;
					return b[0]?.id ?? null;
				});
				sidebarBootstrapCompletedRef.current = true;
				if (options?.includeNotifications) {
					notificationsBootstrapCompletedRef.current = true;
				}
			} catch (error) {
				if (isSidebarBootstrapNotificationsError(error)) {
					throw error.cause;
				}
				const message = describeUnknownError(
					error,
					"日报加载失败，请稍后重试。",
				);
				const phase: DashboardSectionError["phase"] = options?.background
					? "refresh"
					: "initial";
				setBriefsError({ phase, message, at: Date.now() });
				if (phase === "refresh" || briefs.length > 0) {
					notifyGlobalError("侧栏刷新失败", error, message);
				}
				throw error;
			} finally {
				setSidebarLoading(false);
			}
		},
		[briefs.length, loadNotifications, notifyGlobalError, routeSelectedBriefId],
	);
	const refreshNotifications = useCallback(
		async (options?: { background?: boolean }) => {
			if (!options?.background) {
				setNotificationsLoading(true);
			}
			try {
				await loadNotifications(options?.background ? "refresh" : "initial");
				notificationsBootstrapCompletedRef.current = true;
			} finally {
				setNotificationsLoading(false);
			}
		},
		[loadNotifications],
	);
	const selectedBriefId = routeSelectedBriefId ?? fallbackSelectedBriefId;
	const selectedBrief = useMemo(
		() => resolveSelectedBrief(briefs, selectedBriefId),
		[briefs, selectedBriefId],
	);
	const effectiveSelectedBriefId = selectedBrief?.id ?? null;
	const selectedBriefDetailLoading = selectedBrief
		? briefDetailLoadingIds.has(selectedBrief.id)
		: false;
	const selectedBriefDetailError = selectedBrief
		? (briefDetailErrors[selectedBrief.id] ?? null)
		: null;
	const loadBriefDetail = useCallback(
		async (briefId: string): Promise<BriefItem | null> => {
			const existingRequest =
				briefDetailRequestInFlightRef.current.get(briefId);
			if (existingRequest) {
				return existingRequest;
			}
			const request = (async () => {
				setBriefDetailLoadingIds((current) => {
					const next = new Set(current);
					next.add(briefId);
					return next;
				});
				setBriefDetailErrors((current) => ({
					...current,
					[briefId]: undefined,
				}));
				try {
					const detail = await apiGet<BriefItem>(
						`/api/briefs/${encodeURIComponent(briefId)}`,
					);
					setBriefs((current) =>
						current.map((brief) =>
							brief.id === detail.id ? { ...brief, ...detail } : brief,
						),
					);
					return detail;
				} catch (error) {
					const message = describeUnknownError(
						error,
						"日报正文加载失败，请稍后重试。",
					);
					setBriefDetailErrors((current) => ({
						...current,
						[briefId]: message,
					}));
					notifyGlobalError("日报正文加载失败", error, message);
					return null;
				} finally {
					briefDetailRequestInFlightRef.current.delete(briefId);
					setBriefDetailLoadingIds((current) => {
						const next = new Set(current);
						next.delete(briefId);
						return next;
					});
				}
			})();
			briefDetailRequestInFlightRef.current.set(briefId, request);
			return request;
		},
		[notifyGlobalError],
	);
	const selectedBriefContent = selectedBrief?.content_markdown ?? null;

	useEffect(() => {
		if (!selectedBrief || selectedBriefContent) {
			return;
		}
		void loadBriefDetail(selectedBrief.id);
	}, [loadBriefDetail, selectedBrief?.id, selectedBrief, selectedBriefContent]);
	const ensureBriefDetail = useCallback(
		async (briefId: string) => {
			const existing = briefs.find((brief) => brief.id === briefId) ?? null;
			if (existing?.content_markdown) {
				return existing;
			}
			return loadBriefDetail(briefId);
		},
		[briefs, loadBriefDetail],
	);
	const openBrief = useCallback(
		(briefId: string, options?: { replace?: boolean }) => {
			setFallbackSelectedBriefId(briefId);
			setRouteState(
				{
					tab: "briefs",
					scope: null,
					selectedBriefId: briefId,
					activeReleaseId: null,
					activeReleaseLocator: null,
					activeAnnouncementLocator: null,
					releaseReturnTab: "briefs",
				},
				{ replace: options?.replace },
			);
		},
		[setRouteState],
	);
	const copyBrief = useCallback(
		async (briefId: string) => {
			setCopyingBriefId(briefId);
			try {
				const detail = await ensureBriefDetail(briefId);
				const brief =
					detail ?? briefs.find((item) => item.id === briefId) ?? null;
				const markdown = brief?.content_markdown?.trim() ?? "";
				if (!markdown) {
					throw new Error("日报正文还没有准备好。");
				}
				await waitForNextPaint();
				await waitForNextPaint();
				const briefRoot = document.querySelector<HTMLElement>(
					`[data-brief-content-id="${briefId}"] [data-markdown-root="true"]`,
				);
				if (!briefRoot) {
					throw new Error("日报正文还没有完成渲染。");
				}
				await writeRichClipboard(
					buildRichClipboardPayload(briefRoot, { markdown }),
				);
				pushToast({
					title: "日报已复制",
					description: "已写入富文本与纯文本；支持时会附带 Markdown。",
				});
			} catch (error) {
				pushErrorToast(
					"复制日报失败",
					describeUnknownError(error, "请稍后重试。"),
				);
			} finally {
				setCopyingBriefId((current) => (current === briefId ? null : current));
			}
		},
		[briefs, ensureBriefDetail, pushErrorToast, pushToast],
	);

	useEffect(() => {
		if (
			scope ||
			tab !== "briefs" ||
			!effectiveSelectedBriefId ||
			activeReleaseId ||
			activeReleaseLocator ||
			activeAnnouncementLocator
		) {
			return;
		}
		if (routeSelectedBriefId === effectiveSelectedBriefId) {
			return;
		}
		setRouteState(
			{
				...routeState,
				selectedBriefId: effectiveSelectedBriefId,
			},
			{ replace: true },
		);
	}, [
		activeAnnouncementLocator,
		activeReleaseId,
		activeReleaseLocator,
		effectiveSelectedBriefId,
		routeSelectedBriefId,
		routeState,
		scope,
		setRouteState,
		tab,
	]);

	const refreshAll = useCallback(async () => {
		const tasks: Array<Promise<unknown>> = [refreshFeed()];
		if (!scopedMode) {
			tasks.push(
				refreshSidebar({
					includeNotifications:
						hasDesktopSidebarInbox ||
						tab === "inbox" ||
						notificationsBootstrapCompletedRef.current,
				}),
			);
		}
		await Promise.all(tasks);
	}, [hasDesktopSidebarInbox, refreshFeed, refreshSidebar, scopedMode, tab]);

	const onDashboardLiveUpdate = useCallback(
		(notices: DashboardLiveUpdateNotice[]) => {
			setLiveNotices((current) => {
				const next = { ...current };
				for (const notice of notices) {
					if (notice.list === "feed") {
						const noticeFeedType = notice.feedType ?? feedRequestType;
						const boundaryNotice = makeFeedBoundaryNotice(
							notice,
							noticeFeedType,
						);
						const currentFeedNotices = next.feed?.[noticeFeedType] ?? [];
						next.feed = {
							...next.feed,
							[noticeFeedType]: mergeFeedBoundaryNotices(
								currentFeedNotices,
								boundaryNotice,
							),
						};
						continue;
					}
					next[notice.list] = mergeDashboardLiveNotice(
						next[notice.list],
						notice,
					);
				}
				return next;
			});
		},
		[feedRequestType],
	);
	const { checkNow: checkDashboardUpdates } = useDashboardLiveUpdates({
		enabled:
			shellHydrated &&
			(readableSectionsActive
				? !readableSections.loadingInitial
				: !feed.loadingInitial),
		feedType: feedRequestType,
		includeBriefs: !scopedMode,
		includeNotifications:
			!scopedMode && notificationsBootstrapCompletedRef.current,
		scope,
		onUpdate: onDashboardLiveUpdate,
	});

	const scrollToFreshFeedTop = useCallback((keys: string[]) => {
		window.requestAnimationFrame(() => {
			const itemElements = Array.from(
				document.querySelectorAll<HTMLElement>("[data-feed-item-key]"),
			);
			for (const key of keys) {
				const element = itemElements.find(
					(item) => item.dataset.feedItemKey === key,
				);
				if (!element) continue;
				element.scrollIntoView({ block: "start", behavior: "smooth" });
				return;
			}
		});
	}, []);

	const dismissFeedBoundary = useCallback(
		(boundaryId: string) => {
			hydratedFeedNoticeRef.current.delete(boundaryId);
			setLiveNotices((current) => {
				const feedByType = current.feed ?? {};
				const currentFeedNotices = feedByType[feedRequestType] ?? [];
				const nextFeedNotices = currentFeedNotices.filter(
					(notice) => notice.boundaryId !== boundaryId,
				);
				return {
					...current,
					feed: {
						...feedByType,
						[feedRequestType]: nextFeedNotices,
					},
				};
			});
		},
		[feedRequestType],
	);

	const sealFeedBoundary = useCallback(
		(boundaryId: string) => {
			setLiveNotices((current) => {
				const feedByType = current.feed ?? {};
				const currentFeedNotices = feedByType[feedRequestType] ?? [];
				let changed = false;
				const nextFeedNotices = currentFeedNotices.map((notice) => {
					if (notice.boundaryId !== boundaryId || notice.sealed) {
						return notice;
					}
					changed = true;
					return { ...notice, sealed: true };
				});
				if (!changed) return current;
				return {
					...current,
					feed: {
						...feedByType,
						[feedRequestType]: nextFeedNotices,
					},
				};
			});
		},
		[feedRequestType],
	);

	const resolveFeedBoundary = useCallback(
		(boundaryId: string, boundaryAfterKey: string | null) => {
			setLiveNotices((current) => {
				const feedByType = current.feed ?? {};
				const currentFeedNotices = feedByType[feedRequestType] ?? [];
				const target = currentFeedNotices.find(
					(notice) => notice.boundaryId === boundaryId,
				);
				if (!target?.hydrated || target.boundaryAfterKey !== undefined) {
					return current;
				}
				if (boundaryAfterKey === null) {
					hydratedFeedNoticeRef.current.delete(boundaryId);
					return {
						...current,
						feed: {
							...feedByType,
							[feedRequestType]: currentFeedNotices.filter(
								(notice) => notice.boundaryId !== boundaryId,
							),
						},
					};
				}
				return {
					...current,
					feed: {
						...feedByType,
						[feedRequestType]: currentFeedNotices.map((notice) =>
							notice.boundaryId === boundaryId
								? { ...notice, boundaryAfterKey }
								: notice,
						),
					},
				};
			});
		},
		[feedRequestType],
	);

	const revealFeedUpdates = useCallback(
		async (notice = activeFeedNotice) => {
			if (!notice) return;
			const freshKeys = notice.latestKeys.slice(0, notice.newCount);
			const hasHydratedFreshItems = feed.items.some((item) =>
				freshKeys.includes(feedItemKey(item)),
			);
			if (!hasHydratedFreshItems) {
				const nextFreshKeys = Array.from(
					new Set([...feed.freshKeys, ...freshKeys]),
				);
				await refreshFeed({
					freshKeys: nextFreshKeys,
					throwOnError: true,
				});
			}
			scrollToFreshFeedTop(freshKeys);
			dismissFeedBoundary(notice.boundaryId);
			await checkDashboardUpdates({ emit: false, include: ["feed"] });
		},
		[
			activeFeedNotice,
			checkDashboardUpdates,
			dismissFeedBoundary,
			feed.freshKeys,
			feed.items,
			refreshFeed,
			scrollToFreshFeedTop,
		],
	);

	useEffect(() => {
		const notice = activeFeedNotice;
		if (!notice || feed.loadingInitial) return;
		const freshKeys = notice.latestKeys.slice(0, notice.newCount);
		if (freshKeys.length === 0) return;
		const hydratedKey = freshKeys.join("|");
		if (hydratedFeedNoticeRef.current.get(notice.boundaryId) === hydratedKey) {
			return;
		}
		hydratedFeedNoticeRef.current.set(notice.boundaryId, hydratedKey);
		const anchor = captureFeedScrollAnchor();
		const nextFreshKeys = Array.from(
			new Set([...feed.freshKeys, ...freshKeys]),
		);
		void refreshFeed({
			freshKeys: nextFreshKeys,
			throwOnError: true,
		})
			.then(() => {
				restoreFeedScrollAnchor(anchor);
				if (notice.boundaryKeys.length === 0) {
					dismissFeedBoundary(notice.boundaryId);
				} else {
					setLiveNotices((current) => {
						const feedByType = current.feed ?? {};
						const currentFeedNotices = feedByType[feedRequestType] ?? [];
						return {
							...current,
							feed: {
								...feedByType,
								[feedRequestType]: currentFeedNotices.map((item) =>
									item.boundaryId === notice.boundaryId
										? { ...item, hydrated: true }
										: item,
								),
							},
						};
					});
				}
				void checkDashboardUpdates({ emit: false, include: ["feed"] });
			})
			.catch((error) => {
				if (
					hydratedFeedNoticeRef.current.get(notice.boundaryId) === hydratedKey
				) {
					hydratedFeedNoticeRef.current.delete(notice.boundaryId);
				}
				notifyGlobalError(
					"新动态显示失败",
					error,
					"新动态显示失败，请稍后重试。",
				);
			});
	}, [
		activeFeedNotice,
		checkDashboardUpdates,
		dismissFeedBoundary,
		feed.freshKeys,
		feed.loadingInitial,
		feedRequestType,
		notifyGlobalError,
		refreshFeed,
	]);

	const revealBriefUpdates = useCallback(async () => {
		const notice = liveNotices.briefs;
		if (!notice) return;
		const freshKeys = notice.latestKeys.slice(0, notice.newCount);
		const preferredBriefId =
			freshKeys
				.find((key) => key.startsWith("brief:"))
				?.replace(/^brief:/, "") ?? null;
		await refreshSidebar({
			background: true,
			includeNotifications: false,
			preferredBriefId,
		});
		setFreshBriefKeys(new Set(freshKeys));
		setLiveNotices((current) => ({ ...current, briefs: undefined }));
		await checkDashboardUpdates({ emit: false, include: ["briefs"] });
	}, [checkDashboardUpdates, liveNotices.briefs, refreshSidebar]);

	const revealNotificationUpdates = useCallback(async () => {
		const notice = liveNotices.notifications;
		if (!notice) return;
		await refreshNotifications({ background: true });
		setFreshNotificationKeys(
			new Set(notice.latestKeys.slice(0, notice.newCount)),
		);
		setLiveNotices((current) => ({ ...current, notifications: undefined }));
		await checkDashboardUpdates({ emit: false, include: ["notifications"] });
	}, [checkDashboardUpdates, liveNotices.notifications, refreshNotifications]);
	const clearDashboardLiveNotices = useCallback(() => {
		setLiveNotices({});
		setFreshBriefKeys(new Set());
		setFreshNotificationKeys(new Set());
	}, []);

	const ensureTaskWaiter = useCallback((taskId: string) => {
		const existing = taskWaitersRef.current.get(taskId);
		if (existing) {
			return existing.promise;
		}

		let settled = false;
		let settle = (_error?: Error) => undefined;
		const promise = new Promise<void>((resolve, reject) => {
			settle = (error?: Error) => {
				if (settled) return;
				settled = true;
				taskWaitersRef.current.delete(taskId);
				if (error) {
					reject(error);
					return;
				}
				resolve();
			};
		});

		taskWaitersRef.current.set(taskId, { promise, settle });
		return promise;
	}, []);

	const settleTaskWaiter = useCallback((taskId: string, error?: Error) => {
		taskWaitersRef.current.get(taskId)?.settle(error);
	}, []);

	const trackTaskStream = useCallback(
		(task: TaskAcceptedResponse, mode: TaskStreamMode) => {
			const next = {
				taskId: task.task_id,
				eventPath: `/api/tasks/${task.task_id}/events`,
			};
			const promise = ensureTaskWaiter(task.task_id);
			if (mode === "access") {
				setAccessTaskStream((current) =>
					current?.taskId === next.taskId ? current : next,
				);
				setAccessSyncStage("waiting");
				setAccessSyncProgress(accessSyncProgressFromStage("waiting"));
				return promise;
			}
			setRefreshTaskStreams((current) =>
				current.some((item) => item.taskId === next.taskId)
					? current
					: [...current, next],
			);
			return promise;
		},
		[ensureTaskWaiter],
	);

	const run = useCallback(
		async <T,>(
			label: string,
			fn: () => Promise<T>,
			options?: {
				errorTitle?: string;
				fallback?: string;
				actionLabel?: string;
				onAction?: () => void;
			},
		) => {
			setBusy(label);
			try {
				return await fn();
			} catch (error) {
				notifyGlobalError(
					options?.errorTitle ?? `${label}失败`,
					error,
					options?.fallback ?? `${label}失败，请稍后重试。`,
					{
						actionLabel: options?.actionLabel,
						onAction: options?.onAction,
					},
				);
				return null;
			} finally {
				setBusy(null);
			}
		},
		[notifyGlobalError],
	);

	const {
		register: registerTranslate,
		retryLoadedErrors: retryLoadedTranslations,
		translateNow,
		inFlightKeys: translationInFlightKeys,
		autoRetryingKeys: translationAutoRetryingKeys,
	} = useAutoTranslate({
		enabled: true,
		onTranslated: applyTranslationToActive,
	});
	const {
		prime: primeSmart,
		register: registerSmart,
		retryLoadedErrors: retryLoadedSmart,
		smartNow,
		inFlightKeys: smartInFlightKeys,
		autoRetryingKeys: smartAutoRetryingKeys,
	} = useAutoSmart({
		enabled: true,
		onSmart: applySmartToActive,
	});

	useEffect(() => {
		if (scopedMode) {
			setSidebarLoading(false);
			sidebarBootstrapCompletedRef.current = false;
			return;
		}
		if (startupBootstrapRequestedRef.current) {
			return;
		}
		const startSidebarBootstrap = (allowRetry: boolean) => {
			startupBootstrapRequestedRef.current = true;
			void refreshSidebar({
				background: bootedFromWarmStart || hasCachedBriefs,
				includeNotifications: initialNotificationBootstrapRef.current,
			}).catch(() => {
				startupBootstrapRequestedRef.current = false;
				if (!allowRetry || startupSidebarRetriedRef.current) {
					return;
				}
				startupSidebarRetriedRef.current = true;
				startSidebarBootstrap(false);
			});
		};
		startSidebarBootstrap(true);
	}, [bootedFromWarmStart, hasCachedBriefs, refreshSidebar, scopedMode]);

	useEffect(() => {
		if (reactionTokenBootstrapRequestedRef.current) {
			return;
		}
		reactionTokenBootstrapRequestedRef.current = true;
		void loadReactionToken().then((status) => {
			if (status) {
				reactionTokenBootstrapCompletedRef.current = true;
			}
		});
	}, [loadReactionToken]);

	useEffect(() => {
		if (reactionTokenConfigured !== true) {
			return;
		}
		const releaseIds = activeFeedItems
			.filter(isReleaseFeedItem)
			.filter((item) => item.reactions?.status === "ready")
			.map((item) => item.id);
		if (releaseIds.length === 0) {
			return;
		}

		const uniqueReleaseIds = Array.from(new Set(releaseIds)).sort();
		const now = Date.now();
		const releaseIdsToRefresh = uniqueReleaseIds.filter((releaseId) => {
			const key = itemKey({ kind: "release", id: releaseId });
			if (reactionRefreshingKeysRef.current.has(key)) {
				return false;
			}
			const lastRefreshAt = lastFeedReactionRefreshByKeyRef.current.get(key);
			return (
				lastRefreshAt === undefined ||
				now - lastRefreshAt >= FEED_REACTION_REFRESH_TTL_MS
			);
		});
		if (releaseIdsToRefresh.length === 0) {
			return;
		}

		const refreshingKeys = releaseIdsToRefresh.map((releaseId) =>
			itemKey({ kind: "release", id: releaseId }),
		);
		for (const key of refreshingKeys) {
			reactionRefreshingKeysRef.current.add(key);
			lastFeedReactionRefreshByKeyRef.current.set(key, now);
		}
		const refreshBatches: string[][] = [];
		for (
			let i = 0;
			i < releaseIdsToRefresh.length;
			i += FEED_REACTION_REFRESH_BATCH_SIZE
		) {
			refreshBatches.push(
				releaseIdsToRefresh.slice(i, i + FEED_REACTION_REFRESH_BATCH_SIZE),
			);
		}

		void Promise.allSettled(
			refreshBatches.map((releaseIdsBatch) =>
				apiPostJson<FeedReactionRefreshResponse>(
					"/api/feed/reactions/refresh",
					{
						release_ids: releaseIdsBatch,
					},
				),
			),
		)
			.then((results) => {
				for (const result of results) {
					if (result.status !== "fulfilled") {
						const reason = result.reason;
						if (
							reason instanceof ApiError &&
							(reason.code === "pat_invalid" || reason.code === "pat_required")
						) {
							setReactionTokenConfigured(false);
							void loadReactionToken();
						}
						continue;
					}
					for (const item of result.value.items) {
						const key = itemKey({ kind: "release", id: item.release_id });
						if (
							reactionBusyKeysRef.current.has(key) ||
							reactionDesiredByKeyRef.current.has(key)
						) {
							continue;
						}
						reactionServerByKeyRef.current.set(key, item.reactions);
						applyReactionsToActive(
							{ kind: "release", id: item.release_id },
							item.reactions,
						);
					}
				}
			})
			.finally(() => {
				for (const key of refreshingKeys) {
					reactionRefreshingKeysRef.current.delete(key);
				}
			});
	}, [
		applyReactionsToActive,
		activeFeedItems,
		loadReactionToken,
		reactionTokenConfigured,
	]);

	useEffect(() => {
		const shouldLoadNotifications = hasDesktopSidebarInbox || tab === "inbox";
		if (
			!shouldLoadNotifications ||
			notificationsBootstrapRequestedRef.current
		) {
			return;
		}
		notificationsBootstrapRequestedRef.current = true;
		void refreshNotifications({ background: tab !== "inbox" }).catch(() => {
			notificationsBootstrapRequestedRef.current = false;
		});
	}, [hasDesktopSidebarInbox, refreshNotifications, tab]);

	useEffect(() => {
		window.localStorage.setItem(PAGE_DEFAULT_LANE_STORAGE_KEY, pageDefaultLane);
	}, [pageDefaultLane]);

	useEffect(() => {
		if (!feed.error) {
			return;
		}
		if (feed.error.phase !== "initial" || feed.items.length === 0) {
			return;
		}
		pushErrorToast("动态刷新失败", feed.error.message);
	}, [feed.error?.at, feed.error, feed.items.length, pushErrorToast]);

	useEffect(() => {
		if (tab !== "all" && tab !== "releases") {
			return;
		}
		if (feed.loadingInitial || feed.items.length === 0) {
			return;
		}
		void primeSmart(feed.items).catch((error) => {
			notifyGlobalError("润色预取失败", error, "润色预取失败，请稍后重试。");
		});
	}, [feed.items, feed.loadingInitial, notifyGlobalError, primeSmart, tab]);

	useEffect(() => {
		if (tab !== "all" && tab !== "releases") {
			return;
		}
		if (feed.loadingInitial || feed.items.length === 0) {
			return;
		}
		void retryLoadedTranslations(feed.items).catch((error) => {
			notifyGlobalError(
				"翻译自动补救失败",
				error,
				"翻译自动补救失败，请稍后手动重试。",
			);
		});
		void retryLoadedSmart(feed.items).catch((error) => {
			notifyGlobalError(
				"润色自动补救失败",
				error,
				"润色自动补救失败，请稍后手动重试。",
			);
		});
	}, [
		feed.items,
		feed.loadingInitial,
		notifyGlobalError,
		retryLoadedSmart,
		retryLoadedTranslations,
		tab,
	]);

	useEffect(() => {
		if (!accessTaskStream) return;

		const source = openAppEventSource(accessTaskStream.eventPath);
		let reconnectTimer: number | null = null;
		const clearReconnectTimer = () => {
			if (reconnectTimer === null) return;
			window.clearTimeout(reconnectTimer);
			reconnectTimer = null;
		};
		const refreshOnUi = () => {
			void refreshAll().catch((error) => {
				notifyGlobalError("页面刷新失败", error, "页面刷新失败，请稍后重试。");
			});
		};
		const parsePayload = (event: MessageEvent<string>): TaskEventPayload => {
			try {
				return JSON.parse(event.data) as TaskEventPayload;
			} catch {
				return {};
			}
		};
		const failStream = (message: string) => {
			clearReconnectTimer();
			setAccessSyncStage((current) =>
				current === "completed" ? current : "failed",
			);
			setAccessSyncProgress((current) =>
				current
					? {
							...current,
							stageLabel: "同步事件流已断开",
							detail: message,
						}
					: null,
			);
			pushErrorToast("同步事件流已断开", message);
			source.close();
			settleTaskWaiter(accessTaskStream.taskId, new Error(message));
			setAccessTaskStream((current) =>
				current?.taskId === accessTaskStream.taskId ? null : current,
			);
		};

		const onProgress = (event: Event) => {
			const payload = parsePayload(event as MessageEvent<string>);
			if (payload.stage === "star_refreshed") {
				setAccessSyncStage("star_refreshed");
				setAccessSyncProgress(
					accessSyncProgressFromStage("star_refreshed", payload),
				);
				refreshOnUi();
				return;
			}
			if (
				payload.stage === "release_summary" ||
				payload.stage === "social_summary" ||
				payload.stage === "notifications_summary"
			) {
				setAccessSyncProgress(
					accessSyncProgressFromStage(payload.stage, payload),
				);
			}
		};

		const onRunning = () => {
			setAccessSyncStage((current) =>
				current === "waiting" ? "running" : current,
			);
			setAccessSyncProgress((current) =>
				current && current.currentStep > 0
					? current
					: accessSyncProgressFromStage("running"),
			);
		};

		const onCompleted = (event: Event) => {
			const payload = parsePayload(event as MessageEvent<string>);
			const completedTaskId = accessTaskStream.taskId;
			const complete = async () => {
				clearReconnectTimer();
				const failed =
					payload.status !== "succeeded"
						? new Error(payload.error ?? "后台同步失败")
						: undefined;
				setAccessSyncStage(
					payload.status === "succeeded" ? "completed" : "failed",
				);
				setAccessSyncProgress((current) => ({
					currentStep:
						payload.status === "succeeded"
							? ACCESS_SYNC_TOTAL_STEPS
							: (current?.currentStep ?? 0),
					totalSteps: ACCESS_SYNC_TOTAL_STEPS,
					stageLabel:
						payload.status === "succeeded" ? "同步完成" : "后台同步失败",
					detail:
						payload.status === "succeeded"
							? "正在刷新页面内容"
							: (payload.error ?? "后台同步失败"),
				}));
				if (payload.status === "succeeded") {
					try {
						await refreshAll();
						clearDashboardLiveNotices();
						await checkDashboardUpdates({ emit: false });
					} catch (error) {
						const resolvedError =
							error instanceof Error ? error : new Error(String(error));
						notifyGlobalError(
							"同步后刷新失败",
							resolvedError,
							"同步已完成，但页面刷新失败，请稍后重试。",
						);
						source.close();
						settleTaskWaiter(completedTaskId, resolvedError);
						setAccessTaskStream((current) =>
							current?.taskId === completedTaskId ? null : current,
						);
						return;
					}
				} else if (payload.error) {
					pushErrorToast("后台同步失败", payload.error);
				}
				source.close();
				settleTaskWaiter(completedTaskId, failed);
				setAccessTaskStream((current) =>
					current?.taskId === completedTaskId ? null : current,
				);
			};
			void complete();
		};

		source.onopen = clearReconnectTimer;
		source.addEventListener("task.running", onRunning);
		source.addEventListener("task.progress", onProgress);
		source.addEventListener("task.completed", onCompleted);
		source.onerror = () => {
			if (source.readyState === EventSource.CLOSED) {
				failStream("后台任务事件流已断开，请刷新页面后重试。");
				return;
			}
			if (reconnectTimer !== null) return;
			reconnectTimer = window.setTimeout(() => {
				reconnectTimer = null;
				failStream("后台任务事件流恢复超时，请刷新页面后重试。");
			}, TASK_STREAM_RECOVERY_GRACE_MS);
		};

		return () => {
			clearReconnectTimer();
			source.removeEventListener("task.running", onRunning);
			source.removeEventListener("task.progress", onProgress);
			source.removeEventListener("task.completed", onCompleted);
			source.close();
		};
	}, [
		accessTaskStream,
		checkDashboardUpdates,
		clearDashboardLiveNotices,
		notifyGlobalError,
		pushErrorToast,
		refreshAll,
		settleTaskWaiter,
	]);

	useEffect(() => {
		if (refreshTaskStreams.length === 0) return;

		for (const task of refreshTaskStreams) {
			if (refreshTaskSourcesRef.current.has(task.taskId)) {
				continue;
			}

			const source = openAppEventSource(task.eventPath);
			let reconnectTimer: number | null = null;
			const clearReconnectTimer = () => {
				if (reconnectTimer === null) return;
				window.clearTimeout(reconnectTimer);
				reconnectTimer = null;
			};
			refreshTaskSourcesRef.current.set(task.taskId, source);

			const parsePayload = (event: MessageEvent<string>): TaskEventPayload => {
				try {
					return JSON.parse(event.data) as TaskEventPayload;
				} catch {
					return {};
				}
			};
			const close = () => {
				source.close();
				refreshTaskSourcesRef.current.delete(task.taskId);
				setRefreshTaskStreams((current) =>
					current.filter((item) => item.taskId !== task.taskId),
				);
			};
			const failStream = (message: string) => {
				clearReconnectTimer();
				pushErrorToast("后台同步事件流异常", message);
				settleTaskWaiter(task.taskId, new Error(message));
				close();
			};
			const onCompleted = (event: Event) => {
				const payload = parsePayload(event as MessageEvent<string>);
				const completedTaskId = task.taskId;
				const complete = async () => {
					clearReconnectTimer();
					const failed =
						payload.status !== "succeeded"
							? new Error(payload.error ?? "后台同步失败")
							: undefined;
					if (payload.status === "succeeded") {
						try {
							await refreshAll();
							clearDashboardLiveNotices();
							await checkDashboardUpdates({ emit: false });
						} catch (error) {
							const resolvedError =
								error instanceof Error ? error : new Error(String(error));
							notifyGlobalError(
								"同步后刷新失败",
								resolvedError,
								"同步已完成，但页面刷新失败，请稍后重试。",
							);
							settleTaskWaiter(completedTaskId, resolvedError);
							close();
							return;
						}
					} else if (payload.error) {
						pushErrorToast("后台同步失败", payload.error);
					}
					settleTaskWaiter(completedTaskId, failed);
					close();
				};
				void complete();
			};

			source.onopen = clearReconnectTimer;
			source.addEventListener("task.completed", onCompleted);
			source.onerror = () => {
				if (source.readyState === EventSource.CLOSED) {
					failStream("后台同步事件流已断开，请刷新页面后重试。");
					return;
				}
				if (reconnectTimer !== null) return;
				reconnectTimer = window.setTimeout(() => {
					reconnectTimer = null;
					failStream("后台同步事件流恢复超时，请刷新页面后重试。");
				}, TASK_STREAM_RECOVERY_GRACE_MS);
			};
		}
	}, [
		notifyGlobalError,
		pushErrorToast,
		refreshTaskStreams,
		refreshAll,
		checkDashboardUpdates,
		clearDashboardLiveNotices,
		settleTaskWaiter,
	]);

	useEffect(() => {
		return () => {
			for (const source of refreshTaskSourcesRef.current.values()) {
				source.close();
			}
			refreshTaskSourcesRef.current.clear();
			for (const [taskId, waiter] of taskWaitersRef.current) {
				waiter.settle(new Error(`Task stream ${taskId} was closed`));
			}
			taskWaitersRef.current.clear();
		};
	}, []);

	const onTranslateNow = useCallback(
		(item: FeedItem) => {
			void translateNow(item).catch((error) => {
				notifyFeedLaneError(item, "translated", error, translateNow);
			});
		},
		[notifyFeedLaneError, translateNow],
	);
	const onSmartNow = useCallback(
		(item: FeedItem) => {
			void smartNow(item).catch((error) => {
				notifyFeedLaneError(item, "smart", error, smartNow);
			});
		},
		[notifyFeedLaneError, smartNow],
	);
	const requestLaneIfNeeded = useCallback(
		(item: FeedItem, lane: FeedLane) => {
			if (!isLaneCapableFeedItem(item)) {
				return;
			}
			if (
				lane === "translated" &&
				(item.translated?.status === "missing" ||
					(item.translated?.status === "error" &&
						item.translated?.auto_translate !== false))
			) {
				void translateNow(item).catch((error) => {
					notifyFeedLaneError(item, "translated", error, translateNow);
				});
			}
			if (
				lane === "smart" &&
				(item.smart?.status === "missing" ||
					(item.smart?.status === "error" &&
						item.smart?.auto_translate !== false))
			) {
				void smartNow(item).catch((error) => {
					notifyFeedLaneError(item, "smart", error, smartNow);
				});
			}
		},
		[notifyFeedLaneError, smartNow, translateNow],
	);
	const onSelectLane = useCallback(
		(item: FeedItem, lane: FeedLane) => {
			const key = feedItemKey(item);
			setSelectedLaneByKey((prev) => ({ ...prev, [key]: lane }));
			requestLaneIfNeeded(item, lane);
		},
		[requestLaneIfNeeded],
	);
	const onSelectPageDefaultLane = useCallback((lane: FeedLane) => {
		setPageDefaultLane(lane);
		setSelectedLaneByKey({});
	}, []);
	const registerFeedItem = useCallback(
		(item: FeedItem) => (element: HTMLElement | null) => {
			if (!isLaneCapableFeedItem(item)) {
				return;
			}
			registerTranslate(item)(element);
			registerSmart(item)(element);
		},
		[registerSmart, registerTranslate],
	);
	const openPatDialog = useCallback(
		(
			message: string,
			pending?: { releaseId: string; content: ReactionContent },
		) => {
			if (pending) {
				pendingReactionRef.current = pending;
			}
			setPatGuideMessage(message);
			setPatGuideOpen(true);
		},
		[],
	);
	const closePatDialog = useCallback(() => {
		setPatGuideOpen(false);
		setPatGuideMessage(null);
		clearPatDraft();
		pendingReactionRef.current = null;
	}, [clearPatDraft]);
	const onSavePatFromDialog = useCallback(() => {
		void savePat().then((status) => {
			if (!status || !isReactionTokenUsable(status)) return;
			closePatDialog();
		});
	}, [closePatDialog, savePat]);

	const flushPendingReactions = useCallback(
		(key: string) => {
			if (reactionBusyKeysRef.current.has(key)) return;
			const server = reactionServerByKeyRef.current.get(key);
			const desired = reactionDesiredByKeyRef.current.get(key);
			if (!server || !desired) return;

			const content = firstPendingReactionContent(server, desired);
			if (!content) {
				reactionDesiredByKeyRef.current.delete(key);
				return;
			}

			const item = itemFromKey(key);
			if (!item) return;

			const nextBusy = new Set(reactionBusyKeysRef.current);
			nextBusy.add(key);
			reactionBusyKeysRef.current = nextBusy;
			setReactionBusyKeys(nextBusy);

			void apiPostJson<ToggleReleaseReactionResponse>(
				"/api/release/reactions/toggle",
				{
					release_id: item.id,
					content,
				},
			)
				.then((res) => {
					reactionServerByKeyRef.current.set(key, res.reactions);
					setReactionTokenConfigured(true);
					setReactionErrorByKey((prev) => {
						if (!(key in prev)) return prev;
						const next = { ...prev };
						delete next[key];
						return next;
					});

					const latestDesired = reactionDesiredByKeyRef.current.get(key);
					if (
						!latestDesired ||
						!firstPendingReactionContent(res.reactions, latestDesired)
					) {
						applyReactionsToActive(item, res.reactions);
						reactionDesiredByKeyRef.current.delete(key);
					}
				})
				.catch((err) => {
					const stable = reactionServerByKeyRef.current.get(key);
					if (stable) {
						reactionDesiredByKeyRef.current.set(key, stable);
						applyReactionsToActive(item, stable);
					} else {
						reactionDesiredByKeyRef.current.delete(key);
					}

					if (err instanceof ApiError) {
						if (err.status === 401) {
							setReactionErrorByKey((prev) => ({
								...prev,
								[key]: sessionExpiredHint(),
							}));
							return;
						}
						if (err.code === "pat_required" || err.code === "pat_invalid") {
							setReactionTokenConfigured(false);
							openPatDialog(
								err.code === "pat_invalid"
									? "当前 GitHub PAT 无效或已过期，请重新校验后保存。"
									: "先补齐 GitHub PAT，才能继续使用站内反馈。",
								{
									releaseId: item.id,
									content,
								},
							);
							return;
						}
					}

					const raw = err instanceof Error ? err.message : String(err);
					let message = raw;
					if (
						raw.includes("OAuth app restrictions") ||
						raw.includes(
							"organization has enabled OAuth App access restrictions",
						)
					) {
						message = "该仓库限制了站内反馈，请在 GitHub 页面操作。";
					}
					setReactionErrorByKey((prev) => ({ ...prev, [key]: message }));
				})
				.finally(() => {
					const nextBusy = new Set(reactionBusyKeysRef.current);
					nextBusy.delete(key);
					reactionBusyKeysRef.current = nextBusy;
					setReactionBusyKeys(nextBusy);

					const latestServer = reactionServerByKeyRef.current.get(key);
					const latestDesired = reactionDesiredByKeyRef.current.get(key);
					if (
						latestServer &&
						latestDesired &&
						firstPendingReactionContent(latestServer, latestDesired)
					) {
						void flushPendingReactions(key);
					}
				});
		},
		[applyReactionsToActive, openPatDialog],
	);

	const scheduleReactionFlush = useCallback(
		(key: string) => {
			const timers = reactionFlushTimerByKeyRef.current;
			const prev = timers.get(key);
			if (prev !== undefined) {
				window.clearTimeout(prev);
			}
			const timer = window.setTimeout(() => {
				timers.delete(key);
				flushPendingReactions(key);
			}, 350);
			timers.set(key, timer);
		},
		[flushPendingReactions],
	);

	const performReactionToggle = useCallback(
		(item: FeedItem, content: ReactionContent) => {
			if (!isReleaseFeedItem(item)) {
				return;
			}
			const key = itemKey(item);
			const current =
				reactionDesiredByKeyRef.current.get(key) ??
				(item.reactions?.status === "ready" ? item.reactions : null);
			if (!current) return;

			if (!reactionServerByKeyRef.current.has(key)) {
				reactionServerByKeyRef.current.set(key, current);
			}
			const optimistic = buildOptimisticReactions(current, content);
			reactionDesiredByKeyRef.current.set(key, optimistic);
			applyReactionsToActive(item, optimistic);

			setReactionErrorByKey((prev) => {
				if (!(key in prev)) return prev;
				const next = { ...prev };
				delete next[key];
				return next;
			});
			scheduleReactionFlush(key);
		},
		[applyReactionsToActive, scheduleReactionFlush],
	);

	const onToggleReaction = useCallback(
		(item: FeedItem, content: ReactionContent) => {
			if (reactionTokenConfigured !== true) {
				if (!isReleaseFeedItem(item)) return;
				const message =
					reactionTokenConfigured === false
						? "先补齐 GitHub PAT，才能继续使用站内反馈。"
						: patCheckState === "error"
							? (patCheckMessage ??
								"GitHub PAT 状态读取失败，请稍后重试或在这里重新校验。")
							: "正在读取 GitHub PAT 状态，请稍后再试。";
				openPatDialog(message, {
					releaseId: item.id,
					content,
				});
				return;
			}
			performReactionToggle(item, content);
		},
		[
			openPatDialog,
			patCheckMessage,
			patCheckState,
			performReactionToggle,
			reactionTokenConfigured,
		],
	);

	useEffect(
		() => () => {
			for (const timer of reactionFlushTimerByKeyRef.current.values()) {
				window.clearTimeout(timer);
			}
			reactionFlushTimerByKeyRef.current.clear();
		},
		[],
	);

	const onGenerateBrief = useCallback(() => {
		void run(
			"Generate brief",
			async () => {
				await apiPost<BriefGenerateResponse>("/api/briefs/generate");
				await refreshSidebar();
				setLiveNotices((current) => ({ ...current, briefs: undefined }));
				await checkDashboardUpdates({ emit: false, include: ["briefs"] });
			},
			{
				errorTitle: "日报生成失败",
				fallback: "日报生成失败，请稍后重试。",
			},
		);
	}, [checkDashboardUpdates, refreshSidebar, run]);
	const onGenerateBriefForDate = useCallback(
		async (date: string) => {
			try {
				await apiPostJson<BriefGenerateResponse>("/api/briefs/generate", {
					date,
				});
				await refreshSidebar();
				setLiveNotices((current) => ({ ...current, briefs: undefined }));
				await checkDashboardUpdates({ emit: false, include: ["briefs"] });
			} catch (error) {
				notifyGlobalError("日报生成失败", error, "日报生成失败，请稍后重试。");
				throw error;
			}
		},
		[checkDashboardUpdates, notifyGlobalError, refreshSidebar],
	);
	const onSyncInbox = useCallback(() => {
		void run(
			"Sync inbox",
			async () => {
				const task = await apiPost<TaskAcceptedResponse>(
					"/api/sync/notifications?return_mode=task_id",
				);
				await trackTaskStream(task, "refresh");
			},
			{
				errorTitle: "Inbox 同步失败",
				fallback: "Inbox 同步失败，请稍后重试。",
			},
		);
	}, [run, trackTaskStream]);

	const accessSyncRunning =
		accessTaskStream !== null &&
		accessSyncStage !== "completed" &&
		accessSyncStage !== "failed";
	const onSyncAll = useCallback(() => {
		if (
			syncAllInFlightRef.current ||
			busy === SYNC_ALL_LABEL ||
			accessSyncRunning
		) {
			return;
		}
		syncAllInFlightRef.current = true;
		setAccessSyncProgress(accessSyncProgressFromStage("waiting"));
		void run(
			SYNC_ALL_LABEL,
			async () => {
				const task = await apiPost<TaskAcceptedResponse>(
					"/api/sync/all?return_mode=task_id",
				);
				await trackTaskStream(task, "access");
			},
			{
				errorTitle: "全量同步失败",
				fallback: "全量同步失败，请稍后重试。",
			},
		).finally(() => {
			syncAllInFlightRef.current = false;
		});
	}, [accessSyncRunning, busy, run, trackTaskStream]);
	const syncingAll = busy === SYNC_ALL_LABEL || accessSyncRunning;
	const syncingInbox = busy === "Sync inbox";

	const aiDisabledHint = useMemo(() => {
		const any = activeFeedItems.find(
			(it) =>
				it.translated?.status === "disabled" || it.smart?.status === "disabled",
		);
		return Boolean(any);
	}, [activeFeedItems]);

	const onSelectTab = useCallback(
		(nextTab: Tab) => {
			setRouteState({
				tab: nextTab,
				scope,
				selectedBriefId:
					!scope && nextTab === "briefs" ? effectiveSelectedBriefId : null,
				activeReleaseId: null,
				activeReleaseLocator: null,
				activeAnnouncementLocator: null,
				releaseReturnTab: scope ? "all" : "briefs",
			});
		},
		[effectiveSelectedBriefId, scope, setRouteState],
	);

	const onOpenReleaseDetail = useCallback(
		(target: DashboardReleaseTarget) => {
			const nextScope = target.scope ?? scope;
			const nextSelectedBriefId =
				!nextScope && target.fromTab === "briefs"
					? (target.selectedBriefId ??
						routeSelectedBriefId ??
						effectiveSelectedBriefId)
					: null;
			setRouteState({
				tab: target.fromTab,
				scope: nextScope,
				selectedBriefId: nextSelectedBriefId,
				activeReleaseId: target.releaseId,
				activeReleaseLocator: target.locator,
				activeAnnouncementLocator: null,
				releaseReturnTab: target.fromTab,
			});
		},
		[effectiveSelectedBriefId, routeSelectedBriefId, scope, setRouteState],
	);

	const onCloseReleaseDetail = useCallback(() => {
		setRouteState(
			{
				tab: releaseReturnTab,
				scope,
				selectedBriefId:
					!scope && releaseReturnTab === "briefs" ? routeSelectedBriefId : null,
				activeReleaseId: null,
				activeReleaseLocator: null,
				activeAnnouncementLocator: null,
				releaseReturnTab,
			},
			{ replace: true },
		);
	}, [releaseReturnTab, routeSelectedBriefId, scope, setRouteState]);

	const onCloseAnnouncementDetail = useCallback(() => {
		setRouteState(
			{
				tab: releaseReturnTab,
				scope,
				selectedBriefId:
					!scope && releaseReturnTab === "briefs" ? routeSelectedBriefId : null,
				activeReleaseId: null,
				activeReleaseLocator: null,
				activeAnnouncementLocator: null,
				releaseReturnTab,
			},
			{ replace: true },
		);
	}, [releaseReturnTab, routeSelectedBriefId, scope, setRouteState]);

	const onReleaseDetailResolved = useCallback(
		(detail: {
			release_id: string;
			repo_full_name: string | null;
			tag_name: string;
			html_url: string;
		}) => {
			if (activeReleaseLocator) {
				return;
			}
			const locator = releaseLocatorFromReleaseDetail(detail);
			if (!locator) {
				return;
			}
			setRouteState(
				{
					tab,
					scope,
					selectedBriefId:
						!scope && tab === "briefs" ? routeSelectedBriefId : null,
					activeReleaseId: detail.release_id,
					activeReleaseLocator: locator,
					activeAnnouncementLocator: null,
					releaseReturnTab,
				},
				{ replace: true },
			);
		},
		[
			activeReleaseLocator,
			releaseReturnTab,
			routeSelectedBriefId,
			scope,
			setRouteState,
			tab,
		],
	);
	const hasActiveAnnouncementDetail = activeAnnouncementLocator !== null;
	const showPageLaneSelector =
		!hasActiveAnnouncementDetail && (tab === "all" || tab === "releases");
	const renderSidebarInbox = !scopedMode && hasDesktopSidebarInbox;
	const renderSidebar =
		!hasActiveAnnouncementDetail &&
		((tab === "briefs" && hasTabletSidebar) || renderSidebarInbox);
	const dashboardContentLayoutClassName = scopedMode
		? "grid gap-4 lg:grid-cols-[minmax(0,1fr)_360px] lg:gap-6"
		: renderSidebar
			? "grid gap-4 md:grid-cols-[minmax(0,1fr)_360px] md:gap-6"
			: "grid gap-4 md:gap-6";
	const bootNetworkUnavailable =
		bootErrorKind === "offline" || bootErrorKind === "network";
	const retryDashboardNetwork = useCallback(async () => {
		await Promise.allSettled([
			onRetryBoot?.(),
			readableSectionsActive
				? readableSections.loadInitial()
				: feed.loadInitial(),
		]);
	}, [
		feed.loadInitial,
		onRetryBoot,
		readableSectionsActive,
		readableSections.loadInitial,
	]);

	const renderFeedPanel = (
		mode: "all" | "releases" | "stars" | "followers",
	) => {
		const rootReadable =
			mode === "all" && !scopedMode && readableSectionsActive;
		const filteredItems = filterFeedItemsForTab(feed.items, mode, {
			scoped: scopedMode,
		});
		const feedNetworkUnavailable =
			feed.error?.phase === "initial" &&
			(feed.error.kind === "offline" || feed.error.kind === "network");
		const networkUnavailable =
			bootNetworkUnavailable || Boolean(feedNetworkUnavailable);
		const networkUnavailableMessage =
			(feedNetworkUnavailable ? feed.error?.message : null) ??
			bootError ??
			"当前处于离线状态，正在显示可用缓存内容。";
		const networkUnavailableDetail =
			(feedNetworkUnavailable ? feed.error?.detail : null) ?? bootErrorDetail;
		const blockingFeedError =
			feed.error?.phase === "initial" &&
			filteredItems.length === 0 &&
			!networkUnavailable;
		const offlineEmpty =
			networkUnavailable && !feed.loadingInitial && filteredItems.length === 0;
		const offlineWithCachedContent =
			networkUnavailable && filteredItems.length > 0;
		const scopedEmpty =
			scopedMode && !feed.loadingInitial && filteredItems.length === 0;
		const readableEmpty =
			rootReadable &&
			!readableSections.loadingInitial &&
			!readableSections.error &&
			readableSections.sections.length === 0;
		return (
			<>
				{!rootReadable && offlineWithCachedContent ? (
					<div
						className="mb-4 rounded-xl border border-amber-300/45 bg-amber-50/80 px-4 py-3 text-sm text-amber-950 shadow-sm dark:border-amber-300/20 dark:bg-amber-950/20 dark:text-amber-100"
						data-dashboard-offline-cache-banner="true"
					>
						<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
							<div className="flex min-w-0 items-start gap-3">
								<div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-full border border-amber-300/45 bg-amber-100/80 text-amber-700 dark:border-amber-300/20 dark:bg-amber-400/10 dark:text-amber-200">
									<WifiOff className="size-4" />
								</div>
								<div className="min-w-0">
									<p className="font-semibold">正在显示缓存内容</p>
									<p className="mt-0.5 text-amber-900/80 dark:text-amber-100/80">
										{networkUnavailableMessage}
									</p>
								</div>
							</div>
							<Button
								type="button"
								variant="outline"
								size="sm"
								className="w-full border-amber-300/60 bg-background/70 font-mono text-xs hover:bg-background sm:w-auto"
								onClick={() => void retryDashboardNetwork()}
							>
								<RefreshCcw className="size-4" />
								重试连接
							</Button>
						</div>
					</div>
				) : null}

				{!rootReadable && offlineEmpty ? (
					<div
						className="bg-card/75 mb-4 rounded-2xl border border-amber-300/45 p-6 shadow-sm dark:border-amber-300/20"
						data-dashboard-offline-empty-state="true"
					>
						<div className="flex flex-col gap-4 sm:flex-row sm:items-start">
							<div className="flex size-11 shrink-0 items-center justify-center rounded-full border border-amber-300/45 bg-amber-100/80 text-amber-700 dark:border-amber-300/20 dark:bg-amber-400/10 dark:text-amber-200">
								<WifiOff className="size-5" />
							</div>
							<div className="min-w-0 flex-1">
								<h2 className="text-base font-semibold tracking-tight">
									离线时没有可用缓存
								</h2>
								<p className="text-muted-foreground mt-1 text-sm leading-6">
									当前页面之前没有保存到本地的内容；恢复网络后可以重新加载最新动态。
								</p>
								<p className="mt-3 text-sm leading-6 text-amber-900 dark:text-amber-100">
									{networkUnavailableMessage}
								</p>
								{networkUnavailableDetail ? (
									<p className="text-muted-foreground mt-2 break-words font-mono text-xs">
										{networkUnavailableDetail}
									</p>
								) : null}
								<div className="mt-4 flex flex-wrap gap-2">
									<Button
										type="button"
										variant="outline"
										onClick={() => void retryDashboardNetwork()}
									>
										<RefreshCcw className="size-4" />
										重试连接
									</Button>
								</div>
							</div>
						</div>
					</div>
				) : null}

				{readableEmpty ||
				(!rootReadable &&
					!blockingFeedError &&
					!offlineEmpty &&
					!feed.loadingInitial &&
					filteredItems.length === 0) ? (
					<div className="bg-card/70 mb-4 rounded-xl border p-6 shadow-sm">
						{scopedEmpty && scope ? (
							<>
								<h2 className="text-base font-semibold tracking-tight">
									当前范围暂无更新
								</h2>
								<p className="text-muted-foreground mt-1 text-sm leading-6">
									最近还没有新的发布或相关动态。你可以稍后再看，或返回工作台浏览其他更新。
								</p>
								<div className="mt-4 flex flex-wrap gap-2">
									<Button asChild variant="outline">
										<InternalLink href="/" to="/">
											返回工作台
										</InternalLink>
									</Button>
									<Button
										type="button"
										onClick={() => {
											void refreshFeed();
										}}
									>
										<RefreshCcw className="size-4" />
										重新加载
									</Button>
								</div>
							</>
						) : accessSyncRunning ? (
							<>
								<h2 className="text-base font-semibold tracking-tight">
									正在同步你的 Star / Release
								</h2>
								<p className="text-muted-foreground mt-1 text-sm">
									先展示服务端已有缓存，再补齐最新
									release、被加星和被关注记录；完成后这里会自动刷新。
								</p>
							</>
						) : (
							<>
								<h2 className="text-base font-semibold tracking-tight">
									还没有缓存内容
								</h2>
								<p className="text-muted-foreground mt-1 text-sm">
									可以先同步一次，加载发布、星标和关注动态。
								</p>
								<div className="mt-4 flex flex-wrap gap-2">
									<Button disabled={Boolean(busy)} onClick={onSyncAll}>
										{SYNC_ALL_LABEL}
									</Button>
								</div>
							</>
						)}
					</div>
				) : null}

				{rootReadable ? (
					<FeedReadableSectionList
						sections={readableSections.sections}
						details={readableSections.details}
						error={readableSections.error}
						loadingInitial={readableSections.loadingInitial}
						loadingMore={readableSections.loadingMore}
						hasMore={readableSections.hasMore}
						onLoadMore={() => void readableSections.loadMore()}
						onRetry={() => void readableSections.retry()}
						onLoadSectionItems={(sectionId, cursor) => {
							void readableSections.loadSectionItems(sectionId, cursor);
						}}
						feedCardProps={{
							currentViewer: {
								login: me.user.login,
								avatar_url: me.user.avatar_url,
								html_url: `https://github.com/${me.user.login}`,
							},
							sourceTab: mode,
							currentScope: null,
							translationInFlightKeys,
							translationAutoRetryingKeys,
							smartInFlightKeys,
							smartAutoRetryingKeys,
							registerItemRef: registerFeedItem,
							selectedLaneByKey: Object.fromEntries(
								activeFeedItems.map((item) => [
									feedItemKey(item),
									resolveLaneForItem(
										item,
										selectedLaneByKey,
										pageDefaultLane,
										allowReleaseItemLaneOverride,
									),
								]),
							),
							onSelectLane,
							onTranslateNow,
							onSmartNow,
							reactionBusyKeys,
							reactionErrorByKey,
							onToggleReaction,
							freshKeys: new Set(),
						}}
						onOpenReleaseFromBrief={onOpenReleaseDetail}
						onOpenBrief={openBrief}
						onCopyBrief={copyBrief}
						onGenerateBriefForDate={onGenerateBriefForDate}
					/>
				) : (
					<FeedGroupedList
						mode={mode}
						sourceTab={mode}
						currentScope={scope}
						items={filteredItems}
						currentViewer={{
							login: me.user.login,
							avatar_url: me.user.avatar_url,
							html_url: `https://github.com/${me.user.login}`,
						}}
						briefs={briefs}
						dailyBoundaryLocal={dailyBoundaryLocal}
						dailyBoundaryTimeZone={dailyBoundaryTimeZone}
						dailyBoundaryUtcOffsetMinutes={dailyBoundaryUtcOffsetMinutes}
						error={offlineEmpty ? null : feed.error}
						loadingInitial={feed.loadingInitial}
						loadingMore={feed.loadingMore}
						hasMore={feed.hasMore}
						translationInFlightKeys={translationInFlightKeys}
						translationAutoRetryingKeys={translationAutoRetryingKeys}
						smartInFlightKeys={smartInFlightKeys}
						smartAutoRetryingKeys={smartAutoRetryingKeys}
						registerItemRef={registerFeedItem}
						onLoadMore={feed.loadMore}
						onRetryInitial={feed.loadInitial}
						selectedLaneByKey={Object.fromEntries(
							filteredItems.map((item) => [
								feedItemKey(item),
								resolveLaneForItem(
									item,
									selectedLaneByKey,
									pageDefaultLane,
									allowReleaseItemLaneOverride,
								),
							]),
						)}
						onSelectLane={onSelectLane}
						onTranslateNow={onTranslateNow}
						onSmartNow={onSmartNow}
						reactionBusyKeys={reactionBusyKeys}
						reactionErrorByKey={reactionErrorByKey}
						freshKeys={feed.freshKeys}
						onToggleReaction={onToggleReaction}
						onOpenReleaseFromBrief={
							mode === "all" ? onOpenReleaseDetail : undefined
						}
						onOpenBrief={mode === "all" && !scope ? openBrief : undefined}
						onCopyBrief={mode === "all" && !scope ? copyBrief : undefined}
						onEnsureBriefDetail={
							mode === "all" && !scope ? ensureBriefDetail : undefined
						}
						onRetryBriefDetail={
							mode === "all" && !scope
								? (briefId) => {
										void loadBriefDetail(briefId);
									}
								: undefined
						}
						onGenerateBriefForDate={
							mode === "all" && !scope ? onGenerateBriefForDate : undefined
						}
						briefDetailLoadingIds={briefDetailLoadingIds}
						briefDetailErrors={briefDetailErrors}
						copyingBriefId={copyingBriefId}
						newContentBoundaries={activeFeedNotices.map((notice, index) => ({
							id: notice.boundaryId,
							count: notice.boundaryKeys.length,
							label: "动态",
							latestKeys: notice.boundaryKeys,
							afterKey: notice.boundaryAfterKey,
							isLatest: index === 0,
							isSealed: notice.sealed,
							onExitedViewport: dismissFeedBoundary,
							onFreshAreaEntered: sealFeedBoundary,
							onResolveAfterKey: resolveFeedBoundary,
							onReveal: () => {
								void revealFeedUpdates(notice).catch((error) => {
									notifyGlobalError(
										"新动态显示失败",
										error,
										"新动态显示失败，请稍后重试。",
									);
								});
							},
						}))}
					/>
				)}
			</>
		);
	};

	useEffect(() => {
		if (isRouteControlled) return;
		const nextUrl = buildDashboardRouteUrl(routeState);
		const currentUrl = `${window.location.pathname}${window.location.search}`;
		if (nextUrl !== currentUrl) {
			window.history.replaceState({}, "", nextUrl);
		}
	}, [isRouteControlled, routeState]);

	useEffect(() => {
		if (tab !== "all" && tab !== "releases") {
			return;
		}
		if (feed.loadingInitial || feed.items.length === 0) {
			return;
		}
		for (const item of feed.items) {
			if (
				allowReleaseItemLaneOverride &&
				selectedLaneByKey[feedItemKey(item)]
			) {
				continue;
			}
			requestLaneIfNeeded(
				item,
				resolveLaneForItem(
					item,
					selectedLaneByKey,
					pageDefaultLane,
					allowReleaseItemLaneOverride,
				),
			);
		}
	}, [
		allowReleaseItemLaneOverride,
		feed.items,
		feed.loadingInitial,
		pageDefaultLane,
		requestLaneIfNeeded,
		selectedLaneByKey,
		tab,
	]);

	useEffect(() => {
		if (feed.loadingInitial || sidebarLoading) {
			return;
		}
		if (!shellHydrated) {
			setShellHydrated(true);
		}
	}, [feed.loadingInitial, shellHydrated, sidebarLoading]);

	useEffect(() => {
		dashboardSessionStateByUser.set(me.user.id, {
			scopeSignature: currentScopeSignature,
			notifications,
			briefs,
			selectedBriefId: effectiveSelectedBriefId,
			shellHydrated,
			sidebarBootstrapped: sidebarBootstrapCompletedRef.current,
			notificationsBootstrapped: notificationsBootstrapCompletedRef.current,
			reactionTokenBootstrapped: reactionTokenBootstrapCompletedRef.current,
			reactionTokenConfigured,
		});
	}, [
		briefs,
		currentScopeSignature,
		effectiveSelectedBriefId,
		me.user.id,
		notifications,
		reactionTokenConfigured,
		shellHydrated,
	]);

	useEffect(() => {
		if (readableSectionsActive || feed.loadingInitial || sidebarLoading) {
			return;
		}
		persistDashboardWarmSnapshot({
			userId: me.user.id,
			viewerStateKey: buildDashboardWarmViewerStateKey(me),
			routeState: buildDashboardWarmRouteState(routeState),
			feedRequestType,
			feedItems: feed.items,
			nextCursor: feed.nextCursor,
			notifications,
			briefs,
			selectedBriefId: effectiveSelectedBriefId,
		});
	}, [
		briefs,
		effectiveSelectedBriefId,
		feed.items,
		feed.loadingInitial,
		feed.nextCursor,
		me.user.id,
		notifications,
		routeState,
		sidebarLoading,
		feedRequestType,
		readableSectionsActive,
	]);

	const showStartupSkeleton =
		!shellHydrated && (feed.loadingInitial || sidebarLoading);

	if (showStartupSkeleton) {
		return <DashboardStartupSkeleton me={me} />;
	}

	return (
		<AppShell
			header={
				<DashboardHeader
					login={me.user.login}
					name={me.user.name}
					avatarUrl={me.user.avatar_url}
					email={me.user.email}
					isAdmin={isAdmin}
					aiDisabledHint={aiDisabledHint}
					busy={Boolean(busy)}
					syncingAll={syncingAll}
					syncProgress={accessSyncProgress}
					onSyncAll={onSyncAll}
					mineHref={buildDashboardScopeHref({ kind: "mine" })}
					mineLabel={DASHBOARD_MINE_ENTRY_LABEL}
					followingHref={buildDashboardScopeHref({ kind: "following" })}
					followingLabel={DASHBOARD_FOLLOWING_ENTRY_LABEL}
					mobileControlBand={
						<DashboardMobileControlBand
							tab={tab}
							onSelectTab={(nextTab) => onSelectTab(nextTab)}
							showPageLaneSelector={showPageLaneSelector}
							pageLane={effectivePageDefaultLane}
							onSelectPageLane={onSelectPageDefaultLane}
							layout="stacked"
							tabOptions={
								scopedMode ? SCOPED_TAB_OPTIONS : DASHBOARD_TAB_OPTIONS
							}
						/>
					}
				/>
			}
			notice={<VersionUpdateNotice />}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<div data-dashboard-hydration-source={hydrationSource}>
				<Tabs
					value={tab}
					onValueChange={(nextTab) => onSelectTab(nextTab as Tab)}
					className="gap-4 sm:gap-6"
				>
					<div className="hidden flex-wrap items-center justify-between gap-2 sm:flex">
						<DashboardTabsList
							options={scopedMode ? SCOPED_TAB_OPTIONS : DASHBOARD_TAB_OPTIONS}
						/>

						<div
							className="flex min-h-8 items-center gap-2 self-center"
							data-dashboard-secondary-controls
						>
							{showPageLaneSelector ? (
								<FeedPageLaneSelector
									value={effectivePageDefaultLane}
									onValueChange={onSelectPageDefaultLane}
									className="hidden sm:inline-flex"
								/>
							) : null}
							{aiDisabledHint ? (
								<span className="inline-flex h-8 items-center rounded-lg border border-border/45 bg-muted/35 px-3 font-mono text-xs text-muted-foreground">
									AI 未配置，将只显示原文
								</span>
							) : null}
							{busy ? (
								<span className="inline-flex h-8 items-center rounded-lg border border-border/45 bg-muted/35 px-3 font-mono text-xs text-muted-foreground">
									{busy}…
								</span>
							) : null}
						</div>
					</div>

					<div className={dashboardContentLayoutClassName}>
						<section className="min-w-0">
							{scopedMode && scope ? (
								<div className="lg:hidden">
									<ScopedSummaryCard
										scope={scope}
										feedItems={feed.items}
										personalRepos={scope.kind === "mine" ? personalRepos : null}
										personalReposLoading={
											scope.kind === "mine" && personalReposLoading
										}
										personalReposError={
											scope.kind === "mine" ? personalReposError : null
										}
										followingRepos={
											scope.kind === "following" || scope.kind === "repo"
												? followingRepos
												: null
										}
										followingReposLoading={
											(scope.kind === "following" || scope.kind === "repo") &&
											followingReposLoading
										}
										followingReposError={
											scope.kind === "following" || scope.kind === "repo"
												? followingReposError
												: null
										}
										reloadFollowingRepos={async () => {
											await followingReposQuery.refetch();
										}}
									/>
								</div>
							) : null}
							{hasActiveAnnouncementDetail && activeAnnouncementLocator ? (
								<AnnouncementDetailPage
									owner={activeAnnouncementLocator.owner}
									repo={activeAnnouncementLocator.repo}
									number={activeAnnouncementLocator.number}
									onBack={onCloseAnnouncementDetail}
								/>
							) : (
								<>
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
										<NewContentNotice
											count={liveNotices.briefs?.newCount ?? 0}
											label="日报"
											onReveal={() => {
												void revealBriefUpdates().catch((error) => {
													notifyGlobalError(
														"新日报显示失败",
														error,
														"新日报显示失败，请稍后重试。",
													);
												});
											}}
										/>
										<ReleaseDailyCard
											briefs={briefs}
											selectedId={effectiveSelectedBriefId}
											busy={busy === "Generate brief"}
											freshKeys={freshBriefKeys}
											error={
												briefsError?.phase === "initial"
													? briefsError.message
													: null
											}
											detailLoading={selectedBriefDetailLoading}
											detailError={selectedBriefDetailError}
											onGenerate={onGenerateBrief}
											onRetry={() =>
												void refreshSidebar({
													includeNotifications:
														hasDesktopSidebarInbox || tab === "inbox",
												})
											}
											onRetryDetail={
												selectedBrief
													? () => void loadBriefDetail(selectedBrief.id)
													: undefined
											}
											onOpenRelease={onOpenReleaseDetail}
										/>
									</TabsContent>
									<TabsContent value="inbox" className="mt-0 min-w-0">
										<NewContentNotice
											count={liveNotices.notifications?.newCount ?? 0}
											label="Inbox 内容"
											onReveal={() => {
												void revealNotificationUpdates().catch((error) => {
													notifyGlobalError(
														"Inbox 新内容显示失败",
														error,
														"Inbox 新内容显示失败，请稍后重试。",
													);
												});
											}}
										/>
										<InboxList
											notifications={notifications}
											loading={notificationsLoading}
											busy={Boolean(busy)}
											syncing={syncingInbox}
											freshKeys={freshNotificationKeys}
											error={
												notificationsError?.phase === "initial"
													? notificationsError.message
													: null
											}
											onSync={tab === "inbox" ? onSyncInbox : undefined}
											onRetry={() =>
												void refreshNotifications({ background: false })
											}
										/>
									</TabsContent>
								</>
							)}
						</section>

						{scopedMode && scope ? (
							<aside className="hidden space-y-4 lg:block">
								<ScopedSummaryCard
									scope={scope}
									feedItems={feed.items}
									personalRepos={scope.kind === "mine" ? personalRepos : null}
									personalReposLoading={
										scope.kind === "mine" && personalReposLoading
									}
									personalReposError={
										scope.kind === "mine" ? personalReposError : null
									}
									followingRepos={
										scope.kind === "following" || scope.kind === "repo"
											? followingRepos
											: null
									}
									followingReposLoading={
										(scope.kind === "following" || scope.kind === "repo") &&
										followingReposLoading
									}
									followingReposError={
										scope.kind === "following" || scope.kind === "repo"
											? followingReposError
											: null
									}
									reloadFollowingRepos={async () => {
										await followingReposQuery.refetch();
									}}
									desktop
								/>
							</aside>
						) : renderSidebar ? (
							<aside className="space-y-4 sm:space-y-6">
								{tab === "briefs" ? (
									<BriefListCard
										briefs={briefs}
										selectedId={effectiveSelectedBriefId}
										freshKeys={freshBriefKeys}
										onSelectId={(id) => openBrief(id, { replace: true })}
									/>
								) : null}
								{renderSidebarInbox ? (
									<div data-dashboard-sidebar-inbox="true">
										{tab !== "inbox" ? (
											<NewContentNotice
												count={liveNotices.notifications?.newCount ?? 0}
												label="Inbox 内容"
												onReveal={() => {
													void revealNotificationUpdates().catch((error) => {
														notifyGlobalError(
															"Inbox 新内容显示失败",
															error,
															"Inbox 新内容显示失败，请稍后重试。",
														);
													});
												}}
											/>
										) : null}
										<InboxQuickList
											notifications={notifications}
											freshKeys={freshNotificationKeys}
										/>
									</div>
								) : null}
							</aside>
						) : null}
					</div>
				</Tabs>

				<ReleaseDetailCard
					target={activeReleaseTarget}
					onClose={onCloseReleaseDetail}
					onResolvedDetail={onReleaseDetailResolved}
				/>

				<Dialog
					open={patGuideOpen}
					onOpenChange={(open) => {
						if (open) {
							setPatGuideOpen(true);
							return;
						}
						closePatDialog();
					}}
				>
					<DialogContent className="max-w-md">
						<DialogHeader>
							<DialogTitle>配置 GitHub PAT</DialogTitle>
							<DialogDescription>
								不用跳走，直接在这里补齐就行。
							</DialogDescription>
						</DialogHeader>

						<div className="space-y-4">
							{patGuideMessage ? (
								<p className="text-sm text-foreground">{patGuideMessage}</p>
							) : null}

							<div className="space-y-2">
								<div className="flex items-center justify-between gap-3">
									<Label htmlFor="dashboard-reaction-pat">GitHub PAT</Label>
									{reactionTokenMasked ? (
										<span className="text-muted-foreground font-mono text-xs">
											已保存：{reactionTokenMasked}
										</span>
									) : null}
								</div>
								<GitHubPatInput
									id="dashboard-reaction-pat"
									value={patInput}
									onChange={(event) => setPatInput(event.target.value)}
									placeholder="粘贴 classic PAT"
									autoCapitalize="none"
									autoCorrect="off"
									spellCheck={false}
									autoFocus
									inputClassName="h-10 font-mono text-sm"
								/>
							</div>

							<div className="rounded-xl border border-border/70 bg-muted/20 px-3 py-2.5">
								<p className="text-sm font-medium">
									{patCheckState === "checking"
										? "正在校验 GitHub PAT"
										: patCheckState === "valid"
											? "GitHub PAT 可用"
											: patCheckState === "invalid"
												? "GitHub PAT 无效"
												: patCheckState === "error"
													? "GitHub PAT 校验失败"
													: reactionTokenConfigured === true
														? "已保存 GitHub PAT"
														: reactionTokenConfigured === false
															? "还没有可用的 GitHub PAT"
															: "正在读取 GitHub PAT 状态"}
								</p>
								<p className="text-muted-foreground mt-1 text-xs leading-5">
									{patCheckMessage ??
										"输入后会在 800ms 后自动校验；通过后才能保存。"}
								</p>
							</div>

							<div className="text-muted-foreground flex items-center justify-between gap-3 text-xs">
								<span>
									{patCheckedAt
										? `最近检查：${formatDateTime(patCheckedAt)}`
										: "需要 classic PAT"}
								</span>
								<Button
									asChild
									variant="ghost"
									size="sm"
									className="h-auto px-0 text-xs"
									onClick={closePatDialog}
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
							<Button variant="outline" onClick={closePatDialog}>
								取消
							</Button>
							<Button
								disabled={patSaving || !canSavePat}
								onClick={onSavePatFromDialog}
							>
								{patSaving ? "保存中…" : "保存 GitHub PAT"}
							</Button>
						</DialogFooter>
					</DialogContent>
				</Dialog>
			</div>
		</AppShell>
	);
}
