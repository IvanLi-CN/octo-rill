import { useCallback, useEffect, useRef } from "react";

import { apiGet, type DashboardUpdatesResponse } from "@/api";
import type { FeedRequestType } from "@/feed/useFeed";

const FOREGROUND_POLL_MS = 30_000;
const BACKGROUND_POLL_MS = 120_000;
const MAX_ERROR_BACKOFF_MS = 5 * 60_000;

export type DashboardLiveUpdateNotice = {
	list: "feed" | "briefs" | "notifications";
	newCount: number;
	latestKeys: string[];
	feedType?: FeedRequestType;
};
type DashboardLiveUpdateList = DashboardLiveUpdateNotice["list"];
type DashboardLiveUpdateCheckOptions = {
	emit?: boolean;
	include?: DashboardLiveUpdateList[];
};

function buildIncludeParam(options: {
	includeNotifications: boolean;
	includeBriefs: boolean;
}) {
	const parts = ["feed"];
	if (options.includeBriefs) parts.push("briefs");
	if (options.includeNotifications) parts.push("notifications");
	return parts.join(",");
}

function includeParamForLists(lists: DashboardLiveUpdateList[]) {
	return Array.from(new Set(lists)).join(",");
}

function mergeQueuedCheckOptions(
	current: DashboardLiveUpdateCheckOptions | null,
	next?: DashboardLiveUpdateCheckOptions,
): DashboardLiveUpdateCheckOptions {
	if (!current) return next ?? {};
	const merged: DashboardLiveUpdateCheckOptions = {};
	if (current.emit === false || next?.emit === false) {
		merged.emit = false;
	}
	if (current.include && next?.include) {
		merged.include = Array.from(new Set([...current.include, ...next.include]));
	} else if (current.include || next?.include) {
		merged.include = current.include ?? next?.include;
	}
	return merged;
}

function buildBaselineKey(options: {
	feedType: FeedRequestType;
	includeBriefs: boolean;
	includeNotifications: boolean;
}) {
	return `${options.feedType}|briefs:${options.includeBriefs ? "1" : "0"}|notifications:${options.includeNotifications ? "1" : "0"}`;
}

