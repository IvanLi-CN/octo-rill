import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { type MeResponse, ApiError, apiGet, apiPost, apiPostJson } from "@/api";
import {
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
	ReactionContent,
	ReleaseReactions,
	ToggleReleaseReactionResponse,
} from "@/feed/types";
import { isReleaseFeedItem } from "@/feed/types";
import { useAutoSmart } from "@/feed/useAutoSmart";
import { useAutoTranslate } from "@/feed/useAutoTranslate";
import { type FeedRequestType, useFeed } from "@/feed/useFeed";
import { InboxList } from "@/inbox/InboxList";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";
import { InternalLink } from "@/lib/internalNavigation";
import { describeUnknownError } from "@/lib/errorPresentation";
import { useMediaQuery } from "@/lib/useMediaQuery";
import {
	buildDashboardReleaseTarget,
	buildDashboardRouteUrl,
	buildDashboardWarmRouteState,
	parseDashboardRouteStateFromLocation,
	releaseLocatorFromReleaseDetail,
	type DashboardReleaseTarget,
	type DashboardRouteState,
} from "@/dashboard/routeState";
import {
	DashboardMobileControlBand,
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

function feedItemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
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
) {
	switch (tab) {
		case "releases":
			return items.filter((item) => item.kind === "release");
		case "stars":
			return items.filter((item) => item.kind === "repo_star_received");
		case "followers":
			return items.filter((item) => item.kind === "follower_received");
		default:
			return items;
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
		| "star_refreshed"
		| "release_summary"
		| "social_summary"
		| "notifications_summary",
	payload: TaskEventPayload = {},
): DashboardSyncProgress {
	switch (stage) {
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
}) {
	const {
		me,
		routeState: controlledRouteState,
		onRouteStateChange,
		warmStart = null,
	} = props;
	const { pushErrorToast, pushToast } = useAppToast();
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
	const sessionState = dashboardSessionStateByUser.get(me.user.id) ?? null;

	const [busy, setBusy] = useState<string | null>(null);
	const [hydrationSource] = useState<"warm-cache" | "network">(() =>
		warmStart ? "warm-cache" : "network",
	);
	const [bootedFromWarmStart] = useState(
		() => warmStart !== null || sessionState?.shellHydrated === true,
	);
	const [shellHydrated, setShellHydrated] = useState(
		() => warmStart !== null || sessionState?.shellHydrated === true,
	);
	const [accessTaskStream, setAccessTaskStream] =
		useState<TaskStreamState | null>(initialAccessTask);
	const [refreshTaskStreams, setRefreshTaskStreams] = useState<
		TaskStreamState[]
	>([]);
	const [accessSyncStage, setAccessSyncStage] = useState<
		"idle" | "waiting" | "star_refreshed" | "completed" | "failed"
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
		useState<DashboardRouteState>(() => parseDashboardQuery());
	const routeState = controlledRouteState ?? uncontrolledRouteState;
	const tab = routeState.tab;
	const activeReleaseId = routeState.activeReleaseId;
	const activeReleaseLocator = routeState.activeReleaseLocator;
	const releaseReturnTab = routeState.releaseReturnTab;
	const activeReleaseTarget = useMemo(
		() =>
			activeReleaseId || activeReleaseLocator
				? buildDashboardReleaseTarget({
						releaseId: activeReleaseId,
						locator: activeReleaseLocator,
						fromTab: releaseReturnTab,
					})
				: null,
		[activeReleaseId, activeReleaseLocator, releaseReturnTab],
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
				}
			: null;
	const feed = useFeed(feedRequestType, {
		initialData: warmFeedData,
	});
	const loadInitialFeed = feed.loadInitial;
	const refreshFeed = feed.refresh;

	const [selectedLaneByKey, setSelectedLaneByKey] = useState<
		Record<string, FeedLane>
	>({});
	const [pageDefaultLane, setPageDefaultLane] = useState<FeedLane>(
		readStoredPageDefaultLane,
	);
	const effectivePageDefaultLane = useMemo(
		() => resolveDisplayLaneForFeed(feed.items, pageDefaultLane),
		[feed.items, pageDefaultLane],
	);
	const [selectedBriefId, setSelectedBriefId] = useState<string | null>(
		() => sessionState?.selectedBriefId ?? warmStart?.selectedBriefId ?? null,
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
	>(() => sessionState?.reactionTokenConfigured ?? null);
	const [patGuideOpen, setPatGuideOpen] = useState<boolean>(false);
	const [patGuideMessage, setPatGuideMessage] = useState<string | null>(null);
	const pendingReactionRef = useRef<{
		releaseId: string;
		content: ReactionContent;
	} | null>(null);
	const handleReactionTokenStatusLoaded = useCallback(
		(status: Parameters<typeof isReactionTokenUsable>[0]) => {
			setReactionTokenConfigured(isReactionTokenUsable(status));
		},
		[],
	);
	const handleReactionTokenSaved = useCallback(
		(status: Parameters<typeof isReactionTokenUsable>[0]) => {
			setReactionTokenConfigured(isReactionTokenUsable(status));
		},
		[],
	);
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
		onStatusLoaded: handleReactionTokenStatusLoaded,
		onPatSaved: handleReactionTokenSaved,
	});

	const [notifications, setNotifications] = useState<NotificationItem[]>(
		() => sessionState?.notifications ?? warmStart?.notifications ?? [],
	);
	const [briefs, setBriefs] = useState<BriefItem[]>(
		() => sessionState?.briefs ?? warmStart?.briefs ?? [],
	);
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
	const sidebarBootstrapCompletedRef = useRef(
		sessionState?.sidebarBootstrapped ?? false,
	);
	const notificationsBootstrapCompletedRef = useRef(
		sessionState?.notificationsBootstrapped ?? false,
	);
	const reactionTokenBootstrapCompletedRef = useRef(
		sessionState?.reactionTokenBootstrapped ?? false,
	);
	const startupBootstrapRequestedRef = useRef(
		sidebarBootstrapCompletedRef.current,
	);
	const startupSidebarRetriedRef = useRef(false);
	const notificationsBootstrapRequestedRef = useRef(
		notificationsBootstrapCompletedRef.current ||
			(initialNotificationBootstrapRef.current &&
				!startupBootstrapRequestedRef.current),
	);
	const reactionTokenBootstrapRequestedRef = useRef(
		reactionTokenBootstrapCompletedRef.current,
	);
	const notificationsRequestInFlightRef = useRef(false);
	const [sidebarLoading, setSidebarLoading] = useState(
		() => !bootedFromWarmStart,
	);
	const [notificationsLoading, setNotificationsLoading] = useState(false);
	const notifyGlobalError = useCallback(
		(
			title: string,
			error: unknown,
			fallback: string,
			options?: {
				actionLabel?: string;
				onAction?: () => void;
				detail?: string | null;
			},
		) => {
			pushErrorToast(title, describeUnknownError(error, fallback), {
				actionLabel: options?.actionLabel,
				onAction: options?.onAction,
				detail:
					options?.detail ?? (error instanceof Error ? error.message : null),
			});
		},
		[pushErrorToast],
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
				setBriefs(b);
				setSelectedBriefId((prev) => {
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
		[briefs.length, loadNotifications, notifyGlobalError],
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

	const refreshAll = useCallback(async () => {
		await Promise.all([
			refreshFeed(),
			refreshSidebar({
				includeNotifications: hasDesktopSidebarInbox || tab === "inbox",
			}),
		]);
	}, [hasDesktopSidebarInbox, refreshFeed, refreshSidebar, tab]);

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
		translateNow,
		inFlightKeys: translationInFlightKeys,
	} = useAutoTranslate({
		enabled: true,
		onTranslated: feed.applyTranslation,
	});
	const {
		prime: primeSmart,
		register: registerSmart,
		smartNow,
		inFlightKeys: smartInFlightKeys,
	} = useAutoSmart({
		enabled: true,
		onSmart: feed.applySmart,
	});

	useEffect(() => {
		void loadInitialFeed();
	}, [loadInitialFeed]);

	useEffect(() => {
		if (startupBootstrapRequestedRef.current) {
			return;
		}
		const startSidebarBootstrap = (allowRetry: boolean) => {
			startupBootstrapRequestedRef.current = true;
			void refreshSidebar({
				background: bootedFromWarmStart,
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
	}, [bootedFromWarmStart, refreshSidebar]);

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
		const releaseIds = feed.items
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
						feed.applyReactions(
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
		feed.applyReactions,
		feed.items,
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
		if (!accessTaskStream) return;

		const source = new EventSource(accessTaskStream.eventPath);
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
			source.removeEventListener("task.progress", onProgress);
			source.removeEventListener("task.completed", onCompleted);
			source.close();
		};
	}, [
		accessTaskStream,
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

			const source = new EventSource(task.eventPath);
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
				notifyGlobalError("翻译触发失败", error, "翻译触发失败，请稍后重试。");
			});
		},
		[notifyGlobalError, translateNow],
	);
	const onSmartNow = useCallback(
		(item: FeedItem) => {
			void smartNow(item).catch((error) => {
				notifyGlobalError("润色触发失败", error, "润色触发失败，请稍后重试。");
			});
		},
		[notifyGlobalError, smartNow],
	);
	const requestLaneIfNeeded = useCallback(
		(item: FeedItem, lane: FeedLane) => {
			if (!isReleaseFeedItem(item)) {
				return;
			}
			if (
				lane === "translated" &&
				(item.translated?.status === "missing" ||
					(item.translated?.status === "error" &&
						item.translated?.auto_translate !== false))
			) {
				void translateNow(item).catch((error) => {
					notifyGlobalError(
						"翻译触发失败",
						error,
						"翻译触发失败，请稍后重试。",
					);
				});
			}
			if (
				lane === "smart" &&
				(item.smart?.status === "missing" ||
					(item.smart?.status === "error" &&
						item.smart?.auto_translate !== false))
			) {
				void smartNow(item).catch((error) => {
					notifyGlobalError(
						"润色触发失败",
						error,
						"润色触发失败，请稍后重试。",
					);
				});
			}
		},
		[notifyGlobalError, smartNow, translateNow],
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
			if (!isReleaseFeedItem(item)) {
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
						feed.applyReactions(item, res.reactions);
						reactionDesiredByKeyRef.current.delete(key);
					}
				})
				.catch((err) => {
					const stable = reactionServerByKeyRef.current.get(key);
					if (stable) {
						reactionDesiredByKeyRef.current.set(key, stable);
						feed.applyReactions(item, stable);
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
		[feed, openPatDialog],
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
			feed.applyReactions(item, optimistic);

			setReactionErrorByKey((prev) => {
				if (!(key in prev)) return prev;
				const next = { ...prev };
				delete next[key];
				return next;
			});
			scheduleReactionFlush(key);
		},
		[feed, scheduleReactionFlush],
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
			},
			{
				errorTitle: "日报生成失败",
				fallback: "日报生成失败，请稍后重试。",
			},
		);
	}, [refreshSidebar, run]);
	const onGenerateBriefForDate = useCallback(
		async (date: string) => {
			try {
				await apiPostJson<BriefGenerateResponse>("/api/briefs/generate", {
					date,
				});
				await refreshSidebar();
			} catch (error) {
				notifyGlobalError("日报生成失败", error, "日报生成失败，请稍后重试。");
				throw error;
			}
		},
		[notifyGlobalError, refreshSidebar],
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
			pushToast({
				title: "后台同步正在进行",
				description:
					"系统正在同步你的 GitHub 数据，可以悬浮在同步按钮上查看进度。",
				duration: 3200,
			});
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
	}, [accessSyncRunning, busy, pushToast, run, trackTaskStream]);
	const syncingAll = busy === SYNC_ALL_LABEL || accessSyncRunning;
	const syncingInbox = busy === "Sync inbox";

	const aiDisabledHint = useMemo(() => {
		const any = feed.items.find(
			(it) =>
				it.translated?.status === "disabled" || it.smart?.status === "disabled",
		);
		return Boolean(any);
	}, [feed.items]);

	const onSelectTab = useCallback(
		(nextTab: Tab) => {
			setRouteState({
				tab: nextTab,
				activeReleaseId: null,
				activeReleaseLocator: null,
				releaseReturnTab: "briefs",
			});
		},
		[setRouteState],
	);

	const onOpenReleaseDetail = useCallback(
		(target: DashboardReleaseTarget) => {
			setRouteState({
				tab: target.fromTab,
				activeReleaseId: target.releaseId,
				activeReleaseLocator: target.locator,
				releaseReturnTab: target.fromTab,
			});
		},
		[setRouteState],
	);

	const onCloseReleaseDetail = useCallback(() => {
		setRouteState(
			{
				tab: releaseReturnTab,
				activeReleaseId: null,
				activeReleaseLocator: null,
				releaseReturnTab,
			},
			{ replace: true },
		);
	}, [releaseReturnTab, setRouteState]);

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
					activeReleaseId: detail.release_id,
					activeReleaseLocator: locator,
					releaseReturnTab,
				},
				{ replace: true },
			);
		},
		[activeReleaseLocator, releaseReturnTab, setRouteState, tab],
	);
	const showPageLaneSelector = tab === "all" || tab === "releases";
	const renderSidebarInbox = hasDesktopSidebarInbox;
	const renderSidebar =
		(tab === "briefs" && hasTabletSidebar) || renderSidebarInbox;
	const dashboardContentLayoutClassName = renderSidebar
		? "grid gap-4 md:grid-cols-[minmax(0,1fr)_360px] md:gap-6"
		: "grid gap-4 md:gap-6";

	const renderFeedPanel = (
		mode: "all" | "releases" | "stars" | "followers",
	) => {
		const filteredItems = filterFeedItemsForTab(feed.items, mode);
		const blockingFeedError =
			feed.error?.phase === "initial" && filteredItems.length === 0;
		return (
			<>
				{!blockingFeedError &&
				!feed.loadingInitial &&
				filteredItems.length === 0 ? (
					<div className="bg-card/70 mb-4 rounded-xl border p-6 shadow-sm">
						{accessSyncStage === "waiting" ||
						accessSyncStage === "star_refreshed" ? (
							<>
								<h2 className="text-base font-semibold tracking-tight">
									正在同步你的 GitHub 动态
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
									可以先同步一次，把 release、被加星和被关注记录都拉下来喵。
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

				<FeedGroupedList
					mode={mode}
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
					error={feed.error}
					loadingInitial={feed.loadingInitial}
					loadingMore={feed.loadingMore}
					hasMore={feed.hasMore}
					translationInFlightKeys={translationInFlightKeys}
					smartInFlightKeys={smartInFlightKeys}
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
					onToggleReaction={onToggleReaction}
					onOpenReleaseFromBrief={
						mode === "all" ? onOpenReleaseDetail : undefined
					}
					onGenerateBriefForDate={
						mode === "all" ? onGenerateBriefForDate : undefined
					}
				/>
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
			notifications,
			briefs,
			selectedBriefId,
			shellHydrated,
			sidebarBootstrapped: sidebarBootstrapCompletedRef.current,
			notificationsBootstrapped: notificationsBootstrapCompletedRef.current,
			reactionTokenBootstrapped: reactionTokenBootstrapCompletedRef.current,
			reactionTokenConfigured,
		});
	}, [
		briefs,
		me.user.id,
		notifications,
		reactionTokenConfigured,
		selectedBriefId,
		shellHydrated,
	]);

	useEffect(() => {
		if (feed.loadingInitial || sidebarLoading) {
			return;
		}
		persistDashboardWarmSnapshot({
			userId: me.user.id,
			routeState: buildDashboardWarmRouteState(routeState),
			feedRequestType,
			feedItems: feed.items,
			nextCursor: feed.nextCursor,
			notifications,
			briefs,
			selectedBriefId,
		});
	}, [
		briefs,
		feed.items,
		feed.loadingInitial,
		feed.nextCursor,
		me.user.id,
		notifications,
		routeState,
		selectedBriefId,
		sidebarLoading,
		feedRequestType,
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
					mobileControlBand={
						<DashboardMobileControlBand
							tab={tab}
							onSelectTab={(nextTab) => onSelectTab(nextTab)}
							showPageLaneSelector={showPageLaneSelector}
							pageLane={effectivePageDefaultLane}
							onSelectPageLane={onSelectPageDefaultLane}
							layout="stacked"
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
						<DashboardTabsList />

						<div
							className="flex items-center gap-2"
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
								<span className="text-muted-foreground font-mono text-xs">
									AI 未配置，将只显示原文
								</span>
							) : null}
							{busy ? (
								<span className="text-muted-foreground font-mono text-xs">
									{busy}…
								</span>
							) : null}
							{isAdmin ? (
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
							) : null}
						</div>
					</div>

					<div className={dashboardContentLayoutClassName}>
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
									briefs={briefs}
									selectedId={selectedBriefId}
									busy={busy === "Generate brief"}
									error={
										briefsError?.phase === "initial"
											? briefsError.message
											: null
									}
									onGenerate={onGenerateBrief}
									onRetry={() =>
										void refreshSidebar({
											includeNotifications:
												hasDesktopSidebarInbox || tab === "inbox",
										})
									}
									onOpenRelease={onOpenReleaseDetail}
								/>
							</TabsContent>
							<TabsContent value="inbox" className="mt-0 min-w-0">
								<InboxList
									notifications={notifications}
									loading={notificationsLoading}
									busy={Boolean(busy)}
									syncing={syncingInbox}
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
						</section>

						{renderSidebar ? (
							<aside className="space-y-4 sm:space-y-6">
								{tab === "briefs" ? (
									<BriefListCard
										briefs={briefs}
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
