import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
	type DailyBriefProfilePatchRequest,
	type MeResponse,
	type MeProfileResponse,
	ApiError,
	apiGet,
	apiGetMeProfile,
	apiPatchMeProfile,
	apiPost,
	apiPostJson,
	apiPutJson,
} from "@/api";
import {
	persistDashboardWarmSnapshot,
	type DashboardWarmSnapshot,
} from "@/auth/startupCache";
import {
	DailyBriefProfileForm,
	readHourAlignedBrowserTimeZone,
} from "@/briefs/DailyBriefProfileForm";
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
import {
	DEFAULT_PAGE_LANE,
	isFeedLane,
	PAGE_DEFAULT_LANE_STORAGE_KEY,
	resolveDisplayLaneForFeed,
	resolvePreferredLaneForItem,
} from "@/feed/laneOptions";
import type {
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
import { normalizeReleaseId } from "@/lib/releaseId";
import {
	DashboardMobileControlBand,
	type DashboardTab as Tab,
	DashboardTabsList,
} from "@/pages/DashboardControlBand";
import { DashboardStartupSkeleton } from "@/pages/AppBoot";
import { DashboardHeader } from "@/pages/DashboardHeader";
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

function feedItemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

function resolveLaneForItem(
	item: FeedItem,
	selectedLaneByKey: Record<string, FeedLane>,
	pageDefaultLane: FeedLane,
): FeedLane {
	const selected = selectedLaneByKey[feedItemKey(item)];
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

type ReactionTokenStatusResponse = {
	configured: boolean;
	masked_token: string | null;
	check: {
		state: "idle" | "valid" | "invalid" | "error";
		message: string | null;
		checked_at: string | null;
	};
};

type ReactionTokenCheckResponse = {
	state: "valid" | "invalid";
	message: string;
};

const SYNC_ALL_LABEL = "同步";
const TASK_STREAM_RECOVERY_GRACE_MS = 5000;
const REACTION_CONTENTS: ReactionContent[] = [
	"plus1",
	"laugh",
	"heart",
	"hooray",
	"rocket",
	"eyes",
];

function sortNotifications(items: NotificationItem[]) {
	return items.slice().sort((a, b) => {
		if (a.unread !== b.unread) return b.unread - a.unread;
		const at = a.updated_at ?? "";
		const bt = b.updated_at ?? "";
		return bt.localeCompare(at);
	});
}

export type DashboardRouteState = {
	tab: Tab;
	activeReleaseId: string | null;
};

export function parseDashboardRouteState(search: {
	tab?: string | null;
	release?: string | null;
}): DashboardRouteState {
	const releaseId = normalizeReleaseId(search.release);
	if (releaseId) {
		return { tab: "briefs", activeReleaseId: releaseId };
	}

	const rawTab = search.tab;
	const tab: Tab =
		rawTab === "releases" ||
		rawTab === "stars" ||
		rawTab === "followers" ||
		rawTab === "briefs" ||
		rawTab === "inbox"
			? rawTab
			: "all";
	return { tab, activeReleaseId: null };
}

export function buildDashboardSearch(routeState: DashboardRouteState) {
	if (routeState.activeReleaseId) {
		return {
			tab: "briefs" as const,
			release: routeState.activeReleaseId,
		};
	}

	return {
		tab: routeState.tab === "all" ? undefined : routeState.tab,
		release: undefined,
	};
}

function parseDashboardQuery() {
	const params = new URLSearchParams(window.location.search);
	return parseDashboardRouteState({
		tab: params.get("tab"),
		release: params.get("release"),
	});
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
	const isRouteControlled = controlledRouteState !== undefined;
	const isAdmin = me.user.is_admin;
	const [dailyBoundaryLocal, setDailyBoundaryLocal] = useState(
		me.dashboard.daily_boundary_local,
	);
	const [dailyBoundaryTimeZone, setDailyBoundaryTimeZone] = useState(
		me.dashboard.daily_boundary_time_zone,
	);
	const [dailyBoundaryUtcOffsetMinutes, setDailyBoundaryUtcOffsetMinutes] =
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

	const [bootError, setBootError] = useState<string | null>(null);
	const [busy, setBusy] = useState<string | null>(null);
	const [hydrationSource] = useState<"warm-cache" | "network">(() =>
		warmStart ? "warm-cache" : "network",
	);
	const [accessTaskStream, setAccessTaskStream] =
		useState<TaskStreamState | null>(initialAccessTask);
	const [refreshTaskStreams, setRefreshTaskStreams] = useState<
		TaskStreamState[]
	>([]);
	const [accessSyncStage, setAccessSyncStage] = useState<
		"idle" | "waiting" | "star_refreshed" | "completed" | "failed"
	>(initialAccessTask ? "waiting" : "idle");
	const refreshTaskSourcesRef = useRef<Map<string, EventSource>>(new Map());
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
		() => warmStart?.selectedBriefId ?? null,
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
	const [reactionErrorByKey, setReactionErrorByKey] = useState<
		Record<string, string>
	>({});
	const [reactionTokenConfigured, setReactionTokenConfigured] =
		useState<boolean>(false);
	const [reactionTokenMasked, setReactionTokenMasked] = useState<string | null>(
		null,
	);
	const [patDialogOpen, setPatDialogOpen] = useState<boolean>(false);
	const [patInput, setPatInput] = useState<string>("");
	const [patCheckState, setPatCheckState] = useState<
		"idle" | "checking" | "valid" | "invalid"
	>("idle");
	const [patCheckMessage, setPatCheckMessage] = useState<string | null>(null);
	const [patSaving, setPatSaving] = useState<boolean>(false);
	const [pendingReaction, setPendingReaction] = useState<{
		item: FeedItem;
		content: ReactionContent;
	} | null>(null);
	const patCheckSeqRef = useRef(0);

	const [notifications, setNotifications] = useState<NotificationItem[]>(
		() => warmStart?.notifications ?? [],
	);
	const [briefs, setBriefs] = useState<BriefItem[]>(
		() => warmStart?.briefs ?? [],
	);
	const [sidebarLoading, setSidebarLoading] = useState(
		() => warmStart === null,
	);
	const [profileDialogOpen, setProfileDialogOpen] = useState(false);
	const [briefProfile, setBriefProfile] = useState<MeProfileResponse | null>(
		null,
	);
	const [briefProfileDraft, setBriefProfileDraft] =
		useState<DailyBriefProfilePatchRequest>({
			daily_brief_local_time: dailyBoundaryLocal,
			daily_brief_time_zone:
				dailyBoundaryTimeZone ??
				readHourAlignedBrowserTimeZone() ??
				"Asia/Shanghai",
		});
	const [briefProfileLoading, setBriefProfileLoading] = useState(false);
	const [briefProfileSaving, setBriefProfileSaving] = useState(false);
	const [briefProfileError, setBriefProfileError] = useState<string | null>(
		null,
	);

	const loadReactionTokenStatus = useCallback(async () => {
		const res = await apiGet<ReactionTokenStatusResponse>(
			"/api/reaction-token/status",
		);
		setReactionTokenConfigured(res.configured && res.check.state === "valid");
		setReactionTokenMasked(res.masked_token);
		if (!res.configured) {
			setPatCheckState("idle");
			setPatCheckMessage(null);
		}
	}, []);

	const refreshSidebar = useCallback(
		async (options?: { background?: boolean }) => {
			if (!options?.background) {
				setSidebarLoading(true);
			}
			try {
				const [n, b] = await Promise.all([
					apiGet<NotificationItem[]>("/api/notifications"),
					apiGet<BriefItem[]>("/api/briefs"),
				]);
				setNotifications(sortNotifications(n));
				setBriefs(b);
				setSelectedBriefId((prev) => {
					if (prev && b.some((x) => x.id === prev)) return prev;
					return b[0]?.id ?? null;
				});
			} finally {
				setSidebarLoading(false);
			}
		},
		[],
	);
	const refreshDashboardBoundary = useCallback(async () => {
		const latestMe = await apiGet<MeResponse>("/api/me");
		setDailyBoundaryLocal(latestMe.dashboard.daily_boundary_local);
		setDailyBoundaryTimeZone(latestMe.dashboard.daily_boundary_time_zone);
		setDailyBoundaryUtcOffsetMinutes(
			latestMe.dashboard.daily_boundary_utc_offset_minutes,
		);
	}, []);
	const loadBriefProfile = useCallback(async () => {
		setBriefProfileLoading(true);
		setBriefProfileError(null);
		try {
			const profile = await apiGetMeProfile();
			setBriefProfile(profile);
			setBriefProfileDraft({
				daily_brief_local_time: profile.daily_brief_local_time,
				daily_brief_time_zone: profile.daily_brief_time_zone,
			});
		} catch (err) {
			setBriefProfileError(err instanceof Error ? err.message : String(err));
		} finally {
			setBriefProfileLoading(false);
		}
	}, []);

	const refreshAll = useCallback(async () => {
		setBootError(null);
		await Promise.all([refreshFeed(), refreshSidebar()]);
	}, [refreshFeed, refreshSidebar]);

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

	const run = useCallback(async <T,>(label: string, fn: () => Promise<T>) => {
		setBusy(label);
		setBootError(null);
		try {
			return await fn();
		} catch (err) {
			setBootError(err instanceof Error ? err.message : String(err));
			throw err;
		} finally {
			setBusy(null);
		}
	}, []);

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
		void refreshSidebar({ background: warmStart !== null }).catch((err) => {
			setBootError(err instanceof Error ? err.message : String(err));
		});
		void loadReactionTokenStatus().catch((err) => {
			setBootError(err instanceof Error ? err.message : String(err));
		});
	}, [loadInitialFeed, loadReactionTokenStatus, refreshSidebar, warmStart]);

	useEffect(() => {
		window.localStorage.setItem(PAGE_DEFAULT_LANE_STORAGE_KEY, pageDefaultLane);
	}, [pageDefaultLane]);

	useEffect(() => {
		if (tab !== "all" && tab !== "releases") {
			return;
		}
		if (feed.loadingInitial || feed.items.length === 0) {
			return;
		}
		void primeSmart(feed.items).catch((err) => {
			setBootError(err instanceof Error ? err.message : String(err));
		});
	}, [feed.items, feed.loadingInitial, primeSmart, tab]);

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
			void refreshAll().catch((err) => {
				setBootError(err instanceof Error ? err.message : String(err));
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
			setBootError(message);
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
				refreshOnUi();
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
				if (payload.status === "succeeded") {
					try {
						await refreshAll();
					} catch (err) {
						const error = err instanceof Error ? err : new Error(String(err));
						setBootError(error.message);
						source.close();
						settleTaskWaiter(completedTaskId, error);
						setAccessTaskStream((current) =>
							current?.taskId === completedTaskId ? null : current,
						);
						return;
					}
				} else if (payload.error) {
					setBootError(payload.error);
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
	}, [accessTaskStream, refreshAll, settleTaskWaiter]);

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
				setBootError(message);
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
						} catch (err) {
							const error = err instanceof Error ? err : new Error(String(err));
							setBootError(error.message);
							settleTaskWaiter(completedTaskId, error);
							close();
							return;
						}
					} else if (payload.error) {
						setBootError(payload.error);
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
	}, [refreshTaskStreams, refreshAll, settleTaskWaiter]);

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
			void translateNow(item).catch((err) => {
				setBootError(err instanceof Error ? err.message : String(err));
			});
		},
		[translateNow],
	);
	const onSmartNow = useCallback(
		(item: FeedItem) => {
			void smartNow(item).catch((err) => {
				setBootError(err instanceof Error ? err.message : String(err));
			});
		},
		[smartNow],
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
				void translateNow(item).catch((err) => {
					setBootError(err instanceof Error ? err.message : String(err));
				});
			}
			if (
				lane === "smart" &&
				(item.smart?.status === "missing" ||
					(item.smart?.status === "error" &&
						item.smart?.auto_translate !== false))
			) {
				void smartNow(item).catch((err) => {
					setBootError(err instanceof Error ? err.message : String(err));
				});
			}
		},
		[smartNow, translateNow],
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
					setPatCheckState("valid");
					setPatCheckMessage("PAT 可用");
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
							const retryItem = feed.items.find((it) => itemKey(it) === key);
							if (retryItem) {
								setPendingReaction({ item: retryItem, content });
							}
							setPatDialogOpen(true);
							setPatCheckState(err.code === "pat_invalid" ? "invalid" : "idle");
							setPatCheckMessage(
								err.code === "pat_invalid"
									? "PAT 无效或已过期，请重新填写并校验。"
									: "需要先配置 PAT 才能使用站内反馈。",
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
		[feed],
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
			if (!reactionTokenConfigured) {
				setPendingReaction({ item, content });
				setPatDialogOpen(true);
				setPatCheckState("idle");
				setPatCheckMessage("需要先配置 PAT 才能使用站内反馈。");
				return;
			}
			performReactionToggle(item, content);
		},
		[performReactionToggle, reactionTokenConfigured],
	);

	useEffect(() => {
		if (!patDialogOpen) return;
		const token = patInput.trim();
		patCheckSeqRef.current += 1;
		const seq = patCheckSeqRef.current;
		if (!token) {
			setPatCheckState("idle");
			setPatCheckMessage(null);
			return;
		}

		setPatCheckState("checking");
		setPatCheckMessage("正在检查 PAT 可用性…");
		const timer = window.setTimeout(() => {
			void apiPostJson<ReactionTokenCheckResponse>(
				"/api/reaction-token/check",
				{
					token,
				},
			)
				.then((res) => {
					if (seq !== patCheckSeqRef.current) return;
					setPatCheckState(res.state);
					setPatCheckMessage(res.message);
				})
				.catch((err) => {
					if (seq !== patCheckSeqRef.current) return;
					if (err instanceof ApiError && err.status === 401) {
						setPatCheckState("invalid");
						setPatCheckMessage(sessionExpiredHint());
						return;
					}
					const message = err instanceof Error ? err.message : String(err);
					setPatCheckState("invalid");
					setPatCheckMessage(message);
				});
		}, 800);

		return () => window.clearTimeout(timer);
	}, [patDialogOpen, patInput]);

	useEffect(
		() => () => {
			for (const timer of reactionFlushTimerByKeyRef.current.values()) {
				window.clearTimeout(timer);
			}
			reactionFlushTimerByKeyRef.current.clear();
		},
		[],
	);

	const onSavePat = useCallback(() => {
		if (patCheckState !== "valid") return;
		const token = patInput.trim();
		if (!token) return;

		setPatSaving(true);
		void apiPutJson<ReactionTokenStatusResponse>("/api/reaction-token", {
			token,
		})
			.then((res) => {
				setReactionTokenConfigured(res.configured);
				setReactionTokenMasked(res.masked_token);
				setPatDialogOpen(false);
				setPatInput("");
				setPatCheckState("idle");
				setPatCheckMessage(null);
				if (pendingReaction) {
					const next = pendingReaction;
					setPendingReaction(null);
					performReactionToggle(next.item, next.content);
				}
			})
			.catch((err) => {
				if (err instanceof ApiError && err.status === 401) {
					setPatCheckState("invalid");
					setPatCheckMessage(sessionExpiredHint());
					return;
				}
				const message = err instanceof Error ? err.message : String(err);
				setPatCheckState("invalid");
				setPatCheckMessage(message);
			})
			.finally(() => {
				setPatSaving(false);
			});
	}, [patCheckState, patInput, pendingReaction, performReactionToggle]);

	const onGenerateBrief = useCallback(() => {
		void run("Generate brief", async () => {
			await apiPost<BriefGenerateResponse>("/api/briefs/generate");
			await refreshSidebar();
		});
	}, [refreshSidebar, run]);
	const onGenerateBriefForDate = useCallback(
		async (date: string) => {
			setBootError(null);
			await apiPostJson<BriefGenerateResponse>("/api/briefs/generate", {
				date,
			});
			await refreshSidebar();
		},
		[refreshSidebar],
	);
	const onOpenProfileDialog = useCallback(() => {
		setProfileDialogOpen(true);
		void loadBriefProfile();
	}, [loadBriefProfile]);
	const onSaveBriefProfile = useCallback(() => {
		if (!briefProfile) return;
		setBriefProfileSaving(true);
		setBriefProfileError(null);
		void apiPatchMeProfile(briefProfileDraft)
			.then(async (profile) => {
				setBriefProfile(profile);
				setBriefProfileDraft({
					daily_brief_local_time: profile.daily_brief_local_time,
					daily_brief_time_zone: profile.daily_brief_time_zone,
				});
				await refreshDashboardBoundary();
				setProfileDialogOpen(false);
			})
			.catch((err) => {
				setBriefProfileError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				setBriefProfileSaving(false);
			});
	}, [briefProfile, briefProfileDraft, refreshDashboardBoundary]);
	const onSyncInbox = useCallback(() => {
		void run("Sync inbox", async () => {
			const task = await apiPost<TaskAcceptedResponse>(
				"/api/sync/notifications?return_mode=task_id",
			);
			await trackTaskStream(task, "refresh");
		});
	}, [run, trackTaskStream]);

	const onSyncAll = useCallback(() => {
		void run(SYNC_ALL_LABEL, async () => {
			const task = await apiPost<TaskAcceptedResponse>(
				"/api/sync/all?return_mode=task_id",
			);
			await trackTaskStream(task, "access");
		});
	}, [run, trackTaskStream]);
	const syncingAll = busy === SYNC_ALL_LABEL;
	const syncingInbox = busy === "Sync inbox";

	const aiDisabledHint = useMemo(() => {
		const any = feed.items.find(
			(it) =>
				it.translated?.status === "disabled" || it.smart?.status === "disabled",
		);
		return Boolean(any);
	}, [feed.items]);

	const resetPatDialogState = useCallback(() => {
		setPatDialogOpen(false);
		setPatInput("");
		setPatCheckState("idle");
		setPatCheckMessage(null);
	}, []);

	const onPatDialogOpenChange = useCallback(
		(open: boolean) => {
			if (open) {
				setPatDialogOpen(true);
				return;
			}
			resetPatDialogState();
		},
		[resetPatDialogState],
	);

	const onSelectTab = useCallback(
		(nextTab: Tab) => {
			setRouteState({
				tab: nextTab,
				activeReleaseId: nextTab === "briefs" ? activeReleaseId : null,
			});
		},
		[activeReleaseId, setRouteState],
	);

	const onOpenReleaseDetail = useCallback(
		(releaseId: string) => {
			setRouteState({
				tab: "briefs",
				activeReleaseId: releaseId,
			});
		},
		[setRouteState],
	);

	const onCloseReleaseDetail = useCallback(() => {
		setRouteState(
			{
				tab,
				activeReleaseId: null,
			},
			{ replace: true },
		);
	}, [setRouteState, tab]);
	const showPageLaneSelector = tab === "all" || tab === "releases";

	const renderFeedPanel = (
		mode: "all" | "releases" | "stars" | "followers",
	) => {
		const filteredItems = filterFeedItemsForTab(feed.items, mode);
		return (
			<>
				{!feed.loadingInitial && filteredItems.length === 0 ? (
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
					selectedLaneByKey={Object.fromEntries(
						filteredItems.map((item) => [
							feedItemKey(item),
							resolveLaneForItem(item, selectedLaneByKey, pageDefaultLane),
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
		const params = new URLSearchParams(window.location.search);
		if (tab === "all") {
			params.delete("tab");
		} else {
			params.set("tab", tab);
		}
		if (activeReleaseId) {
			params.set("release", activeReleaseId);
		} else {
			params.delete("release");
		}
		const query = params.toString();
		const nextUrl = `${window.location.pathname}${query ? `?${query}` : ""}`;
		if (nextUrl !== `${window.location.pathname}${window.location.search}`) {
			window.history.replaceState({}, "", nextUrl);
		}
	}, [isRouteControlled, tab, activeReleaseId]);

	useEffect(() => {
		if (tab !== "all" && tab !== "releases") {
			return;
		}
		if (feed.loadingInitial || feed.items.length === 0) {
			return;
		}
		for (const item of feed.items) {
			if (selectedLaneByKey[feedItemKey(item)]) {
				continue;
			}
			requestLaneIfNeeded(
				item,
				resolvePreferredLaneForItem(item, pageDefaultLane),
			);
		}
	}, [
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
		persistDashboardWarmSnapshot({
			userId: me.user.id,
			routeState: {
				tab,
				activeReleaseId,
			},
			feedRequestType,
			feedItems: feed.items,
			nextCursor: feed.nextCursor,
			notifications,
			briefs,
			selectedBriefId,
		});
	}, [
		activeReleaseId,
		briefs,
		feed.items,
		feed.loadingInitial,
		feed.nextCursor,
		feedRequestType,
		me.user.id,
		notifications,
		selectedBriefId,
		sidebarLoading,
		tab,
	]);

	const showStartupSkeleton =
		warmStart === null && (feed.loadingInitial || sidebarLoading);

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
				{bootError ? (
					<p className="text-destructive mb-4 text-sm">{bootError}</p>
				) : null}

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
							<Button
								variant="outline"
								size="sm"
								className="font-mono text-xs"
								onClick={onOpenProfileDialog}
							>
								日报设置
							</Button>
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
									briefs={briefs}
									selectedId={selectedBriefId}
									busy={busy === "Generate brief"}
									onGenerate={onGenerateBrief}
									onOpenRelease={onOpenReleaseDetail}
								/>
							</TabsContent>
							<TabsContent value="inbox" className="mt-0 min-w-0">
								<InboxList
									notifications={notifications}
									busy={Boolean(busy)}
									syncing={syncingInbox}
									onSync={tab === "inbox" ? onSyncInbox : undefined}
								/>
							</TabsContent>
						</section>

						<aside className="space-y-4 sm:space-y-6">
							{tab === "briefs" ? (
								<BriefListCard
									briefs={briefs}
									selectedId={selectedBriefId}
									onSelectId={(id) => setSelectedBriefId(id)}
								/>
							) : null}
							<InboxQuickList notifications={notifications} />
						</aside>
					</div>
				</Tabs>

				<Dialog
					open={profileDialogOpen}
					onOpenChange={(open) => {
						setProfileDialogOpen(open);
						if (!open) {
							setBriefProfileError(null);
						}
					}}
				>
					<DialogContent className="max-w-lg">
						<DialogHeader>
							<DialogTitle>日报设置</DialogTitle>
							<DialogDescription>
								之后新生成的日报会按这里的“本地整点 + IANA
								时区”切窗口；历史快照不会被回写。
							</DialogDescription>
						</DialogHeader>
						<DailyBriefProfileForm
							localTime={briefProfileDraft.daily_brief_local_time}
							timeZone={briefProfileDraft.daily_brief_time_zone}
							disabled={briefProfileLoading || briefProfileSaving}
							error={briefProfileError}
							helperText={
								briefProfile?.last_active_at
									? `最近活动：${new Date(briefProfile.last_active_at).toLocaleString()}`
									: "保存时会严格校验 IANA 时区；非法值不会降级成 offset。"
							}
							onLocalTimeChange={(value) =>
								setBriefProfileDraft((current) => ({
									...current,
									daily_brief_local_time: value,
								}))
							}
							onTimeZoneChange={(value) =>
								setBriefProfileDraft((current) => ({
									...current,
									daily_brief_time_zone: value,
								}))
							}
							onUseBrowserTimeZone={(timeZone) =>
								setBriefProfileDraft((current) => ({
									...current,
									daily_brief_time_zone: timeZone,
								}))
							}
						/>
						<DialogFooter>
							<Button
								variant="outline"
								onClick={() => setProfileDialogOpen(false)}
								disabled={briefProfileSaving}
							>
								取消
							</Button>
							<Button
								onClick={onSaveBriefProfile}
								disabled={
									briefProfileLoading || briefProfileSaving || !briefProfile
								}
							>
								{briefProfileSaving ? "保存中…" : "保存设置"}
							</Button>
						</DialogFooter>
					</DialogContent>
				</Dialog>

				<ReleaseDetailCard
					releaseId={activeReleaseId}
					onClose={onCloseReleaseDetail}
				/>

				<Dialog open={patDialogOpen} onOpenChange={onPatDialogOpenChange}>
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
							<p className="text-muted-foreground mt-2 text-xs">
								最小权限：公共仓库用{" "}
								<span className="font-mono">public_repo</span>； 私有仓库用{" "}
								<span className="font-mono">repo</span>。
							</p>
							{reactionTokenMasked ? (
								<p className="text-muted-foreground mt-2 text-xs">
									当前已保存：
									<span className="font-mono">{reactionTokenMasked}</span>
								</p>
							) : null}
						</div>

						<div className="space-y-2">
							<Label htmlFor="reaction-pat">GitHub PAT</Label>
							<Input
								id="reaction-pat"
								type="password"
								value={patInput}
								onChange={(event) => setPatInput(event.target.value)}
								placeholder="粘贴 PAT 后将自动校验（800ms 防抖）"
								className="font-mono text-sm"
							/>
						</div>

						<p
							className={
								patCheckState === "valid"
									? "text-xs text-emerald-600"
									: patCheckState === "invalid"
										? "text-xs text-red-600"
										: "text-muted-foreground text-xs"
							}
						>
							{patCheckMessage ??
								"输入后会自动检查 PAT 是否可用；仅最后一次输入结果生效。"}
						</p>

						<DialogFooter>
							<Button variant="outline" onClick={resetPatDialogState}>
								稍后再说
							</Button>
							<Button
								onClick={onSavePat}
								disabled={patSaving || patCheckState !== "valid"}
							>
								{patSaving ? "保存中…" : "保存并继续"}
							</Button>
						</DialogFooter>
					</DialogContent>
				</Dialog>
			</div>
		</AppShell>
	);
}