export function useDashboardLiveUpdates(options: {
	enabled: boolean;
	feedType: FeedRequestType;
	includeNotifications: boolean;
	includeBriefs: boolean;
	onUpdate: (notices: DashboardLiveUpdateNotice[]) => void;
}) {
	const { enabled, feedType, includeNotifications, includeBriefs, onUpdate } =
		options;
	const baselineKey = buildBaselineKey({
		feedType,
		includeBriefs,
		includeNotifications,
	});
	const tokenRef = useRef<string | null>(null);
	const timerRef = useRef<number | null>(null);
	const inFlightRef = useRef(false);
	const activeRef = useRef(false);
	const generationRef = useRef(0);
	const errorCountRef = useRef(0);
	const onUpdateRef = useRef(onUpdate);
	const checkNowRef = useRef<
		(options?: DashboardLiveUpdateCheckOptions) => Promise<void>
	>(async () => undefined);
	const feedTypeRef = useRef(feedType);
	const includeRef = useRef({ includeBriefs, includeNotifications });
	const pendingIncludeBootstrapRef = useRef<DashboardLiveUpdateList[]>([]);
	const queuedCheckRef = useRef<DashboardLiveUpdateCheckOptions | null>(null);

	const currentBaselineKey = useCallback(
		() =>
			buildBaselineKey({
				feedType: feedTypeRef.current,
				includeBriefs: includeRef.current.includeBriefs,
				includeNotifications: includeRef.current.includeNotifications,
			}),
		[],
	);

	useEffect(() => {
		onUpdateRef.current = onUpdate;
	}, [onUpdate]);

	useEffect(() => {
		if (feedTypeRef.current !== feedType) {
			feedTypeRef.current = feedType;
			includeRef.current = { includeBriefs, includeNotifications };
			pendingIncludeBootstrapRef.current = [];
			queuedCheckRef.current = null;
			tokenRef.current = null;
			errorCountRef.current = 0;
			return;
		}
		const previous = includeRef.current;
		const newlyIncluded: DashboardLiveUpdateList[] = [];
		if (!previous.includeBriefs && includeBriefs) newlyIncluded.push("briefs");
		if (!previous.includeNotifications && includeNotifications) {
			newlyIncluded.push("notifications");
		}
		includeRef.current = { includeBriefs, includeNotifications };
		if (newlyIncluded.length > 0) {
			pendingIncludeBootstrapRef.current = [
				...pendingIncludeBootstrapRef.current,
				...newlyIncluded,
			];
		}
	}, [feedType, includeBriefs, includeNotifications]);

	const clearTimer = useCallback(() => {
		if (timerRef.current === null) return;
		window.clearTimeout(timerRef.current);
		timerRef.current = null;
	}, []);

	const scheduleNext = useCallback(
		(delayOverride?: number) => {
			clearTimer();
			if (!enabled || typeof window === "undefined") return;
			if (!activeRef.current) return;
			if (!navigator.onLine) return;
			const baseDelay =
				document.visibilityState === "hidden"
					? BACKGROUND_POLL_MS
					: FOREGROUND_POLL_MS;
			const errorBackoff =
				errorCountRef.current > 0
					? Math.min(
							MAX_ERROR_BACKOFF_MS,
							FOREGROUND_POLL_MS * 2 ** (errorCountRef.current - 1),
						)
					: 0;
			timerRef.current = window.setTimeout(() => {
				void checkNowRef.current();
			}, delayOverride ?? Math.max(baseDelay, errorBackoff));
		},
		[clearTimer, enabled],
	);

	const checkNow = useCallback(
		async (options?: DashboardLiveUpdateCheckOptions) => {
			if (!enabled || !activeRef.current) {
				if (options?.emit === false) {
					queuedCheckRef.current = mergeQueuedCheckOptions(
						queuedCheckRef.current,
						options,
					);
				}
				return;
			}
			if (inFlightRef.current) {
				queuedCheckRef.current = mergeQueuedCheckOptions(
					queuedCheckRef.current,
					options,
				);
				return;
			}
			if (typeof window === "undefined") return;
			if (!navigator.onLine) {
				scheduleNext();
				return;
			}
			inFlightRef.current = true;
			const requestBaselineKey = baselineKey;
			const requestGeneration = generationRef.current;
			try {
				const bootstrapInclude = pendingIncludeBootstrapRef.current;
				if (bootstrapInclude.length > 0 && tokenRef.current) {
					const bootstrapParams = new URLSearchParams();
					bootstrapParams.set("feed_type", feedType);
					bootstrapParams.set(
						"include",
						includeParamForLists(bootstrapInclude),
					);
					bootstrapParams.set("token", tokenRef.current);
					const bootstrapResponse = await apiGet<DashboardUpdatesResponse>(
						`/api/dashboard/updates?${bootstrapParams.toString()}`,
					);
					if (currentBaselineKey() !== requestBaselineKey) return;
					tokenRef.current = bootstrapResponse.token;
					pendingIncludeBootstrapRef.current = [];
				}
				const params = new URLSearchParams();
				params.set("feed_type", feedType);
				params.set(
					"include",
					options?.include
						? includeParamForLists(options.include)
						: buildIncludeParam({ includeBriefs, includeNotifications }),
				);
				if (tokenRef.current) {
					params.set("token", tokenRef.current);
				}
				const response = await apiGet<DashboardUpdatesResponse>(
					`/api/dashboard/updates?${params.toString()}`,
				);
				if (currentBaselineKey() !== requestBaselineKey) return;
				errorCountRef.current = 0;
				const notices: DashboardLiveUpdateNotice[] = [];
				for (const list of ["feed", "briefs", "notifications"] as const) {
					const update = response.lists[list];
					if (!update?.changed || update.new_count <= 0) continue;
					notices.push({
						list,
						newCount: update.new_count,
						latestKeys: update.latest_keys,
						feedType: list === "feed" ? feedType : undefined,
					});
				}
				const queuedSilentCheck =
					queuedCheckRef.current?.emit === false
						? queuedCheckRef.current
						: null;
				const suppressedLists =
					options?.emit !== false && queuedSilentCheck
						? new Set<DashboardLiveUpdateList>(
								queuedSilentCheck.include ?? [
									"feed",
									"briefs",
									"notifications",
								],
							)
						: null;
				const emittedNotices = suppressedLists
					? notices.filter((notice) => !suppressedLists.has(notice.list))
					: notices;
				const shouldEmit = options?.emit !== false && emittedNotices.length > 0;
				if (shouldEmit) {
					onUpdateRef.current(emittedNotices);
				} else if (
					options?.emit === false ||
					(!suppressedLists && notices.length === 0)
				) {
					tokenRef.current = response.token;
				}
			} catch {
				errorCountRef.current += 1;
			} finally {
				inFlightRef.current = false;
				const queuedCheck = queuedCheckRef.current;
				queuedCheckRef.current = null;
				if (queuedCheck && activeRef.current) {
					void checkNowRef.current(queuedCheck);
				} else if (
					activeRef.current &&
					generationRef.current === requestGeneration
				) {
					scheduleNext();
				}
			}
		},
		[
			baselineKey,
			enabled,
			feedType,
			includeBriefs,
			includeNotifications,
			currentBaselineKey,
			scheduleNext,
		],
	);

	useEffect(() => {
		checkNowRef.current = checkNow;
	}, [checkNow]);

	useEffect(() => {
		if (!enabled) {
			activeRef.current = false;
			generationRef.current += 1;
			clearTimer();
			return;
		}
		activeRef.current = true;
		generationRef.current += 1;
		const queuedCheck = queuedCheckRef.current;
		queuedCheckRef.current = null;
		void checkNow(queuedCheck ?? undefined);
		const onVisibilityChange = () => scheduleNext(1_000);
		const onOnline = () => void checkNow();
		document.addEventListener("visibilitychange", onVisibilityChange);
		window.addEventListener("online", onOnline);
		window.addEventListener("offline", clearTimer);
		return () => {
			activeRef.current = false;
			generationRef.current += 1;
			clearTimer();
			document.removeEventListener("visibilitychange", onVisibilityChange);
			window.removeEventListener("online", onOnline);
			window.removeEventListener("offline", clearTimer);
		};
	}, [checkNow, clearTimer, enabled, scheduleNext]);

	return { checkNow };
}
