import {
	ArrowLeft,
	ChevronDown,
	ChevronUp,
	ExternalLink,
	RefreshCcw,
} from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";
import {
	defaultRangeExtractor,
	measureElement as defaultMeasureElement,
	useVirtualizer,
	useWindowVirtualizer,
} from "@tanstack/react-virtual";
import {
	useCallback,
	useEffect,
	useLayoutEffect,
	useMemo,
	useRef,
	useState,
} from "react";

import {
	ApiError,
	apiGetReactionTokenStatus,
	type PublicReleaseHighlight,
	type PublicReleaseGap,
	type PublicReleaseListItem,
	type PublicReleasePendingResponse,
	type PublicReleaseResponse,
	type ReleaseDetailResponse,
	apiGetPublicRepoReleaseContent,
	apiGetPublicRepoReleases,
	apiPostJson,
} from "@/api";
import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { AuthProviderIcon } from "@/components/brand/AuthProviderIcon";
import { BrandLogo } from "@/components/brand/BrandLogo";
import { RepoIdentity } from "@/components/repo/RepoIdentity";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { ReleaseFeedCard } from "@/feed/FeedItemCard";
import { FeedPageLaneSelector } from "@/feed/FeedPageLaneSelector";
import { InternalLink } from "@/lib/internalNavigation";
import type {
	FeedLane,
	FeedReactionRefreshResponse,
	ReactionContent,
	ReleaseFeedItem,
	ReleaseReactions,
	ToggleReleaseReactionResponse,
} from "@/feed/types";
import {
	appendPublicReleaseHighlightParams,
	publicReleaseHighlightSearch,
	type PublicReleaseHighlightSelection,
} from "@/publicRelease/routeState";
import {
	DASHBOARD_QUERY_STALE_MS,
	dashboardReactionTokenQueryKey,
} from "@/query/dashboardQueryKeys";
import { isReactionTokenUsable } from "@/settings/reactionTokenEditor";
import { cn } from "@/lib/utils";
import { buildVersionReleaseHref } from "@/version/versionReleaseLink";
import { useVersionMonitor } from "@/version/versionMonitor";

const PUBLIC_RELEASE_LIST_BODY_MAX_CHARS = 2800;
const PUBLIC_RELEASE_PAGE_SIZE = 6;
const PUBLIC_RELEASE_HIGHLIGHT_PAGE_SIZE = 30;
const PUBLIC_RELEASE_REACTION_BATCH_SIZE = 100;
const PUBLIC_RELEASE_REACTION_MAX_RETRIES = 3;
const PUBLIC_RELEASE_REACTION_RETRY_DELAY_MS = 1_000;
const PUBLIC_RELEASE_ESTIMATED_CARD_HEIGHT = 148;
const PUBLIC_RELEASE_FOCUS_LEAD_PX = 72;

function publicReleasePathPrefix() {
	return window.location.pathname.startsWith("/demo/") ? "/demo" : "";
}

function publicReleaseScrollBehavior(): ScrollBehavior {
	return window.matchMedia("(prefers-reduced-motion: reduce)").matches
		? "auto"
		: "smooth";
}

const releaseScrollFrames = new WeakMap<HTMLElement, number>();
const releaseScrollGuards = new WeakMap<
	HTMLElement,
	{ direction: "forward" | "backward"; extreme: number }
>();
const releaseScrollPositions = new Map<string, { directory: number }>();
const publicReleasePointerState = {
	lastDetailPointerDown: 0,
	lastDetailPointerMove: 0,
};

function isVisibleEnough(
	element: HTMLElement,
	scrollElement: HTMLElement | null,
) {
	if (!scrollElement) return false;
	const elementRect = element.getBoundingClientRect();
	const viewportRect = scrollElement.getBoundingClientRect();
	if (elementRect.height > viewportRect.height) {
		return Math.abs(elementRect.top - viewportRect.top) <= 1;
	}
	return (
		elementRect.top >= viewportRect.top &&
		elementRect.bottom <= viewportRect.bottom
	);
}

type ReleaseRevealOptions = {
	forcePrecedingLines?: boolean;
	precedingLines?: number;
};

function revealReleaseElement(
	element: HTMLElement,
	scrollElement: HTMLElement | null,
	options: ReleaseRevealOptions = {},
) {
	if (!scrollElement) return;
	const elementRect = element.getBoundingClientRect();
	const viewportRect = scrollElement.getBoundingClientRect();
	const precedingLines = Math.max(0, options.precedingLines ?? 0);
	const lineHeight = Number.parseFloat(
		window.getComputedStyle(element).lineHeight,
	);
	const lead = Number.isFinite(lineHeight)
		? lineHeight * precedingLines
		: 24 * precedingLines;
	let nextTop = scrollElement.scrollTop;
	if (elementRect.top < viewportRect.top) {
		nextTop += elementRect.top - viewportRect.top;
	} else if (
		elementRect.bottom > viewportRect.bottom ||
		(precedingLines > 0 &&
			(options.forcePrecedingLines ||
				elementRect.top < viewportRect.top + lead))
	) {
		const targetOffset = Math.min(
			lead,
			Math.max(0, viewportRect.height - elementRect.height),
		);
		nextTop += elementRect.top - (viewportRect.top + targetOffset);
	}
	animateReleaseScrollTo(scrollElement, nextTop);
}

function animateReleaseScrollTo(scrollElement: HTMLElement, targetTop: number) {
	const requestedTarget = Math.max(
		0,
		Math.min(
			targetTop,
			scrollElement.scrollHeight - scrollElement.clientHeight,
		),
	);
	const currentTop = scrollElement.scrollTop;
	const existingGuard = releaseScrollGuards.get(scrollElement);
	const requestedDirection =
		requestedTarget >= currentTop ? "forward" : "backward";
	const direction = existingGuard?.direction ?? requestedDirection;
	const clampedTarget =
		existingGuard && existingGuard.direction !== requestedDirection
			? currentTop
			: requestedTarget;
	if (!existingGuard && Math.abs(requestedTarget - currentTop) >= 1) {
		releaseScrollGuards.set(scrollElement, {
			direction,
			extreme: currentTop,
		});
	}
	if (Math.abs(clampedTarget - scrollElement.scrollTop) < 1) return;
	const previousFrame = releaseScrollFrames.get(scrollElement);
	if (previousFrame !== undefined) {
		window.cancelAnimationFrame(previousFrame);
	}
	if (publicReleaseScrollBehavior() === "auto") {
		scrollElement.style.scrollBehavior = "auto";
		scrollElement.scrollTop = clampedTarget;
		releaseScrollFrames.delete(scrollElement);
		// The guard only protects the active programmatic seek. Keeping it after
		// completion makes later, user-driven geometry changes fight the stale
		// extreme and can produce an oscillating scroll position.
		releaseScrollGuards.delete(scrollElement);
		window.requestAnimationFrame(() => {
			if (scrollElement.isConnected) scrollElement.style.scrollBehavior = "";
		});
		return;
	}
	const startTop = scrollElement.scrollTop;
	const startedAt = performance.now();
	const duration = Math.min(
		520,
		Math.max(220, Math.abs(clampedTarget - startTop) * 0.45),
	);
	const step = (now: number) => {
		const progress = Math.min(1, (now - startedAt) / duration);
		const eased = 1 - (1 - progress) ** 3;
		scrollElement.style.scrollBehavior = "auto";
		scrollElement.scrollTop = startTop + (clampedTarget - startTop) * eased;
		const guard = releaseScrollGuards.get(scrollElement);
		if (guard) {
			guard.extreme =
				guard.direction === "forward"
					? Math.max(guard.extreme, scrollElement.scrollTop)
					: Math.min(guard.extreme, scrollElement.scrollTop);
		}
		if (progress < 1 && scrollElement.isConnected) {
			const frame = window.requestAnimationFrame(step);
			releaseScrollFrames.set(scrollElement, frame);
			return;
		}
		releaseScrollFrames.delete(scrollElement);
		releaseScrollGuards.delete(scrollElement);
		if (scrollElement.isConnected) scrollElement.style.scrollBehavior = "";
	};
	const frame = window.requestAnimationFrame(step);
	releaseScrollFrames.set(scrollElement, frame);
}

function cancelSmoothScroll(element: HTMLElement | null) {
	if (!element) return;
	releaseScrollGuards.delete(element);
	const frame = releaseScrollFrames.get(element);
	if (frame !== undefined) {
		window.cancelAnimationFrame(frame);
		releaseScrollFrames.delete(element);
	}
	const top = element.scrollTop;
	element.style.scrollBehavior = "auto";
	element.scrollTop = top;
	window.requestAnimationFrame(() => {
		if (element.isConnected) element.style.scrollBehavior = "";
	});
}

type PublicReleaseReactionControls = {
	enabled: boolean;
	byReleaseId: Record<string, ReleaseReactions>;
	availableReleaseIds: Set<string>;
	busyReleaseIds: Set<string>;
	errorByReleaseId: Record<string, string>;
	onToggle: (releaseId: string, content: ReactionContent) => void;
};

type LoadState =
	| { status: "loading" }
	| { status: "pending"; pending: PublicReleasePendingResponse }
	| {
			status: "list";
			data: Extract<PublicReleaseResponse, { status: "ready" }>;
	  }
	| { status: "error"; message: string; code?: string };

function isPendingResponse(
	value: unknown,
): value is PublicReleasePendingResponse {
	return (
		typeof value === "object" &&
		value !== null &&
		"status" in value &&
		(value as { status?: unknown }).status === "pending_sync"
	);
}

function usePublicReleaseReactionControls(
	items: PublicReleaseListItem[],
): PublicReleaseReactionControls {
	const auth = useAuthBootstrap();
	const userId = auth.me?.user.id ?? null;
	const reactionTokenQuery = useQuery({
		queryKey: dashboardReactionTokenQueryKey(userId ?? "anonymous"),
		queryFn: apiGetReactionTokenStatus,
		enabled: auth.isAuthenticated && userId !== null,
		staleTime: DASHBOARD_QUERY_STALE_MS,
		retry: false,
	});
	const [reactionAccessBlocked, setReactionAccessBlocked] = useState(false);
	const [byReleaseId, setByReleaseId] = useState<
		Record<string, ReleaseReactions>
	>({});
	const [availableReleaseIds, setAvailableReleaseIds] = useState<Set<string>>(
		() => new Set(),
	);
	const [busyReleaseIds, setBusyReleaseIds] = useState<Set<string>>(
		() => new Set(),
	);
	const [errorByReleaseId, setErrorByReleaseId] = useState<
		Record<string, string>
	>({});
	const [reactionRefreshGeneration, setReactionRefreshGeneration] = useState(0);
	const releaseIdSignature = useMemo(
		() =>
			Array.from(new Set(items.map((item) => item.release_id)))
				.sort()
				.join("|"),
		[items],
	);
	const requestedReleaseIdsRef = useRef(new Set<string>());
	const reactionRefreshRetriesRef = useRef(new Map<string, number>());
	const reactionRefreshRetryTimerRef = useRef<number | null>(null);
	const reactionSessionRef = useRef({ userId, generation: 0 });
	if (reactionSessionRef.current.userId !== userId) {
		reactionSessionRef.current = {
			userId,
			generation: reactionSessionRef.current.generation + 1,
		};
	}
	const enabled =
		auth.isAuthenticated &&
		!reactionAccessBlocked &&
		isReactionTokenUsable(
			reactionTokenQuery.data ?? {
				configured: false,
				masked_token: null,
				owner: null,
				check: { state: "idle", message: null, checked_at: null },
			},
		);

	useEffect(() => {
		if (reactionRefreshRetryTimerRef.current !== null) {
			window.clearTimeout(reactionRefreshRetryTimerRef.current);
			reactionRefreshRetryTimerRef.current = null;
		}
		setReactionAccessBlocked(false);
		setByReleaseId({});
		setAvailableReleaseIds(new Set());
		setBusyReleaseIds(new Set());
		setErrorByReleaseId({});
		requestedReleaseIdsRef.current = new Set();
		reactionRefreshRetriesRef.current = new Map();
	}, [userId]);

	useEffect(
		() => () => {
			if (reactionRefreshRetryTimerRef.current !== null) {
				window.clearTimeout(reactionRefreshRetryTimerRef.current);
			}
		},
		[],
	);

	const scheduleReactionRefreshRetry = useCallback(() => {
		if (reactionRefreshRetryTimerRef.current !== null) return;
		reactionRefreshRetryTimerRef.current = window.setTimeout(() => {
			reactionRefreshRetryTimerRef.current = null;
			setReactionRefreshGeneration((current) => current + 1);
		}, PUBLIC_RELEASE_REACTION_RETRY_DELAY_MS);
	}, []);

	useEffect(() => {
		if (!enabled || !releaseIdSignature) return;
		const releaseIds = releaseIdSignature
			.split("|")
			.filter((releaseId) => !requestedReleaseIdsRef.current.has(releaseId));
		if (releaseIds.length === 0) return;
		for (const releaseId of releaseIds) {
			requestedReleaseIdsRef.current.add(releaseId);
		}
		const batches = Array.from(
			{
				length: Math.ceil(
					releaseIds.length / PUBLIC_RELEASE_REACTION_BATCH_SIZE,
				),
			},
			(_, index) =>
				releaseIds.slice(
					index * PUBLIC_RELEASE_REACTION_BATCH_SIZE,
					(index + 1) * PUBLIC_RELEASE_REACTION_BATCH_SIZE,
				),
		);
		const requestSessionGeneration = reactionSessionRef.current.generation;

		void Promise.allSettled(
			batches.map((batch) =>
				apiPostJson<FeedReactionRefreshResponse>(
					"/api/feed/reactions/refresh",
					{ release_ids: batch },
				),
			),
		).then((results) => {
			if (reactionSessionRef.current.generation !== requestSessionGeneration) {
				return;
			}
			const refreshed = results.flatMap((result) =>
				result.status === "fulfilled" ? result.value.items : [],
			);
			for (const item of refreshed) {
				reactionRefreshRetriesRef.current.delete(item.release_id);
			}
			setByReleaseId((current) => ({
				...current,
				...Object.fromEntries(
					refreshed.map((item) => [item.release_id, item.reactions]),
				),
			}));
			setAvailableReleaseIds(
				(current) =>
					new Set([...current, ...refreshed.map((item) => item.release_id)]),
			);

			let shouldRetry = false;
			for (const [index, result] of results.entries()) {
				if (result.status === "fulfilled") continue;
				const failedReleaseIds = batches[index] ?? [];
				for (const releaseId of failedReleaseIds) {
					requestedReleaseIdsRef.current.delete(releaseId);
				}
				if (
					result.reason instanceof ApiError &&
					(result.reason.code === "pat_invalid" ||
						result.reason.code === "pat_required")
				) {
					setReactionAccessBlocked(true);
					continue;
				}
				for (const releaseId of failedReleaseIds) {
					const retries =
						(reactionRefreshRetriesRef.current.get(releaseId) ?? 0) + 1;
					reactionRefreshRetriesRef.current.set(releaseId, retries);
					shouldRetry ||= retries <= PUBLIC_RELEASE_REACTION_MAX_RETRIES;
				}
			}
			if (shouldRetry) scheduleReactionRefreshRetry();
		});
	}, [
		enabled,
		reactionRefreshGeneration,
		releaseIdSignature,
		scheduleReactionRefreshRetry,
	]);

	const onToggle = useCallback(
		(releaseId: string, content: ReactionContent) => {
			if (!enabled || busyReleaseIds.has(releaseId)) return;
			const requestSessionGeneration = reactionSessionRef.current.generation;
			setBusyReleaseIds((current) => new Set(current).add(releaseId));
			setErrorByReleaseId((current) => {
				if (!(releaseId in current)) return current;
				const next = { ...current };
				delete next[releaseId];
				return next;
			});

			void apiPostJson<ToggleReleaseReactionResponse>(
				"/api/release/reactions/toggle",
				{ release_id: releaseId, content },
			)
				.then((response) => {
					if (
						reactionSessionRef.current.generation !== requestSessionGeneration
					) {
						return;
					}
					setByReleaseId((current) => ({
						...current,
						[response.release_id]: response.reactions,
					}));
				})
				.catch((error) => {
					if (
						reactionSessionRef.current.generation !== requestSessionGeneration
					) {
						return;
					}
					if (
						error instanceof ApiError &&
						(error.code === "pat_invalid" || error.code === "pat_required")
					) {
						setReactionAccessBlocked(true);
						return;
					}
					if (error instanceof ApiError && error.code === "not_found") {
						setAvailableReleaseIds((current) => {
							const next = new Set(current);
							next.delete(releaseId);
							return next;
						});
						return;
					}
					setErrorByReleaseId((current) => ({
						...current,
						[releaseId]:
							error instanceof Error ? error.message : "表情反应更新失败",
					}));
				})
				.finally(() => {
					if (
						reactionSessionRef.current.generation !== requestSessionGeneration
					) {
						return;
					}
					setBusyReleaseIds((current) => {
						const next = new Set(current);
						next.delete(releaseId);
						return next;
					});
				});
		},
		[busyReleaseIds, enabled],
	);

	return {
		enabled,
		byReleaseId,
		availableReleaseIds,
		busyReleaseIds,
		errorByReleaseId,
		onToggle,
	};
}

function releaseTitle(item: Pick<PublicReleaseListItem, "name" | "tag_name">) {
	return item.name?.trim() || item.tag_name;
}

function mergePublicReleaseItems(
	current: PublicReleaseListItem[],
	incoming: PublicReleaseListItem[],
) {
	const byId = new Map(current.map((item) => [item.release_id, item]));
	for (const item of incoming) {
		byId.set(item.release_id, { ...byId.get(item.release_id), ...item });
	}
	return Array.from(byId.values()).sort((left, right) => {
		const ts = (right.published_at ?? "").localeCompare(
			left.published_at ?? "",
		);
		if (ts !== 0) return ts;
		return right.release_id.localeCompare(left.release_id, undefined, {
			numeric: true,
		});
	});
}

function activeHighlightSelector(highlight?: PublicReleaseHighlight) {
	const activeReleaseId = highlight?.active_release_id;
	if (!activeReleaseId) return undefined;
	return (
		highlight.resolved.find((target) => target.release_id === activeReleaseId)
			?.selector ?? `id:${activeReleaseId}`
	);
}

function mergePaginatedHighlight(
	current: PublicReleaseHighlight | undefined,
	incoming: PublicReleaseHighlight | undefined,
) {
	if (!current) return incoming;
	if (!incoming) return current;
	return {
		...incoming,
		active_release_id: current.active_release_id,
		active_index: current.active_index,
	};
}

function applyActiveHighlight(
	items: PublicReleaseListItem[],
	highlight: PublicReleaseHighlight | undefined,
) {
	if (!highlight?.active_release_id) return items;
	return items.map((item) => ({
		...item,
		is_active_highlight: item.release_id === highlight.active_release_id,
	}));
}

function PulseBlock(props: { className?: string; rounded?: string }) {
	const { className, rounded = "rounded-2xl" } = props;
	return (
		<div
			className={cn("bg-muted/70 animate-pulse", rounded, className)}
			data-testid="public-release-skeleton-block"
		/>
	);
}

export function PublicReleasePage(props: {
	owner: string;
	repo: string;
	tag?: string | null;
	highlight?: PublicReleaseHighlightSelection;
}) {
	const { owner, repo, tag = null, highlight = null } = props;
	const timelineCacheKey = `${owner}/${repo}`;
	const [state, setState] = useState<LoadState>({ status: "loading" });
	const [loadingMore, setLoadingMore] = useState(false);
	const [loadingNewer, setLoadingNewer] = useState(false);
	const [loadingGap, setLoadingGap] = useState<string | null>(null);
	const [appendError, setAppendError] = useState<string | null>(null);
	const [selectedLane, setSelectedLane] = useState<FeedLane>("smart");
	const initialLoadKeyRef = useRef<string | null>(null);
	const initialLoadPendingRef = useRef<string | null>(null);
	const isHighlightMode = highlight !== null;
	const initialLoadKey = JSON.stringify({ owner, repo, tag, highlight });
	const requestKeyRef = useRef(initialLoadKey);
	if (requestKeyRef.current !== initialLoadKey) {
		requestKeyRef.current = initialLoadKey;
	}
	const previousTimelineCacheKeyRef = useRef(timelineCacheKey);
	const reactionControls = usePublicReleaseReactionControls(
		state.status === "list" ? state.data.items : [],
	);
	useEffect(() => {
		if (previousTimelineCacheKeyRef.current === timelineCacheKey) return;
		previousTimelineCacheKeyRef.current = timelineCacheKey;
		setState({ status: "loading" });
		setLoadingMore(false);
		setLoadingNewer(false);
		setLoadingGap(null);
		setAppendError(null);
	}, [timelineCacheKey]);
	useEffect(() => {
		setLoadingMore(false);
		setLoadingNewer(false);
		setLoadingGap(null);
		setAppendError(null);
	}, [initialLoadKey]);

	const highlightRequest = useMemo(() => {
		if (!highlight) return {};
		if (highlight.mode === "discrete") {
			return {
				highlight: highlight.selectors,
				highlight_active: highlight.active,
			};
		}
		if (highlight.mode === "range") {
			return {
				highlight_start: highlight.start,
				highlight_end: highlight.end,
				highlight_active: highlight.active,
			};
		}
		return {
			highlight: highlight.selectors,
			highlight_start: highlight.start,
			highlight_end: highlight.end,
			highlight_active: highlight.active,
		};
	}, [highlight]);

	const buildHighlightRequest = useCallback(
		(
			direction?: "older" | "newer",
			cursor?: string | null,
			activeSelector?: string,
		) => ({
			owner,
			repo,
			source: "page" as const,
			limit: isHighlightMode
				? PUBLIC_RELEASE_HIGHLIGHT_PAGE_SIZE
				: PUBLIC_RELEASE_PAGE_SIZE,
			cursor,
			direction,
			content: "polished" as const,
			include_original: true,
			...highlightRequest,
			...(activeSelector ? { highlight_active: activeSelector } : {}),
			...(tag && !cursor ? { focus: `tag:${tag}` } : {}),
		}),
		[highlightRequest, isHighlightMode, owner, repo, tag],
	);

	const load = useCallback(async () => {
		const requestKey = initialLoadKey;
		initialLoadPendingRef.current = requestKey;
		try {
			setState((current) =>
				current.status === "error" ? { status: "loading" } : current,
			);
			setAppendError(null);
			const data = await apiGetPublicRepoReleases({
				...buildHighlightRequest(),
			});
			if (requestKeyRef.current !== requestKey) return;
			if (isPendingResponse(data)) {
				setState({ status: "pending", pending: data });
			} else {
				const nextData = data as Extract<
					PublicReleaseResponse,
					{ status: "ready" }
				>;
				setState((current) => {
					if (requestKeyRef.current !== requestKey) return current;
					if (current.status !== "list" || !tag) {
						return { status: "list", data: nextData };
					}
					const targetAlreadyLoaded = current.data.items.some(
						(item) => item.tag_name === tag,
					);
					if (!targetAlreadyLoaded) {
						return { status: "list", data: nextData };
					}
					return {
						status: "list",
						data: {
							...nextData,
							items: mergePublicReleaseItems(
								current.data.items,
								nextData.items,
							),
							next_cursor: current.data.next_cursor ?? nextData.next_cursor,
							previous_cursor:
								current.data.previous_cursor ?? nextData.previous_cursor,
							highlight: mergePaginatedHighlight(
								current.data.highlight,
								nextData.highlight,
							),
						},
					};
				});
			}
		} catch (err) {
			if (requestKeyRef.current !== requestKey) return;
			if (err instanceof ApiError) {
				setState({ status: "error", message: err.message, code: err.code });
				return;
			}
			setState({ status: "error", message: "公开 Release 加载失败" });
		} finally {
			if (initialLoadPendingRef.current === requestKey) {
				initialLoadPendingRef.current = null;
			}
		}
	}, [buildHighlightRequest, initialLoadKey, repo, tag, timelineCacheKey]);

	const mergeItems = useCallback(
		(current: PublicReleaseListItem[], incoming: PublicReleaseListItem[]) => {
			return mergePublicReleaseItems(current, incoming);
		},
		[],
	);

	const loadMore = useCallback(async () => {
		if (
			loadingMore ||
			initialLoadPendingRef.current === initialLoadKey ||
			state.status !== "list" ||
			!state.data.next_cursor
		) {
			return;
		}
		setLoadingMore(true);
		setAppendError(null);
		const requestKey = initialLoadKey;
		try {
			const data = await apiGetPublicRepoReleases({
				...buildHighlightRequest(
					"older",
					state.data.next_cursor,
					activeHighlightSelector(state.data.highlight),
				),
			});
			if (requestKeyRef.current !== requestKey) return;
			if (isPendingResponse(data)) {
				setState({ status: "pending", pending: data });
				return;
			}
			setState((current) => {
				if (requestKeyRef.current !== requestKey) return current;
				if (current.status !== "list") {
					return current;
				}
				const highlight = mergePaginatedHighlight(
					current.data.highlight,
					data.highlight,
				);
				return {
					status: "list",
					data: {
						...current.data,
						items: applyActiveHighlight(
							mergeItems(current.data.items, data.items),
							highlight,
						),
						next_cursor: data.next_cursor,
						previous_cursor: current.data.previous_cursor,
						highlight,
					},
				};
			});
		} catch (err) {
			if (requestKeyRef.current !== requestKey) return;
			setAppendError(err instanceof Error ? err.message : String(err));
		} finally {
			if (requestKeyRef.current === requestKey) setLoadingMore(false);
		}
	}, [buildHighlightRequest, initialLoadKey, loadingMore, mergeItems, state]);

	const loadNewer = useCallback(async () => {
		if (
			loadingNewer ||
			initialLoadPendingRef.current === initialLoadKey ||
			state.status !== "list" ||
			!state.data.previous_cursor
		) {
			return;
		}
		setLoadingNewer(true);
		setAppendError(null);
		const requestKey = initialLoadKey;
		try {
			const data = await apiGetPublicRepoReleases({
				...buildHighlightRequest(
					"newer",
					state.data.previous_cursor,
					activeHighlightSelector(state.data.highlight),
				),
			});
			if (requestKeyRef.current !== requestKey) return;
			if (isPendingResponse(data)) {
				setState({ status: "pending", pending: data });
				return;
			}
			setState((current) => {
				if (requestKeyRef.current !== requestKey) return current;
				if (current.status !== "list") return current;
				const highlight = mergePaginatedHighlight(
					current.data.highlight,
					data.highlight,
				);
				return {
					status: "list",
					data: {
						...current.data,
						items: applyActiveHighlight(
							mergeItems(current.data.items, data.items),
							highlight,
						),
						previous_cursor: data.previous_cursor,
						next_cursor: current.data.next_cursor ?? data.next_cursor,
						highlight,
					},
				};
			});
		} catch (err) {
			if (requestKeyRef.current !== requestKey) return;
			setAppendError(err instanceof Error ? err.message : String(err));
		} finally {
			if (requestKeyRef.current === requestKey) setLoadingNewer(false);
		}
	}, [buildHighlightRequest, initialLoadKey, loadingNewer, mergeItems, state]);

	const loadGap = useCallback(
		async (gap: PublicReleaseGap) => {
			if (
				loadingGap ||
				initialLoadPendingRef.current === initialLoadKey ||
				state.status !== "list"
			)
				return;
			setLoadingGap(gap.newer_cursor);
			setAppendError(null);
			const requestKey = initialLoadKey;
			try {
				const data = await apiGetPublicRepoReleases({
					...buildHighlightRequest(
						"older",
						gap.newer_cursor,
						activeHighlightSelector(state.data.highlight),
					),
					until_cursor: gap.older_cursor,
				});
				if (requestKeyRef.current !== requestKey) return;
				if (isPendingResponse(data)) return;
				setState((current) => {
					if (requestKeyRef.current !== requestKey) return current;
					if (current.status !== "list") return current;
					const highlight = mergePaginatedHighlight(
						current.data.highlight,
						data.highlight,
					);
					const existingIds = new Set(
						current.data.items.map((item) => item.release_id),
					);
					const inserted = data.items.filter(
						(item) => !existingIds.has(item.release_id),
					).length;
					const reachedOlderBoundary = data.items.some((item) =>
						gap.older_cursor.endsWith(`|${item.release_id}`),
					);
					const nextGaps = (current.data.gaps ?? []).flatMap((candidate) => {
						if (candidate.newer_cursor !== gap.newer_cursor) return [candidate];
						if (reachedOlderBoundary || !data.next_cursor) return [];
						return [
							{
								...candidate,
								newer_cursor: data.next_cursor,
								remaining_count: Math.max(
									0,
									candidate.remaining_count - inserted,
								),
							},
						];
					});
					return {
						status: "list",
						data: {
							...current.data,
							items: applyActiveHighlight(
								mergeItems(current.data.items, data.items),
								highlight,
							),
							gaps: nextGaps,
							highlight,
						},
					};
				});
			} catch (err) {
				if (requestKeyRef.current !== requestKey) return;
				setAppendError(err instanceof Error ? err.message : String(err));
			} finally {
				if (requestKeyRef.current === requestKey) setLoadingGap(null);
			}
		},
		[
			buildHighlightRequest,
			initialLoadKey,
			loadingGap,
			mergeItems,
			state.status,
		],
	);

	const hydrateItems = useCallback(
		(
			items: Array<Pick<PublicReleaseListItem, "release_id" | "translated">>,
		) => {
			setState((current) => {
				if (current.status !== "list") return current;
				const translatedById = new Map(
					items.map((item) => [item.release_id, item.translated]),
				);
				return {
					status: "list",
					data: {
						...current.data,
						items: current.data.items.map((item) =>
							translatedById.has(item.release_id)
								? {
										...item,
										translated: translatedById.get(item.release_id) ?? null,
									}
								: item,
						),
					},
				};
			});
		},
		[],
	);

	const activateHighlight = useCallback((releaseId: string, index: number) => {
		setState((current) => {
			if (current.status !== "list" || !current.data.highlight) return current;
			return {
				status: "list",
				data: {
					...current.data,
					highlight: {
						...current.data.highlight,
						active_release_id: releaseId,
						active_index: index,
					},
					items: current.data.items.map((item) => ({
						...item,
						is_active_highlight: item.release_id === releaseId,
					})),
				},
			};
		});
	}, []);

	useEffect(() => {
		if (initialLoadKeyRef.current === initialLoadKey) return;
		initialLoadKeyRef.current = initialLoadKey;
		void load();
	}, [initialLoadKey, load]);

	useEffect(() => {
		if (state.status !== "pending") return;
		const delay = Math.max(15, state.pending.retry_after_seconds) * 1000;
		const timer = window.setTimeout(() => void load(), delay);
		return () => window.clearTimeout(timer);
	}, [load, state]);

	const repoFullName = useMemo(() => `${owner}/${repo}`, [owner, repo]);
	const repoVisual =
		state.status === "list" ? state.data.items[0]?.repo_visual : null;
	const currentHighlightSelector =
		state.status === "list" && state.data.highlight
			? state.data.highlight.resolved.find(
					(target) =>
						target.release_id === state.data.highlight?.active_release_id,
				)?.selector
			: undefined;
	const highlightedListSearch = useMemo(() => {
		const search = publicReleaseHighlightSearch(highlight);
		return currentHighlightSelector
			? { ...search, highlight_active: currentHighlightSelector }
			: search;
	}, [currentHighlightSelector, highlight]);
	const highlightedListHref = useMemo(() => {
		if (!tag || !highlight) return null;
		const params = new URLSearchParams();
		for (const [key, value] of Object.entries(highlightedListSearch)) {
			if (Array.isArray(value)) {
				for (const entry of value) params.append(key, entry);
			} else if (value !== undefined) {
				params.set(key, value);
			}
		}
		return `${publicReleasePathPrefix()}/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/releases?${params.toString()}`;
	}, [highlight, highlightedListSearch, owner, repo, tag]);

	return (
		<main className="min-h-dvh bg-background text-foreground">
			<div className="mx-auto flex min-h-dvh w-full max-w-5xl flex-col px-4 py-5 sm:px-6 lg:px-8">
				<div className="flex min-h-full flex-col">
					<header className="flex flex-wrap items-center justify-between gap-3 border-b pb-4">
						<InternalLink
							href="/"
							to="/"
							className="inline-flex items-center gap-3"
						>
							<BrandLogo variant="wordmark" className="h-7 sm:h-8" />
						</InternalLink>
						<Button asChild variant="outline" size="sm">
							<a
								href={`https://github.com/${owner}/${repo}/releases`}
								target="_blank"
								rel="noreferrer"
							>
								<ExternalLink className="size-4" />
								GitHub
							</a>
						</Button>
					</header>

					{highlightedListHref ? (
						<div className="pt-4">
							<InternalLink
								href={highlightedListHref}
								to="/$owner/$repo/releases"
								params={{ owner, repo }}
								search={highlightedListSearch}
								className="inline-flex items-center gap-2 text-sm text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
							>
								<ArrowLeft className="size-4" />
								返回高亮列表
							</InternalLink>
						</div>
					) : null}

					<section className="py-6" data-testid="public-release-title-band">
						<div className="flex flex-wrap items-center gap-x-6 gap-y-3">
							<RepoIdentity
								repoFullName={repoFullName}
								repoVisual={repoVisual}
								labelAs="h1"
								className="min-w-0 max-w-full flex-[1_1_100%] sm:flex-1"
								labelClassName="break-words text-3xl font-semibold tracking-normal"
								visualClassName="size-10"
							/>
							{state.status === "list" ? (
								<div
									className="ml-auto shrink-0"
									data-testid="public-release-page-lane"
								>
									<FeedPageLaneSelector
										value={selectedLane}
										onValueChange={setSelectedLane}
									/>
								</div>
							) : null}
						</div>
					</section>

					{state.status === "loading" ? (
						<PublicReleaseLoadingSkeleton hasTag={Boolean(tag)} />
					) : null}

					{state.status === "pending" ? (
						<WaitingCard
							title="Release 数据同步中"
							description="这个仓库的 Release 数据还在同步中，稍后会自动重试。"
							retryAfter={state.pending.retry_after_seconds}
							statusLabel="同步中"
							onRetry={load}
						/>
					) : null}

					{state.status === "error" ? (
						<Card>
							<CardHeader>
								<CardTitle>暂时无法展示</CardTitle>
								<CardDescription>
									{state.code ? `${state.code}: ` : ""}
									{state.message || "请求失败，请稍后重试。"}
								</CardDescription>
							</CardHeader>
							<CardContent>
								<Button type="button" onClick={() => void load()}>
									<RefreshCcw className="size-4" />
									重试
								</Button>
							</CardContent>
						</Card>
					) : null}

					{state.status === "list" ? (
						<ReleaseTimeline
							tag={tag}
							owner={owner}
							repo={repo}
							items={state.data.items}
							highlight={state.data.highlight}
							gaps={state.data.gaps}
							hasMore={Boolean(state.data.next_cursor)}
							hasNewer={Boolean(state.data.previous_cursor)}
							loadingMore={loadingMore}
							loadingNewer={loadingNewer}
							appendError={appendError}
							onLoadMore={loadMore}
							onLoadNewer={loadNewer}
							onLoadGap={loadGap}
							loadingGap={loadingGap}
							highlightSelection={highlight}
							selectedLane={selectedLane}
							onHydrateItems={hydrateItems}
							onActivateHighlight={activateHighlight}
							reactionControls={reactionControls}
						/>
					) : null}

					<PublicReleaseFooter owner={owner} repo={repo} />
				</div>
			</div>
		</main>
	);
}

function PublicReleaseLoadingSkeleton(props: { hasTag: boolean }) {
	const { hasTag } = props;

	if (hasTag) {
		return (
			<section
				className="space-y-4"
				aria-label="Release loading skeleton"
				data-testid="public-release-loading-skeleton"
			>
				<div className="rounded-[28px] border border-border/70 bg-card/82 p-5 shadow-sm sm:p-6">
					<div className="flex flex-wrap items-start justify-between gap-4">
						<div className="min-w-0 flex-1 space-y-3">
							<PulseBlock className="h-5 w-28 rounded-full" />
							<PulseBlock className="h-9 w-3/4 max-w-xl rounded-3xl" />
							<PulseBlock className="h-4 w-48 rounded-full" />
						</div>
						<div className="flex gap-2">
							<PulseBlock className="h-10 w-20 rounded-xl" />
							<PulseBlock className="h-10 w-20 rounded-xl" />
							<PulseBlock className="h-10 w-20 rounded-xl" />
						</div>
					</div>
					<div className="mt-6 space-y-3">
						<PulseBlock className="h-4 w-full rounded-full" />
						<PulseBlock className="h-4 w-[94%] rounded-full" />
						<PulseBlock className="h-4 w-[88%] rounded-full" />
						<PulseBlock className="h-4 w-[76%] rounded-full" />
						<PulseBlock className="h-40 w-full rounded-[24px]" />
					</div>
				</div>
			</section>
		);
	}

	return (
		<section
			className="space-y-4"
			aria-label="Release loading skeleton"
			data-testid="public-release-loading-skeleton"
		>
			<div className="flex flex-wrap items-center justify-between gap-3 rounded-[28px] border border-border/70 bg-card/82 p-5 shadow-sm sm:p-6">
				<div className="space-y-3">
					<PulseBlock className="h-4 w-24 rounded-full" />
					<PulseBlock className="h-8 w-48 rounded-3xl" />
				</div>
				<div className="flex gap-2">
					<PulseBlock className="h-10 w-20 rounded-xl" />
					<PulseBlock className="h-10 w-20 rounded-xl" />
					<PulseBlock className="h-10 w-20 rounded-xl" />
				</div>
			</div>

			{Array.from({ length: 3 }, (_, index) => (
				<div
					key={`public-release-loading-card-${index}`}
					className="rounded-[28px] border border-border/70 bg-card/82 p-5 shadow-sm sm:p-6"
				>
					<div className="flex flex-wrap items-start justify-between gap-4">
						<div className="min-w-0 flex-1 space-y-3">
							<div className="flex items-center gap-3">
								<PulseBlock className="size-11 rounded-full" />
								<div className="min-w-0 flex-1 space-y-2">
									<PulseBlock className="h-4 w-44 rounded-full" />
									<PulseBlock className="h-3 w-28 rounded-full" />
								</div>
							</div>
							<PulseBlock className="h-8 w-3/4 rounded-3xl" />
						</div>
						<div className="flex gap-2">
							<PulseBlock className="h-9 w-16 rounded-xl" />
							<PulseBlock className="h-9 w-16 rounded-xl" />
							<PulseBlock className="h-9 w-16 rounded-xl" />
						</div>
					</div>
					<div className="mt-6 space-y-3">
						<PulseBlock className="h-4 w-full rounded-full" />
						<PulseBlock className="h-4 w-[95%] rounded-full" />
						<PulseBlock className="h-4 w-[82%] rounded-full" />
						<PulseBlock className="h-24 w-full rounded-[22px]" />
					</div>
				</div>
			))}
		</section>
	);
}

function PublicReleaseFooter(props: { owner: string; repo: string }) {
	const year = new Date().getFullYear();
	const repositoryHref = `https://github.com/${props.owner}/${props.repo}`;
	const { loadedVersion } = useVersionMonitor();
	const versionReleaseHref = buildVersionReleaseHref(loadedVersion);

	return (
		<footer className="mt-auto flex flex-wrap items-center justify-between gap-3 border-t pt-4 pb-1 font-mono text-[11px] text-muted-foreground">
			<span>© {year} Ivan Li</span>
			<div className="flex flex-wrap items-center justify-end gap-x-4 gap-y-1">
				<a
					href={repositoryHref}
					target="_blank"
					rel="noreferrer"
					className="inline-flex items-center gap-1.5 underline-offset-4 hover:text-foreground hover:underline"
				>
					<AuthProviderIcon provider="github" className="size-3" />
					GitHub
				</a>
				{versionReleaseHref ? (
					<a
						href={versionReleaseHref}
						className="underline-offset-4 hover:text-foreground hover:underline"
					>
						Version {loadedVersion}
					</a>
				) : (
					<span>Version {loadedVersion}</span>
				)}
			</div>
		</footer>
	);
}

function WaitingCard(props: {
	title: string;
	description?: string;
	retryAfter?: number;
	statusLabel?: string;
	onRetry: () => void;
}) {
	return (
		<Card>
			<CardHeader>
				<CardTitle>{props.title}</CardTitle>
				<CardDescription>
					{props.description ??
						"正在读取这个仓库的已知 Release 数据；若本地已有共享缓存，页面会直接显示结果。"}
				</CardDescription>
			</CardHeader>
			<CardContent className="flex flex-wrap items-center gap-3">
				<Badge
					variant="secondary"
					className="max-w-full shrink flex-wrap justify-start whitespace-normal text-left"
				>
					<span>{props.statusLabel ?? "正在读取"}</span>
					{props.retryAfter ? (
						<span className="shrink-0 whitespace-nowrap">
							· 约 {props.retryAfter}s 后重试
						</span>
					) : null}
				</Badge>
				<Button type="button" variant="outline" onClick={props.onRetry}>
					<RefreshCcw className="size-4" />
					立即重试
				</Button>
			</CardContent>
		</Card>
	);
}

type ReleaseTimelineProps = {
	tag: string | null | undefined;
	owner: string;
	repo: string;
	items: PublicReleaseListItem[];
	highlight?: PublicReleaseHighlight;
	gaps?: PublicReleaseGap[];
	highlightSelection: PublicReleaseHighlightSelection;
	selectedLane: FeedLane;
	reactionControls: PublicReleaseReactionControls;
	hasMore: boolean;
	hasNewer: boolean;
	loadingMore: boolean;
	loadingNewer: boolean;
	loadingGap: string | null;
	appendError: string | null;
	onLoadMore: () => Promise<void>;
	onLoadNewer: () => Promise<void>;
	onLoadGap: (gap: PublicReleaseGap) => Promise<void>;
	onHydrateItems: (
		items: Array<Pick<PublicReleaseListItem, "release_id" | "translated">>,
	) => void;
	onActivateHighlight: (releaseId: string, index: number) => void;
};

function ReleaseTimeline(props: ReleaseTimelineProps) {
	const router = useRouter();
	const detailScrollRef = useRef<HTMLDivElement | null>(null);
	const directoryScrollRef = useRef<HTMLDivElement | null>(null);
	const [detailScrollElement, setDetailScrollElement] =
		useState<HTMLDivElement | null>(null);
	const [directoryScrollElement, setDirectoryScrollElement] =
		useState<HTMLDivElement | null>(null);
	const hydratedTranslatedRef = useRef(new Set<string>());
	const programmaticUntilRef = useRef(0);
	const pendingFocusIdRef = useRef<string | null>(null);
	const pendingDomFocusIdRef = useRef<string | null>(null);
	const programmaticDomFocusRef = useRef<string | null>(null);
	const pendingFocusRetryTimerRef = useRef<number | null>(null);
	const pendingFocusScheduledIdRef = useRef<string | null>(null);
	const pendingFocusSeekStartedRef = useRef<string | null>(null);
	const pendingFocusGeometryRef = useRef<{
		targetId: string;
		height: number;
		scrollHeight: number;
		stableFrames: number;
		revealCount: number;
	} | null>(null);
	const pendingDirectoryFocusIdRef = useRef<string | null>(null);
	const pendingDirectoryFocusRetryTimerRef = useRef<number | null>(null);
	const routeFocusFrameRef = useRef<number | null>(null);
	const routeFocusSuppressedRef = useRef(false);
	const routeFocusLockRef = useRef<string | null>(null);
	const pendingFocusRevealAppliedRef = useRef<string | null>(null);
	const focusSignatureRef = useRef<string | null>(null);
	const naturalTagRef = useRef<string | null>(null);
	const routeKey = `${props.tag ?? "list"}:${props.highlight?.active_release_id ?? ""}`;
	const routeKeyRef = useRef(routeKey);
	const detailAnchorRef = useRef<{ id: string; offset: number } | null>(null);
	const focusedLaneAnchorRef = useRef<{
		id: string;
		lane: FeedLane;
		offset: number;
	} | null>(null);
	const focusedLanePreserveFrameRef = useRef<number | null>(null);
	const focusedLanePreserveGenerationRef = useRef(0);
	const focusedLaneTransactionRef = useRef<string | null>(null);
	const focusedLanePreserveRef = useRef<{
		id: string;
		desiredOffset: number;
		startedAt: number;
		stableFrames: number;
		lastOffset: number | null;
		lastHeight: number | null;
	} | null>(null);
	const directoryAnchorRef = useRef<{ id: string; offset: number } | null>(
		null,
	);
	const directoryRestoreTimerRef = useRef<number | null>(null);
	const directoryRestoreGenerationRef = useRef(0);
	const [focusedReleaseId, setFocusedReleaseId] = useState<string | null>(
		() =>
			props.items.find((item) => item.tag_name === props.tag)?.release_id ??
			props.items[0]?.release_id ??
			null,
	);
	const [pulseReleaseId, setPulseReleaseId] = useState<string | null>(null);
	const scrollPositionKey = `${props.owner}/${props.repo}`;
	const setDetailScrollNode = useCallback((element: HTMLDivElement | null) => {
		detailScrollRef.current = element;
		setDetailScrollElement(element);
	}, []);
	const setDirectoryScrollNode = useCallback(
		(element: HTMLDivElement | null) => {
			directoryScrollRef.current = element;
			setDirectoryScrollElement(element);
		},
		[],
	);

	type VirtualRow =
		| { kind: "release"; item: PublicReleaseListItem }
		| { kind: "gap"; gap: PublicReleaseGap };
	const rows = useMemo<VirtualRow[]>(() => {
		const gapByNewerId = new Map<string, PublicReleaseGap>();
		for (const gap of props.gaps ?? []) {
			gapByNewerId.set(gap.newer_cursor.split("|").at(-1) ?? "", gap);
		}
		return props.items.flatMap((item) => {
			const gap = gapByNewerId.get(item.release_id);
			return gap
				? [
						{ kind: "release" as const, item },
						{ kind: "gap" as const, gap },
					]
				: [{ kind: "release" as const, item }];
		});
	}, [props.gaps, props.items]);

	const detailVirtualizer = useVirtualizer({
		count: rows.length,
		getScrollElement: () => detailScrollRef.current,
		estimateSize: (index) =>
			rows[index]?.kind === "gap" ? 72 : PUBLIC_RELEASE_ESTIMATED_CARD_HEIGHT,
		overscan: 5,
		measureElement: (element, entry, instance) =>
			defaultMeasureElement(element, entry, instance),
		rangeExtractor: (range) => {
			const indexes = new Set(defaultRangeExtractor(range));
			const pendingId = pendingFocusIdRef.current;
			if (pendingId) {
				const pendingIndex = rows.findIndex(
					(row) => row.kind === "release" && row.item.release_id === pendingId,
				);
				if (pendingIndex >= 0) indexes.add(pendingIndex);
			}
			if (focusedReleaseId) {
				const focusedIndex = rows.findIndex(
					(row) =>
						row.kind === "release" && row.item.release_id === focusedReleaseId,
				);
				if (focusedIndex >= 0) indexes.add(focusedIndex);
			}
			return Array.from(indexes).sort((left, right) => left - right);
		},
		// The row anchor and focused-lane effects below own scroll preservation. Letting the
		// virtualizer compensate estimate-to-measure deltas here would move the
		// reader while a target is being revealed and can create a visible rebound.
		// @ts-expect-error @tanstack/react-virtual omits this core option from its adapter type.
		shouldAdjustScrollPositionOnItemSizeChange: () => false,
		getItemKey: (index) => {
			const row = rows[index];
			return row?.kind === "release"
				? `release-${row.item.release_id}`
				: `gap-${row?.gap.newer_cursor ?? index}`;
		},
	});
	const directoryVirtualizer = useVirtualizer({
		count: props.items.length,
		getScrollElement: () => directoryScrollRef.current,
		estimateSize: () => 62,
		overscan: 8,
		getItemKey: (index) => props.items[index]?.release_id ?? index,
	});
	const scrollDetailToIndex = useCallback(
		(index: number, options: { preflight?: boolean } = {}) => {
			const scrollElement = detailScrollRef.current;
			const offsetInfo = detailVirtualizer.getOffsetForIndex(index, "start");
			if (!scrollElement || !offsetInfo) return;
			const estimatedTop = offsetInfo[0];
			const targetTop = options.preflight
				? Math.max(0, estimatedTop - PUBLIC_RELEASE_FOCUS_LEAD_PX)
				: estimatedTop;
			animateReleaseScrollTo(scrollElement, targetTop);
		},
		[detailVirtualizer],
	);
	const scrollDirectoryToIndex = useCallback(
		(index: number) => {
			const scrollElement = directoryScrollRef.current;
			const offsetInfo = directoryVirtualizer.getOffsetForIndex(index, "start");
			if (!scrollElement || !offsetInfo) return;
			animateReleaseScrollTo(scrollElement, offsetInfo[0]);
		},
		[directoryVirtualizer],
	);
	const cancelPendingFocusRetry = useCallback(() => {
		pendingFocusScheduledIdRef.current = null;
		pendingFocusSeekStartedRef.current = null;
		pendingFocusGeometryRef.current = null;
		if (pendingFocusRetryTimerRef.current !== null) {
			window.clearTimeout(pendingFocusRetryTimerRef.current);
			pendingFocusRetryTimerRef.current = null;
		}
	}, []);
	const schedulePendingFocusRetry = useCallback(
		(targetId: string, index: number) => {
			if (
				pendingFocusScheduledIdRef.current === targetId &&
				pendingFocusRetryTimerRef.current !== null
			) {
				return;
			}
			cancelPendingFocusRetry();
			pendingFocusScheduledIdRef.current = targetId;
			if (pendingFocusRevealAppliedRef.current !== targetId) {
				pendingFocusRevealAppliedRef.current = null;
			}
			pendingFocusSeekStartedRef.current = null;
			pendingFocusGeometryRef.current = null;
			const startedAt = performance.now();
			const retry = () => {
				if (pendingFocusIdRef.current !== targetId) {
					pendingFocusScheduledIdRef.current = null;
					pendingFocusRetryTimerRef.current = null;
					return;
				}
				const scrollElement = detailScrollRef.current;
				if (!scrollElement) {
					pendingFocusRetryTimerRef.current = window.setTimeout(retry, 80);
					return;
				}
				const target = scrollElement.querySelector<HTMLElement>(
					`[data-release-id="${CSS.escape(targetId)}"]`,
				);
				if (target) {
					const geometry = pendingFocusGeometryRef.current;
					const height = target.getBoundingClientRect().height;
					const sameGeometry =
						geometry?.targetId === targetId &&
						geometry.height === height &&
						geometry.scrollHeight === scrollElement.scrollHeight;
					pendingFocusGeometryRef.current = {
						targetId,
						height,
						scrollHeight: scrollElement.scrollHeight,
						stableFrames: sameGeometry ? (geometry?.stableFrames ?? 0) + 1 : 0,
						revealCount: geometry?.revealCount ?? 0,
					};
				}
				const geometry = pendingFocusGeometryRef.current;
				const geometryStable = (geometry?.stableFrames ?? 0) >= 4;
				if (
					target &&
					geometryStable &&
					pendingFocusRevealAppliedRef.current !== targetId &&
					isVisibleEnough(target, scrollElement)
				) {
					pendingFocusRevealAppliedRef.current = targetId;
				}
				if (
					target &&
					pendingFocusRevealAppliedRef.current === targetId &&
					isVisibleEnough(target, scrollElement) &&
					!releaseScrollFrames.has(scrollElement)
				) {
					pendingFocusIdRef.current = null;
					pendingFocusScheduledIdRef.current = null;
					pendingFocusRetryTimerRef.current = null;
					routeFocusLockRef.current = null;
					return;
				}
				if (!releaseScrollFrames.has(scrollElement)) {
					if (target && geometryStable) {
						if (
							pendingFocusRevealAppliedRef.current !== targetId &&
							!isVisibleEnough(target, scrollElement)
						) {
							revealReleaseElement(target, scrollElement, {
								precedingLines: 3,
								forcePrecedingLines: true,
							});
							if (pendingFocusGeometryRef.current) {
								pendingFocusGeometryRef.current.revealCount += 1;
							}
							pendingFocusRevealAppliedRef.current = targetId;
						} else if (
							pendingFocusRevealAppliedRef.current === targetId &&
							!isVisibleEnough(target, scrollElement) &&
							(geometry?.revealCount ?? 0) < 3
						) {
							pendingFocusRevealAppliedRef.current = null;
						}
					} else if (
						!target &&
						pendingFocusSeekStartedRef.current !== targetId
					) {
						// The virtualizer still needs time to mount the estimated target.
						pendingFocusSeekStartedRef.current = targetId;
						scrollDetailToIndex(index, { preflight: true });
					}
				}
				if (performance.now() - startedAt >= 5_000) {
					pendingFocusIdRef.current = null;
					pendingFocusScheduledIdRef.current = null;
					pendingFocusRetryTimerRef.current = null;
					routeFocusLockRef.current = null;
					return;
				}
				pendingFocusRetryTimerRef.current = window.setTimeout(retry, 80);
			};
			retry();
		},
		[cancelPendingFocusRetry, detailVirtualizer, scrollDetailToIndex],
	);
	const cancelDirectoryFocusRetry = useCallback(() => {
		pendingDirectoryFocusIdRef.current = null;
		if (pendingDirectoryFocusRetryTimerRef.current !== null) {
			window.clearTimeout(pendingDirectoryFocusRetryTimerRef.current);
			pendingDirectoryFocusRetryTimerRef.current = null;
		}
	}, []);
	const scheduleDirectoryFocusRetry = useCallback(
		(targetId: string, index: number) => {
			cancelDirectoryFocusRetry();
			pendingDirectoryFocusIdRef.current = targetId;
			const startedAt = performance.now();
			const retry = () => {
				if (pendingDirectoryFocusIdRef.current !== targetId) {
					pendingDirectoryFocusRetryTimerRef.current = null;
					return;
				}
				const scrollElement = directoryScrollRef.current;
				if (!scrollElement) {
					pendingDirectoryFocusRetryTimerRef.current = window.setTimeout(
						retry,
						80,
					);
					return;
				}
				const target = scrollElement.querySelector<HTMLElement>(
					`[data-release-directory-id="${CSS.escape(targetId)}"]`,
				);
				if (target && isVisibleEnough(target, scrollElement)) {
					releaseScrollGuards.delete(scrollElement);
					cancelDirectoryFocusRetry();
					return;
				}
				if (!releaseScrollFrames.has(scrollElement)) {
					if (target) revealReleaseElement(target, scrollElement);
					else scrollDirectoryToIndex(index);
				}
				if (performance.now() - startedAt >= 5_000) {
					cancelDirectoryFocusRetry();
					return;
				}
				pendingDirectoryFocusRetryTimerRef.current = window.setTimeout(
					retry,
					80,
				);
			};
			retry();
		},
		[cancelDirectoryFocusRetry, scrollDirectoryToIndex],
	);
	const cancelSavedDirectoryRestore = useCallback(() => {
		directoryRestoreGenerationRef.current += 1;
		if (directoryRestoreTimerRef.current !== null) {
			window.clearTimeout(directoryRestoreTimerRef.current);
			directoryRestoreTimerRef.current = null;
		}
	}, []);
	const cancelFocusedLanePreservation = useCallback(() => {
		focusedLanePreserveGenerationRef.current += 1;
		if (focusedLanePreserveFrameRef.current !== null) {
			window.cancelAnimationFrame(focusedLanePreserveFrameRef.current);
			focusedLanePreserveFrameRef.current = null;
		}
		focusedLanePreserveRef.current = null;
		focusedLaneTransactionRef.current = null;
		if (detailScrollRef.current) {
			detailScrollRef.current.style.scrollBehavior = "";
		}
	}, []);
	const cancelFocusTransaction = useCallback(() => {
		routeFocusSuppressedRef.current = true;
		routeFocusLockRef.current = null;
		pendingFocusIdRef.current = null;
		pendingDomFocusIdRef.current = null;
		programmaticDomFocusRef.current = null;
		pendingFocusRevealAppliedRef.current = null;
		cancelPendingFocusRetry();
		cancelDirectoryFocusRetry();
		if (routeFocusFrameRef.current !== null) {
			window.cancelAnimationFrame(routeFocusFrameRef.current);
			routeFocusFrameRef.current = null;
		}
		programmaticUntilRef.current = 0;
		setPulseReleaseId(null);
		cancelSavedDirectoryRestore();
		cancelFocusedLanePreservation();
		cancelSmoothScroll(detailScrollRef.current);
		cancelSmoothScroll(directoryScrollRef.current);
	}, [
		cancelDirectoryFocusRetry,
		cancelPendingFocusRetry,
		cancelFocusedLanePreservation,
		cancelSavedDirectoryRestore,
	]);
	useEffect(
		() => () => {
			cancelPendingFocusRetry();
			cancelDirectoryFocusRetry();
		},
		[cancelDirectoryFocusRetry, cancelPendingFocusRetry],
	);
	useEffect(
		() => () => {
			cancelFocusedLanePreservation();
		},
		[cancelFocusedLanePreservation],
	);
	const virtualItems = detailVirtualizer.getVirtualItems();
	const directoryVirtualItems = directoryVirtualizer.getVirtualItems();
	const visibleReleaseIds = virtualItems
		.map((virtualItem) => rows[virtualItem.index])
		.filter(
			(row): row is Extract<VirtualRow, { kind: "release" }> =>
				row?.kind === "release",
		)
		.map((row) => row.item.release_id);
	const visibleReleaseSignature = visibleReleaseIds.join(",");

	// Reconcile list data and pagination changes against the first mounted row.
	useLayoutEffect(() => {
		const detailElement = detailScrollRef.current;
		const routeChanged = routeKeyRef.current !== routeKey;
		if (
			detailElement &&
			detailAnchorRef.current &&
			!routeChanged &&
			pendingFocusIdRef.current === null &&
			routeFocusLockRef.current === null &&
			performance.now() >= programmaticUntilRef.current &&
			!releaseScrollFrames.has(detailElement)
		) {
			const anchor = detailElement.querySelector<HTMLElement>(
				`[data-release-id="${CSS.escape(detailAnchorRef.current.id)}"]`,
			);
			if (anchor) {
				const viewport = detailElement.getBoundingClientRect();
				const nextOffset = anchor.getBoundingClientRect().top - viewport.top;
				detailElement.scrollTop += nextOffset - detailAnchorRef.current.offset;
			}
		}
		if (detailElement) {
			const first =
				detailElement.querySelector<HTMLElement>("[data-release-id]");
			if (first) {
				detailAnchorRef.current = {
					id: first.dataset.releaseId ?? "",
					offset:
						first.getBoundingClientRect().top -
						detailElement.getBoundingClientRect().top,
				};
			}
		}
		const directoryElement = directoryScrollRef.current;
		if (directoryElement && directoryAnchorRef.current && !routeChanged) {
			const anchor = directoryElement.querySelector<HTMLElement>(
				`[data-release-directory-id="${CSS.escape(directoryAnchorRef.current.id)}"]`,
			);
			if (anchor) {
				const viewport = directoryElement.getBoundingClientRect();
				const nextOffset = anchor.getBoundingClientRect().top - viewport.top;
				directoryElement.scrollTop +=
					nextOffset - directoryAnchorRef.current.offset;
			}
		}
		if (directoryElement) {
			const first = directoryElement.querySelector<HTMLElement>(
				"[data-release-directory-id]",
			);
			if (first) {
				directoryAnchorRef.current = {
					id: first.dataset.releaseDirectoryId ?? "",
					offset:
						first.getBoundingClientRect().top -
						directoryElement.getBoundingClientRect().top,
				};
			}
		}
	}, [props.items, routeKey, rows.length]);

	// Lane changes remeasure every mounted card. Keep the current release mounted
	// and reconcile its viewport offset until the new card geometry settles.
	useLayoutEffect(() => {
		const detailElement = detailScrollRef.current;
		const routeChanged = routeKeyRef.current !== routeKey;
		if (routeChanged) {
			cancelFocusedLanePreservation();
			focusedLaneAnchorRef.current = null;
			return;
		}
		if (!detailElement || !focusedReleaseId) return;
		const target = detailElement.querySelector<HTMLElement>(
			`[data-release-id="${CSS.escape(focusedReleaseId)}"]`,
		);
		if (!target) return;
		const previous = focusedLaneAnchorRef.current;
		const laneChanged =
			previous?.id === focusedReleaseId && previous.lane !== props.selectedLane;
		if (laneChanged && previous) {
			cancelFocusedLanePreservation();
			focusedLaneTransactionRef.current = focusedReleaseId;
			const generation = focusedLanePreserveGenerationRef.current;
			const preservation = {
				id: focusedReleaseId,
				desiredOffset: previous.offset,
				startedAt: performance.now(),
				stableFrames: 0,
				lastOffset: null as number | null,
				lastHeight: null as number | null,
			};
			focusedLanePreserveRef.current = preservation;
			const reconcile = () => {
				if (
					focusedLanePreserveGenerationRef.current !== generation ||
					focusedLanePreserveRef.current !== preservation
				) {
					return;
				}
				const scroll = detailScrollRef.current;
				const currentTarget = scroll?.querySelector<HTMLElement>(
					`[data-release-id="${CSS.escape(preservation.id)}"]`,
				);
				if (scroll && currentTarget) {
					const viewport = scroll.getBoundingClientRect();
					const currentOffset =
						currentTarget.getBoundingClientRect().top - viewport.top;
					const delta = currentOffset - preservation.desiredOffset;
					if (Math.abs(delta) > 0.5) {
						scroll.style.scrollBehavior = "auto";
						scroll.scrollTop += delta;
						preservation.stableFrames = 0;
					}
					const committedOffset =
						currentTarget.getBoundingClientRect().top - viewport.top;
					const height = currentTarget.getBoundingClientRect().height;
					const geometryStable =
						preservation.lastOffset !== null &&
						Math.abs(committedOffset - preservation.lastOffset) <= 0.5 &&
						preservation.lastHeight === height;
					preservation.stableFrames = geometryStable
						? preservation.stableFrames + 1
						: 0;
					preservation.lastOffset = committedOffset;
					preservation.lastHeight = height;
					focusedLaneAnchorRef.current = {
						id: preservation.id,
						lane: props.selectedLane,
						offset: committedOffset,
					};
				}
				if (
					preservation.stableFrames >= 6 ||
					performance.now() - preservation.startedAt >= 1_800
				) {
					cancelFocusedLanePreservation();
					return;
				}
				focusedLanePreserveFrameRef.current =
					window.requestAnimationFrame(reconcile);
			};
			focusedLanePreserveFrameRef.current =
				window.requestAnimationFrame(reconcile);
		}
		const viewport = detailElement.getBoundingClientRect();
		focusedLaneAnchorRef.current = {
			id: focusedReleaseId,
			lane: props.selectedLane,
			offset: target.getBoundingClientRect().top - viewport.top,
		};
	}, [
		cancelFocusedLanePreservation,
		focusedReleaseId,
		props.selectedLane,
		routeKey,
		visibleReleaseSignature,
	]);

	useEffect(() => {
		if (props.selectedLane !== "translated" || !visibleReleaseSignature) return;
		const ids = visibleReleaseIds.filter(
			(id) => !hydratedTranslatedRef.current.has(id),
		);
		if (ids.length === 0) return;
		for (const id of ids) hydratedTranslatedRef.current.add(id);
		void apiGetPublicRepoReleaseContent({
			owner: props.owner,
			repo: props.repo,
			release_ids: ids.slice(0, 30),
			content: "translated",
		})
			.then((response) =>
				props.onHydrateItems(
					response.items.map((item) => ({
						release_id: item.release_id,
						translated: item.translated,
					})),
				),
			)
			.catch(() => {
				for (const id of ids) hydratedTranslatedRef.current.delete(id);
			});
	}, [
		props.onHydrateItems,
		props.owner,
		props.repo,
		props.selectedLane,
		visibleReleaseIds,
		visibleReleaseSignature,
	]);

	const updateTagUrl = useCallback(
		(releaseId: string) => {
			if (!props.tag) return;
			const item = props.items.find(
				(candidate) => candidate.release_id === releaseId,
			);
			if (!item || item.tag_name === props.tag) return;
			naturalTagRef.current = item.tag_name;
			const url = new URL(window.location.href);
			const path = `${publicReleasePathPrefix()}/public/${encodeURIComponent(props.owner)}/${encodeURIComponent(props.repo)}/releases/tag/${encodeURIComponent(item.tag_name)}`;
			const history = router.history as typeof router.history & {
				_ignoreSubscribers?: boolean;
			};
			const ignored = history._ignoreSubscribers;
			history._ignoreSubscribers = true;
			try {
				window.history.replaceState(
					window.history.state,
					"",
					`${path}${url.search}`,
				);
			} finally {
				history._ignoreSubscribers = ignored;
			}
		},
		[props.items, props.owner, props.repo, props.tag, router],
	);

	const markCurrent = useCallback(
		(releaseId: string, { replaceUrl = true } = {}) => {
			setFocusedReleaseId((current) =>
				current === releaseId ? current : releaseId,
			);
			if (replaceUrl) updateTagUrl(releaseId);
		},
		[updateTagUrl],
	);
	const focusDirectoryRelease = useCallback(
		(releaseId: string) => {
			const directoryIndex = props.items.findIndex(
				(item) => item.release_id === releaseId,
			);
			if (directoryIndex < 0) return;
			const directoryElement = directoryScrollRef.current;
			const directoryTarget = directoryElement?.querySelector<HTMLElement>(
				`[data-release-directory-id="${CSS.escape(releaseId)}"]`,
			);
			if (
				directoryTarget &&
				isVisibleEnough(directoryTarget, directoryElement)
			) {
				if (directoryElement) cancelSmoothScroll(directoryElement);
				cancelDirectoryFocusRetry();
				return;
			}
			scheduleDirectoryFocusRetry(releaseId, directoryIndex);
		},
		[cancelDirectoryFocusRetry, props.items, scheduleDirectoryFocusRetry],
	);
	const focusCurrentRelease = useCallback(
		(releaseId: string) => {
			markCurrent(releaseId);
			focusDirectoryRelease(releaseId);
		},
		[focusDirectoryRelease, markCurrent],
	);
	const focusDetailInteraction = useCallback(
		(releaseId: string, { replaceUrl = false } = {}) => {
			cancelFocusTransaction();
			markCurrent(releaseId, { replaceUrl });
			focusDirectoryRelease(releaseId);
		},
		[cancelFocusTransaction, focusDirectoryRelease, markCurrent],
	);
	const focusHoveredRelease = useCallback(
		(releaseId: string) => {
			// A title click can cause the retained pointer position to enter a
			// different row after SPA navigation. Only a pointer move after the
			// click may replace the explicit route focus with hover focus.
			if (
				publicReleasePointerState.lastDetailPointerDown >
				publicReleasePointerState.lastDetailPointerMove
			) {
				return;
			}
			focusDetailInteraction(releaseId);
		},
		[focusDetailInteraction],
	);
	useEffect(
		() => () => {
			cancelSavedDirectoryRestore();
		},
		[cancelSavedDirectoryRestore, scrollPositionKey],
	);
	const restoreSavedDirectoryScroll = useCallback(() => {
		cancelSavedDirectoryRestore();
		const generation = directoryRestoreGenerationRef.current;
		let stableAttempts = 0;
		let observedSaved: { directory: number } | null = null;
		const restore = () => {
			if (directoryRestoreGenerationRef.current !== generation) return;
			const saved = releaseScrollPositions.get(scrollPositionKey);
			if (!saved) {
				directoryRestoreTimerRef.current = null;
				return;
			}
			if (saved !== observedSaved) {
				observedSaved = saved;
				stableAttempts = 0;
			}
			const element = directoryScrollRef.current;
			if (!element || element.clientHeight === 0) {
				directoryRestoreTimerRef.current = window.setTimeout(restore, 50);
				return;
			}
			cancelSmoothScroll(element);
			const previousBehavior = element.style.scrollBehavior;
			element.style.scrollBehavior = "auto";
			element.scrollTop = saved.directory;
			element.style.scrollBehavior = previousBehavior;
			if (Math.abs(element.scrollTop - saved.directory) < 1) {
				stableAttempts += 1;
				if (stableAttempts >= 8) {
					if (releaseScrollPositions.get(scrollPositionKey) === saved) {
						releaseScrollPositions.delete(scrollPositionKey);
					}
					directoryRestoreTimerRef.current = null;
					return;
				}
			} else {
				stableAttempts = 0;
			}
			directoryRestoreTimerRef.current = window.setTimeout(restore, 50);
		};
		restore();
	}, [cancelSavedDirectoryRestore, scrollPositionKey]);

	useLayoutEffect(() => {
		if (!releaseScrollPositions.has(scrollPositionKey)) return;
		restoreSavedDirectoryScroll();
	}, [props.items, restoreSavedDirectoryScroll, routeKey, scrollPositionKey]);

	const focusRelease = useCallback(
		(releaseId: string, { moveDomFocus = false } = {}) => {
			if (detailScrollRef.current) {
				cancelSmoothScroll(detailScrollRef.current);
				releaseScrollGuards.delete(detailScrollRef.current);
			}
			if (directoryScrollRef.current) {
				cancelSmoothScroll(directoryScrollRef.current);
				releaseScrollGuards.delete(directoryScrollRef.current);
			}
			cancelPendingFocusRetry();
			pendingFocusIdRef.current = null;
			pendingFocusRevealAppliedRef.current = null;
			routeFocusLockRef.current = releaseId;
			pendingDomFocusIdRef.current = moveDomFocus ? releaseId : null;
			markCurrent(releaseId, { replaceUrl: false });
			const index = rows.findIndex(
				(row) => row.kind === "release" && row.item.release_id === releaseId,
			);
			if (index < 0) {
				pendingDomFocusIdRef.current = null;
				return;
			}
			programmaticUntilRef.current = performance.now() + 5_000;
			setPulseReleaseId(releaseId);
			window.setTimeout(
				() =>
					setPulseReleaseId((current) =>
						current === releaseId ? null : current,
					),
				3_000,
			);
			// Keep the target locked through a short geometry-stability window even
			// when it is initially visible. Virtualized rows can still settle after
			// this callback and otherwise move the target out of the viewport.
			pendingFocusIdRef.current = releaseId;
			schedulePendingFocusRetry(releaseId, index);
			const preserveDirectoryPosition =
				releaseScrollPositions.has(scrollPositionKey);
			if (!preserveDirectoryPosition) focusDirectoryRelease(releaseId);
			else cancelDirectoryFocusRetry();
			restoreSavedDirectoryScroll();
		},
		[
			detailVirtualizer,
			directoryVirtualizer,
			focusDirectoryRelease,
			markCurrent,
			props.items,
			restoreSavedDirectoryScroll,
			scrollPositionKey,
			rows,
			cancelPendingFocusRetry,
			cancelDirectoryFocusRetry,
			schedulePendingFocusRetry,
			scrollDirectoryToIndex,
			scrollDetailToIndex,
		],
	);
	const cancelDetailInteraction = useCallback(() => {
		cancelFocusTransaction();
		naturalTagRef.current = null;
		cancelSavedDirectoryRestore();
	}, [cancelFocusTransaction, cancelSavedDirectoryRestore]);
	const cancelDirectoryInteraction = useCallback(() => {
		cancelFocusTransaction();
		naturalTagRef.current = null;
		cancelSavedDirectoryRestore();
	}, [cancelFocusTransaction, cancelSavedDirectoryRestore]);
	const handleDetailPointerDown = useCallback(() => {
		publicReleasePointerState.lastDetailPointerDown = performance.now();
		cancelDetailInteraction();
	}, [cancelDetailInteraction]);
	const handleDetailPointerMove = useCallback(() => {
		publicReleasePointerState.lastDetailPointerMove = performance.now();
	}, []);

	useEffect(() => {
		const routeChanged = routeKeyRef.current !== routeKey;
		if (routeChanged) {
			if (routeFocusFrameRef.current !== null) {
				window.cancelAnimationFrame(routeFocusFrameRef.current);
				routeFocusFrameRef.current = null;
			}
			routeKeyRef.current = routeKey;
			routeFocusSuppressedRef.current = false;
			routeFocusLockRef.current = null;
			focusSignatureRef.current = null;
		}
		const targetId =
			props.highlight?.active_release_id ??
			(props.tag
				? props.items.find((item) => item.tag_name === props.tag)?.release_id
				: undefined);
		if (!targetId) {
			focusSignatureRef.current = props.tag ? null : "list";
			return;
		}
		const signature = `${props.tag ?? "list"}:${targetId}`;
		if (focusSignatureRef.current === signature) return;
		if (routeChanged && naturalTagRef.current === props.tag) {
			naturalTagRef.current = null;
			routeFocusSuppressedRef.current = true;
			focusSignatureRef.current = signature;
			return;
		}
		if (routeFocusSuppressedRef.current) {
			focusSignatureRef.current = signature;
			return;
		}
		if (
			!rows.some(
				(row) => row.kind === "release" && row.item.release_id === targetId,
			)
		)
			return;
		focusSignatureRef.current = signature;
		const frame = window.requestAnimationFrame(() => {
			routeFocusFrameRef.current = null;
			if (
				focusSignatureRef.current !== signature ||
				routeKeyRef.current !== routeKey
			) {
				return;
			}
			focusRelease(targetId);
		});
		routeFocusFrameRef.current = frame;
	}, [
		focusRelease,
		props.highlight?.active_release_id,
		props.items,
		props.tag,
		routeKey,
		rows,
		rows.length,
	]);

	useLayoutEffect(() => {
		const targetId = pendingFocusIdRef.current;
		if (!targetId || !detailScrollRef.current) return;
		const virtualItem = virtualItems.find((item) => {
			const row = rows[item.index];
			return row?.kind === "release" && row.item.release_id === targetId;
		});
		if (!virtualItem) return;
		schedulePendingFocusRetry(targetId, virtualItem.index);
	}, [rows, schedulePendingFocusRetry, virtualItems]);

	useLayoutEffect(() => {
		const targetId = pendingDomFocusIdRef.current;
		if (!targetId || !detailScrollRef.current) return;
		const target = detailScrollRef.current.querySelector<HTMLElement>(
			`[data-release-id="${CSS.escape(targetId)}"]`,
		);
		if (target?.dataset.currentRelease !== "true") return;
		programmaticDomFocusRef.current = targetId;
		target.focus({ preventScroll: true });
		pendingDomFocusIdRef.current = null;
	}, [focusedReleaseId, rows, virtualItems]);

	useLayoutEffect(() => {
		const targetId = pendingDirectoryFocusIdRef.current;
		if (!targetId || !directoryScrollRef.current) return;
		const virtualItem = directoryVirtualItems.find(
			(item) => props.items[item.index]?.release_id === targetId,
		);
		if (!virtualItem) return;
		scheduleDirectoryFocusRetry(targetId, virtualItem.index);
	}, [directoryVirtualItems, props.items, scheduleDirectoryFocusRetry]);

	useLayoutEffect(() => {
		const targetId = routeFocusLockRef.current;
		const scrollElement = detailScrollRef.current;
		if (!targetId || !scrollElement) return;
		const routeTargetId =
			props.highlight?.active_release_id ??
			(props.tag
				? props.items.find((item) => item.tag_name === props.tag)?.release_id
				: undefined);
		if (routeTargetId !== targetId) return;
		if (pendingFocusIdRef.current === targetId) return;
		if (pendingFocusRevealAppliedRef.current === targetId) return;
		if (releaseScrollFrames.has(scrollElement)) return;
		const index = rows.findIndex(
			(row) => row.kind === "release" && row.item.release_id === targetId,
		);
		if (index < 0) return;
		const target = scrollElement.querySelector<HTMLElement>(
			`[data-release-id="${CSS.escape(targetId)}"]`,
		);
		if (target) {
			const visible = isVisibleEnough(target, scrollElement);
			if (!visible) {
				revealReleaseElement(target, scrollElement, { precedingLines: 3 });
			}
			return;
		}
		scrollDetailToIndex(index, { preflight: true });
	}, [
		props.highlight?.active_release_id,
		props.items,
		props.tag,
		rows,
		scrollDetailToIndex,
		virtualItems,
	]);

	useEffect(() => {
		const directoryElement = directoryScrollElement;
		if (
			!directoryElement ||
			props.highlight ||
			props.appendError ||
			props.loadingMore ||
			props.loadingNewer
		)
			return;
		if (directoryElement.scrollHeight > directoryElement.clientHeight + 480) {
			return;
		}
		if (directoryElement.scrollTop <= 1 && props.hasNewer) {
			void props.onLoadNewer();
			return;
		}
		if (props.hasMore) {
			void props.onLoadMore();
		}
	}, [
		directoryScrollElement,
		props.appendError,
		props.hasMore,
		props.hasNewer,
		props.items.length,
		props.loadingMore,
		props.loadingNewer,
		props.onLoadMore,
		props.onLoadNewer,
	]);

	const updateFromScroll = useCallback(() => {
		const scrollElement = detailScrollRef.current;
		if (
			!scrollElement ||
			routeFocusLockRef.current ||
			focusedLaneTransactionRef.current ||
			performance.now() < programmaticUntilRef.current
		)
			return;
		const viewport = scrollElement.getBoundingClientRect();
		const candidates = Array.from(
			scrollElement.querySelectorAll<HTMLElement>("[data-release-id]"),
		)
			.map((element) => ({ element, rect: element.getBoundingClientRect() }))
			.filter(
				({ rect }) => rect.bottom > viewport.top && rect.top < viewport.bottom,
			);
		const full = candidates.find(
			({ rect }) => rect.top >= viewport.top && rect.bottom <= viewport.bottom,
		);
		const crossing = candidates.find(
			({ rect }) =>
				rect.top <= viewport.top + viewport.height * 0.3 &&
				rect.bottom >= viewport.top + viewport.height * 0.3,
		);
		const active = full ?? crossing;
		const id = active?.element.dataset.releaseId;
		if (id) focusCurrentRelease(id);
	}, [focusCurrentRelease]);

	const handleDetailScroll = useCallback(() => {
		const scrollElement = detailScrollRef.current;
		const guard = scrollElement
			? releaseScrollGuards.get(scrollElement)
			: undefined;
		if (scrollElement && guard) {
			if (
				guard.direction === "forward" &&
				scrollElement.scrollTop < guard.extreme - 0.5
			) {
				scrollElement.style.scrollBehavior = "auto";
				scrollElement.scrollTop = guard.extreme;
			} else if (
				guard.direction === "backward" &&
				scrollElement.scrollTop > guard.extreme + 0.5
			) {
				scrollElement.style.scrollBehavior = "auto";
				scrollElement.scrollTop = guard.extreme;
			} else {
				guard.extreme =
					guard.direction === "forward"
						? Math.max(guard.extreme, scrollElement.scrollTop)
						: Math.min(guard.extreme, scrollElement.scrollTop);
			}
		}
		window.requestAnimationFrame(updateFromScroll);
	}, [updateFromScroll]);

	const detailHref = useCallback(
		(item: PublicReleaseListItem) => {
			const params = appendPublicReleaseHighlightParams(
				new URLSearchParams(),
				props.highlightSelection,
			);
			if (props.highlight) {
				const selector =
					props.highlight.resolved.find(
						(target) => target.release_id === item.release_id,
					)?.selector ?? `id:${item.release_id}`;
				params.set("highlight_active", selector);
			}
			const query = params.toString();
			const path = `${publicReleasePathPrefix()}/public/${encodeURIComponent(props.owner)}/${encodeURIComponent(props.repo)}/releases/tag/${encodeURIComponent(item.tag_name)}`;
			return query ? `${path}?${query}` : path;
		},
		[props.highlight, props.highlightSelection, props.owner, props.repo],
	);
	const preserveTimelineScroll = useCallback(() => {
		cancelSavedDirectoryRestore();
		releaseScrollPositions.set(scrollPositionKey, {
			directory: directoryScrollRef.current?.scrollTop ?? 0,
		});
	}, [cancelSavedDirectoryRestore, scrollPositionKey]);
	const detailSearch = useCallback(
		(item: PublicReleaseListItem) => {
			const search = publicReleaseHighlightSearch(props.highlightSelection);
			if (!props.highlight) return search;
			return {
				...search,
				highlight_active:
					props.highlight.resolved.find(
						(target) => target.release_id === item.release_id,
					)?.selector ?? `id:${item.release_id}`,
			};
		},
		[props.highlight, props.highlightSelection],
	);
	const navigationTargets = useMemo(() => {
		if (!props.highlight) return [];
		if (props.highlight.mode === "discrete") return props.highlight.resolved;
		return props.items
			.filter((item) => item.is_highlighted)
			.map((item, index) => ({
				selector: `id:${item.release_id}`,
				release_id: item.release_id,
				tag_name: item.tag_name,
				ordinal: index + 1,
			}));
	}, [props.highlight, props.items]);
	const activeTargetIndex = Math.max(
		0,
		navigationTargets.findIndex(
			(target) => target.release_id === props.highlight?.active_release_id,
		),
	);
	const activateHighlightTarget = useCallback(
		(target: { selector: string; release_id: string }) => {
			const url = new URL(window.location.href);
			url.searchParams.set("highlight_active", target.selector);
			window.history.replaceState(window.history.state, "", url);
			const navigationIndex = navigationTargets.findIndex(
				(candidate) => candidate.release_id === target.release_id,
			);
			const currentIndex = navigationTargets.findIndex(
				(candidate) =>
					candidate.release_id === props.highlight?.active_release_id,
			);
			const absoluteIndex =
				props.highlight?.mode === "range" &&
				props.highlight.active_index !== null &&
				currentIndex >= 0
					? props.highlight.active_index + (navigationIndex - currentIndex)
					: navigationIndex + 1;
			props.onActivateHighlight(
				target.release_id,
				Math.min(
					props.highlight?.total ?? absoluteIndex,
					Math.max(1, absoluteIndex),
				),
			);
			focusRelease(target.release_id, { moveDomFocus: true });
		},
		[
			focusRelease,
			navigationTargets,
			props.highlight,
			props.onActivateHighlight,
		],
	);

	if (props.items.length === 0) {
		return (
			<Card>
				<CardHeader>
					<CardTitle>还没有缓存到 Release</CardTitle>
					<CardDescription>
						仓库已同步完成，但当前共享缓存里没有可展示的 Release。
					</CardDescription>
				</CardHeader>
			</Card>
		);
	}

	return (
		<div className="space-y-3 sm:space-y-4" data-testid="public-release-reader">
			{props.highlight &&
			(props.highlight.unresolved.length > 0 || props.highlight.message) ? (
				<p
					className="font-mono text-xs text-muted-foreground"
					data-testid="public-release-highlight-unresolved"
					role="status"
				>
					{props.highlight.message ??
						`${props.highlight.unresolved.length} 个高亮目标暂时未找到`}
				</p>
			) : null}
			<div className="grid min-h-0 grid-cols-1 overflow-hidden rounded-2xl border border-border/70 bg-card/30 lg:h-[calc(100dvh-11rem)] lg:min-h-[32rem] lg:grid-cols-[minmax(15rem,18rem)_minmax(0,1fr)]">
				<nav
					className="hidden min-h-0 flex-col border-r border-border/70 bg-muted/15 lg:flex"
					aria-label="版本目录"
					data-testid="public-release-directory"
				>
					<div className="border-b border-border/60 px-4 py-3 font-mono text-xs uppercase tracking-wide text-muted-foreground">
						版本目录
					</div>
					<div
						ref={setDirectoryScrollNode}
						className="min-h-0 flex-1 overflow-y-auto scroll-smooth"
						data-testid="public-release-directory-scroll"
						onWheel={cancelDirectoryInteraction}
						onPointerDown={cancelDirectoryInteraction}
					>
						<AutoLoadSentinel
							root={directoryScrollElement}
							enabled={
								props.hasNewer && !props.loadingNewer && !props.appendError
							}
							onVisible={props.onLoadNewer}
						/>
						<div
							className="relative w-full"
							style={{ height: `${directoryVirtualizer.getTotalSize()}px` }}
							data-release-count={props.items.length}
							data-testid="public-release-directory-virtual-list"
						>
							{directoryVirtualizer.getVirtualItems().map((virtualItem) => {
								const item = props.items[virtualItem.index];
								if (!item) return null;
								const active = item.release_id === focusedReleaseId;
								return (
									<InternalLink
										key={virtualItem.key}
										href={detailHref(item)}
										to="/public/$owner/$repo/releases/tag/$tag"
										params={{
											owner: props.owner,
											repo: props.repo,
											tag: item.tag_name,
										}}
										search={detailSearch(item)}
										resetScroll={false}
										onPointerDown={preserveTimelineScroll}
										onClick={preserveTimelineScroll}
										aria-current={active ? "page" : undefined}
										data-release-directory-id={item.release_id}
										className={cn(
											"absolute top-0 left-0 w-full border-l-2 px-4 py-3 transition-colors",
											active
												? "border-primary bg-background text-foreground"
												: "border-transparent text-muted-foreground hover:bg-background/70 hover:text-foreground",
										)}
										style={{ transform: `translateY(${virtualItem.start}px)` }}
									>
										<span className="block truncate font-mono text-sm font-medium">
											{item.tag_name}
										</span>
										<span className="mt-1 block truncate text-xs">
											{releaseTitle(item)}
										</span>
									</InternalLink>
								);
							})}
						</div>
						<AutoLoadSentinel
							root={directoryScrollElement}
							enabled={
								props.hasMore && !props.loadingMore && !props.appendError
							}
							onVisible={props.onLoadMore}
						/>
					</div>
				</nav>
				<section className="min-h-0" aria-label="版本详情时间线">
					<section
						ref={setDetailScrollNode}
						aria-label="版本详情滚动区域"
						className="public-release-virtual-scroll h-[min(72dvh,880px)] overflow-y-auto overscroll-contain scroll-smooth px-3 py-4 sm:px-5 sm:py-5 lg:h-full"
						data-testid="public-release-detail-scroll"
						onScroll={handleDetailScroll}
						onWheel={cancelDetailInteraction}
						onPointerDown={handleDetailPointerDown}
						onPointerMove={handleDetailPointerMove}
						onKeyDown={cancelDetailInteraction}
						onFocusCapture={(event) => {
							const id = (event.target as HTMLElement).closest<HTMLElement>(
								"[data-release-id]",
							)?.dataset.releaseId;
							if (!id) return;
							if (programmaticDomFocusRef.current === id) {
								programmaticDomFocusRef.current = null;
								return;
							}
							focusDetailInteraction(id, { replaceUrl: true });
						}}
					>
						<AutoLoadSentinel
							root={detailScrollElement}
							enabled={
								props.hasNewer && !props.loadingNewer && !props.appendError
							}
							onVisible={props.onLoadNewer}
						/>
						<div
							className="relative w-full"
							style={{ height: `${detailVirtualizer.getTotalSize()}px` }}
							data-release-count={props.items.length}
							data-testid="public-release-virtual-list"
						>
							{virtualItems.map((virtualItem) => {
								const row = rows[virtualItem.index];
								if (!row) return null;
								return (
									<div
										key={virtualItem.key}
										ref={detailVirtualizer.measureElement}
										data-index={virtualItem.index}
										className="absolute top-0 left-0 w-full pb-3 sm:pb-4"
										style={{ transform: `translateY(${virtualItem.start}px)` }}
									>
										{row.kind === "gap" ? (
											<GapLoader
												gap={row.gap}
												loading={props.loadingGap === row.gap.newer_cursor}
												onVisible={props.onLoadGap}
												root={detailScrollElement}
											/>
										) : (
											<ReleaseVirtualRow
												owner={props.owner}
												repo={props.repo}
												item={row.item}
												lane={props.selectedLane}
												detailHref={detailHref(row.item)}
												reactionControls={props.reactionControls}
												hasHighlightContext={Boolean(props.highlight)}
												isCurrent={row.item.release_id === focusedReleaseId}
												isPulse={row.item.release_id === pulseReleaseId}
												onFocusCurrent={() =>
													focusDetailInteraction(row.item.release_id, {
														replaceUrl: true,
													})
												}
												onHoverCurrent={() =>
													focusHoveredRelease(row.item.release_id)
												}
												onTitleActivate={() =>
													focusDetailInteraction(row.item.release_id)
												}
												titleSearch={detailSearch(row.item)}
											/>
										)}
									</div>
								);
							})}
						</div>
						<AutoLoadSentinel
							root={detailScrollElement}
							enabled={
								props.hasMore && !props.loadingMore && !props.appendError
							}
							onVisible={props.onLoadMore}
						/>
						{props.loadingMore ? (
							<p className="font-mono text-xs text-muted-foreground">
								加载中...
							</p>
						) : null}
						{props.appendError ? (
							<div className="flex justify-center pt-1">
								<Button
									type="button"
									variant="outline"
									size="sm"
									className="font-mono text-xs"
									onClick={props.onLoadMore}
								>
									继续加载
								</Button>
							</div>
						) : null}
						{props.hasMore && !props.loadingMore && !props.appendError ? (
							<div className="flex justify-center pt-1">
								<Button
									type="button"
									variant="ghost"
									size="sm"
									className="font-mono text-xs"
									onClick={props.onLoadMore}
								>
									更多
								</Button>
							</div>
						) : null}
					</section>
				</section>
			</div>
			{props.highlight && props.highlight.total > 0 ? (
				<nav
					className="fixed right-4 z-30 flex items-center gap-1 rounded-xl border bg-background/95 p-1 shadow-sm supports-[backdrop-filter]:backdrop-blur-sm"
					style={{ bottom: "calc(env(safe-area-inset-bottom) + 1rem)" }}
					aria-label="高亮记录导航"
					data-testid="public-release-highlight-navigation"
				>
					<Button
						type="button"
						variant="ghost"
						size="icon"
						title="上一条高亮记录"
						disabled={activeTargetIndex <= 0}
						onClick={() => {
							const target = navigationTargets[activeTargetIndex - 1];
							if (target) activateHighlightTarget(target);
						}}
					>
						<ChevronUp className="size-4" />
					</Button>
					<span className="min-w-16 text-center font-mono text-xs tabular-nums">
						{props.highlight.active_index ?? activeTargetIndex + 1} /{" "}
						{props.highlight.total}
					</span>
					<Button
						type="button"
						variant="ghost"
						size="icon"
						title="下一条高亮记录"
						disabled={
							activeTargetIndex >= navigationTargets.length - 1 &&
							!props.hasMore
						}
						onClick={() => {
							const target = navigationTargets[activeTargetIndex + 1];
							if (target) activateHighlightTarget(target);
							else void props.onLoadMore();
						}}
					>
						<ChevronDown className="size-4" />
					</Button>
				</nav>
			) : null}
		</div>
	);
}

function _ReleaseList(props: {
	owner: string;
	repo: string;
	items: PublicReleaseListItem[];
	highlight?: PublicReleaseHighlight;
	gaps?: PublicReleaseGap[];
	highlightSelection: PublicReleaseHighlightSelection;
	selectedLane: FeedLane;
	reactionControls: PublicReleaseReactionControls;
	hasMore: boolean;
	hasNewer: boolean;
	loadingMore: boolean;
	loadingNewer: boolean;
	loadingGap: string | null;
	appendError: string | null;
	onLoadMore: () => Promise<void>;
	onLoadNewer: () => Promise<void>;
	onLoadGap: (gap: PublicReleaseGap) => Promise<void>;
	onHydrateItems: (
		items: Array<Pick<PublicReleaseListItem, "release_id" | "translated">>,
	) => void;
	onActivateHighlight: (releaseId: string, index: number) => void;
}) {
	const listRef = useRef<HTMLDivElement | null>(null);
	const focusedHighlightSignatureRef = useRef<string | null>(null);
	const hydratedTranslatedRef = useRef(new Set<string>());

	type VirtualRow =
		| { kind: "release"; item: PublicReleaseListItem }
		| { kind: "gap"; gap: PublicReleaseGap };

	const rows = useMemo<VirtualRow[]>(() => {
		const gapByNewerId = new Map<string, PublicReleaseGap>();
		for (const gap of props.gaps ?? []) {
			gapByNewerId.set(gap.newer_cursor.split("|").at(-1) ?? "", gap);
		}
		const result: VirtualRow[] = [];
		for (const item of props.items) {
			result.push({ kind: "release", item });
			const gap = gapByNewerId.get(item.release_id);
			if (gap) result.push({ kind: "gap", gap });
		}
		return result;
	}, [props.gaps, props.items]);

	const virtualizer = useWindowVirtualizer({
		count: rows.length,
		estimateSize: (index) => (rows[index]?.kind === "gap" ? 72 : 420),
		overscan: 4,
		scrollMargin: listRef.current?.offsetTop ?? 0,
		getItemKey: (index) => {
			const row = rows[index];
			return row?.kind === "release"
				? `release-${row.item.release_id}`
				: `gap-${row?.gap.newer_cursor ?? index}`;
		},
	});

	const virtualItems = virtualizer.getVirtualItems();
	const visibleReleaseIds = virtualItems
		.map((virtualItem) => rows[virtualItem.index])
		.filter(
			(row): row is Extract<VirtualRow, { kind: "release" }> =>
				row?.kind === "release",
		)
		.map((row) => row.item.release_id);
	const visibleReleaseSignature = visibleReleaseIds.join(",");

	useEffect(() => {
		if (props.selectedLane !== "translated" || !visibleReleaseSignature) return;
		const ids = visibleReleaseIds.filter(
			(id) => !hydratedTranslatedRef.current.has(id),
		);
		if (ids.length === 0) return;
		for (const id of ids) hydratedTranslatedRef.current.add(id);
		void apiGetPublicRepoReleaseContent({
			owner: props.owner,
			repo: props.repo,
			release_ids: ids.slice(0, 30),
			content: "translated",
		})
			.then((response) =>
				props.onHydrateItems(
					response.items.map((item) => ({
						release_id: item.release_id,
						translated: item.translated,
					})),
				),
			)
			.catch(() => {
				for (const id of ids) hydratedTranslatedRef.current.delete(id);
			});
	}, [
		props.onHydrateItems,
		props.owner,
		props.repo,
		props.selectedLane,
		visibleReleaseIds,
		visibleReleaseSignature,
	]);

	useLayoutEffect(() => {
		if (!props.highlight || props.items.length === 0) return;
		const signature = [
			props.highlight.mode,
			...props.highlight.requested,
			props.highlight.active_release_id ?? "",
		].join(":");
		if (focusedHighlightSignatureRef.current === signature) return;
		const activeId =
			props.highlight.active_release_id ??
			props.items.find((item) => item.is_highlighted)?.release_id;
		const index = rows.findIndex(
			(row) => row.kind === "release" && row.item.release_id === activeId,
		);
		if (index < 0) return;
		virtualizer.scrollToIndex(index, { align: "start", behavior: "auto" });
		focusedHighlightSignatureRef.current = signature;
	}, [props.highlight, props.items, rows, virtualizer]);

	const replaceActiveInUrl = useCallback((selector: string) => {
		const url = new URL(window.location.href);
		url.searchParams.set("highlight_active", selector);
		window.history.replaceState(window.history.state, "", url);
	}, []);

	const activateTarget = useCallback(
		(target: { selector: string; release_id: string }, focus: boolean) => {
			replaceActiveInUrl(target.selector);
			const targetIds =
				props.highlight?.mode === "discrete"
					? props.highlight.resolved.map((candidate) => candidate.release_id)
					: props.items
							.filter((item) => item.is_highlighted)
							.map((item) => item.release_id);
			const navigationIndex = targetIds.indexOf(target.release_id);
			const currentNavigationIndex = targetIds.indexOf(
				props.highlight?.active_release_id ?? "",
			);
			const absoluteIndex =
				props.highlight?.mode === "range" &&
				props.highlight.active_index !== null &&
				currentNavigationIndex >= 0
					? props.highlight.active_index +
						(navigationIndex - currentNavigationIndex)
					: navigationIndex + 1;
			props.onActivateHighlight(
				target.release_id,
				Math.min(
					props.highlight?.total ?? absoluteIndex,
					Math.max(1, absoluteIndex),
				),
			);
			const index = rows.findIndex(
				(row) =>
					row.kind === "release" && row.item.release_id === target.release_id,
			);
			if (index < 0) return;
			virtualizer.scrollToIndex(index, { align: "start", behavior: "auto" });
			if (focus) {
				window.requestAnimationFrame(() => {
					document
						.querySelector<HTMLElement>(
							`[data-release-id="${CSS.escape(target.release_id)}"]`,
						)
						?.focus({ preventScroll: true });
				});
			}
		},
		[
			props.highlight,
			props.items,
			props.onActivateHighlight,
			replaceActiveInUrl,
			rows,
			virtualizer,
		],
	);

	const navigationTargets = useMemo(() => {
		if (!props.highlight) return [];
		if (props.highlight.mode === "discrete") return props.highlight.resolved;
		return props.items
			.filter((item) => item.is_highlighted)
			.map((item, index) => ({
				selector: `id:${item.release_id}`,
				release_id: item.release_id,
				tag_name: item.tag_name,
				ordinal: index + 1,
			}));
	}, [props.highlight, props.items]);
	const activeTargetIndex = Math.max(
		0,
		navigationTargets.findIndex(
			(target) => target.release_id === props.highlight?.active_release_id,
		),
	);

	const detailHref = useCallback(
		(item: PublicReleaseListItem) => {
			const params = appendPublicReleaseHighlightParams(
				new URLSearchParams(),
				props.highlightSelection,
			);
			if (props.highlight) {
				const selector =
					props.highlight.resolved.find(
						(target) => target.release_id === item.release_id,
					)?.selector ?? `id:${item.release_id}`;
				params.set("highlight_active", selector);
			}
			const query = params.toString();
			const path = `/public/${encodeURIComponent(props.owner)}/${encodeURIComponent(props.repo)}/releases/tag/${encodeURIComponent(item.tag_name)}`;
			return query ? `${path}?${query}` : path;
		},
		[props.highlight, props.highlightSelection, props.owner, props.repo],
	);

	if (props.items.length === 0) {
		return (
			<Card>
				<CardHeader>
					<CardTitle>还没有缓存到 Release</CardTitle>
					<CardDescription>
						仓库已同步完成，但当前共享缓存里没有可展示的 Release。
					</CardDescription>
				</CardHeader>
			</Card>
		);
	}

	return (
		<div className="space-y-3 sm:space-y-4">
			<AutoLoadSentinel
				enabled={props.hasNewer && !props.loadingNewer && !props.appendError}
				onVisible={props.onLoadNewer}
			/>
			{props.highlight &&
			(props.highlight.unresolved.length > 0 || props.highlight.message) ? (
				<p
					className="font-mono text-xs text-muted-foreground"
					data-testid="public-release-highlight-unresolved"
					role="status"
				>
					{props.highlight.message ??
						`${props.highlight.unresolved.length} 个高亮目标暂时未找到`}
				</p>
			) : null}
			<div
				ref={listRef}
				className="relative w-full"
				style={{ height: `${virtualizer.getTotalSize()}px` }}
				data-testid="public-release-virtual-list"
			>
				{virtualItems.map((virtualItem) => {
					const row = rows[virtualItem.index];
					if (!row) return null;
					return (
						<div
							key={virtualItem.key}
							ref={virtualizer.measureElement}
							data-index={virtualItem.index}
							className="absolute top-0 left-0 w-full pb-3 sm:pb-4"
							style={{
								transform: `translateY(${virtualItem.start - virtualizer.options.scrollMargin}px)`,
							}}
						>
							{row.kind === "gap" ? (
								<GapLoader
									gap={row.gap}
									loading={props.loadingGap === row.gap.newer_cursor}
									onVisible={props.onLoadGap}
								/>
							) : (
								<ReleaseVirtualRow
									item={row.item}
									lane={props.selectedLane}
									detailHref={detailHref(row.item)}
									reactionControls={props.reactionControls}
									hasHighlightContext={Boolean(props.highlight)}
								/>
							)}
						</div>
					);
				})}
			</div>
			<AutoLoadSentinel
				enabled={props.hasMore && !props.loadingMore && !props.appendError}
				onVisible={props.onLoadMore}
			/>
			{props.loadingMore ? (
				<p className="font-mono text-xs text-muted-foreground">加载中...</p>
			) : null}
			{props.appendError ? (
				<div className="flex justify-center pt-1">
					<Button
						type="button"
						variant="outline"
						size="sm"
						className="font-mono text-xs"
						onClick={props.onLoadMore}
					>
						继续加载
					</Button>
				</div>
			) : null}
			{props.hasMore && !props.loadingMore && !props.appendError ? (
				<div className="flex justify-center pt-1">
					<Button
						type="button"
						variant="ghost"
						size="sm"
						className="font-mono text-xs"
						onClick={props.onLoadMore}
					>
						更多
					</Button>
				</div>
			) : null}
			{props.highlight && props.highlight.total > 0 ? (
				<nav
					className="fixed right-4 z-30 flex items-center gap-1 rounded-xl border bg-background/95 p-1 shadow-sm supports-[backdrop-filter]:backdrop-blur-sm"
					style={{ bottom: "calc(env(safe-area-inset-bottom) + 1rem)" }}
					aria-label="高亮记录导航"
					data-testid="public-release-highlight-navigation"
				>
					<Button
						type="button"
						variant="ghost"
						size="icon"
						title="上一条高亮记录"
						disabled={activeTargetIndex <= 0}
						onClick={() => {
							const target = navigationTargets[activeTargetIndex - 1];
							if (target) activateTarget(target, true);
						}}
					>
						<ChevronUp className="size-4" />
					</Button>
					<span className="min-w-16 text-center font-mono text-xs tabular-nums">
						{props.highlight.active_index ?? activeTargetIndex + 1} /{" "}
						{props.highlight.total}
					</span>
					<Button
						type="button"
						variant="ghost"
						size="icon"
						title="下一条高亮记录"
						disabled={
							activeTargetIndex >= navigationTargets.length - 1 &&
							!props.hasMore
						}
						onClick={() => {
							const target = navigationTargets[activeTargetIndex + 1];
							if (target) {
								activateTarget(target, true);
							} else {
								void props.onLoadMore();
							}
						}}
					>
						<ChevronDown className="size-4" />
					</Button>
				</nav>
			) : null}
		</div>
	);
}

function AutoLoadSentinel(props: {
	enabled: boolean;
	onVisible: () => Promise<void>;
	root?: Element | null;
}) {
	const ref = useRef<HTMLDivElement | null>(null);
	const visibleRef = useRef(false);
	useEffect(() => {
		if (!props.enabled || !ref.current) return;
		const observer = new IntersectionObserver(
			(entries) => {
				const visible = entries.some((entry) => entry.isIntersecting);
				if (visible && !visibleRef.current) {
					visibleRef.current = true;
					void props.onVisible();
				} else if (!visible) {
					visibleRef.current = false;
				}
			},
			{ root: props.root ?? null, rootMargin: "900px 0px", threshold: 0.01 },
		);
		observer.observe(ref.current);
		return () => observer.disconnect();
	}, [props.enabled, props.onVisible, props.root]);
	return <div ref={ref} className="h-px" aria-hidden="true" />;
}

void _ReleaseList;

function GapLoader(props: {
	gap: PublicReleaseGap;
	loading: boolean;
	onVisible: (gap: PublicReleaseGap) => Promise<void>;
	root?: Element | null;
}) {
	const ref = useRef<HTMLDivElement | null>(null);
	useEffect(() => {
		if (props.loading || !ref.current) return;
		const observer = new IntersectionObserver(
			(entries) => {
				if (entries.some((entry) => entry.isIntersecting)) {
					void props.onVisible(props.gap);
				}
			},
			{ root: props.root ?? null, rootMargin: "700px 0px", threshold: 0.01 },
		);
		observer.observe(ref.current);
		return () => observer.disconnect();
	}, [props.gap, props.loading, props.onVisible, props.root]);
	return (
		<div
			ref={ref}
			className="flex min-h-14 items-center justify-center rounded-xl border border-dashed bg-muted/20 px-4 py-3 font-mono text-xs text-muted-foreground"
			role="status"
		>
			{props.loading
				? "正在补齐中间记录..."
				: `省略 ${props.gap.remaining_count} 条，滚动后自动加载`}
		</div>
	);
}

function ReleaseVirtualRow(props: {
	owner?: string;
	repo?: string;
	item: PublicReleaseListItem;
	lane: FeedLane;
	detailHref: string;
	reactionControls: PublicReleaseReactionControls;
	hasHighlightContext: boolean;
	isCurrent?: boolean;
	isPulse?: boolean;
	onFocusCurrent?: () => void;
	onHoverCurrent?: () => void;
	onTitleActivate?: () => void;
	titleSearch?: Record<string, unknown>;
}) {
	const showReactions =
		props.reactionControls.enabled &&
		props.reactionControls.availableReleaseIds.has(props.item.release_id);
	const reactions = showReactions
		? (props.reactionControls.byReleaseId[props.item.release_id] ?? null)
		: null;
	const feedItem = publicReleaseToFeedItem(props.item, reactions);
	const emphasis = !props.hasHighlightContext
		? "default"
		: props.item.is_active_highlight
			? "active-highlight"
			: props.item.is_highlighted
				? "highlighted"
				: "subdued";
	return (
		<article
			tabIndex={
				props.isCurrent ? 0 : props.item.is_active_highlight ? -1 : undefined
			}
			onFocus={props.onFocusCurrent}
			className={cn(
				"scroll-mt-5 rounded-xl outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
				props.isPulse ? "public-release-focus-pulse" : undefined,
			)}
			onPointerEnter={props.onHoverCurrent}
			data-highlighted={props.item.is_highlighted ? "true" : "false"}
			data-active-highlight={props.item.is_active_highlight ? "true" : "false"}
			data-current-release={props.isCurrent ? "true" : "false"}
			data-release-id={props.item.release_id}
			data-testid={`public-release-item-${props.item.release_id}`}
		>
			<ReleaseFeedCard
				item={feedItem}
				activeLane={props.lane}
				emphasis={emphasis}
				isTranslating={false}
				isTranslationAutoRetrying={false}
				isSmartGenerating={false}
				isSmartAutoRetrying={false}
				isReactionBusy={props.reactionControls.busyReleaseIds.has(
					props.item.release_id,
				)}
				reactionError={
					props.reactionControls.errorByReleaseId[props.item.release_id] ?? null
				}
				showReactions={showReactions}
				showRepoIdentity={false}
				showHeaderActions={false}
				titleHref={props.detailHref}
				titleTo={
					props.owner && props.repo
						? "/public/$owner/$repo/releases/tag/$tag"
						: null
				}
				titleParams={
					props.owner && props.repo
						? {
								owner: props.owner,
								repo: props.repo,
								tag: props.item.tag_name,
							}
						: undefined
				}
				titleSearch={props.titleSearch}
				onTitleActivate={props.onTitleActivate}
				onSelectLane={() => undefined}
				onTranslateNow={() => undefined}
				onSmartNow={() => undefined}
				onToggleReaction={(content) =>
					props.reactionControls.onToggle(props.item.release_id, content)
				}
			/>
		</article>
	);
}

function publicReleaseToFeedItem(
	item: PublicReleaseListItem,
	reactions: ReleaseReactions | null = null,
): ReleaseFeedItem {
	const body = truncatePublicReleaseListBody(item.body);
	return {
		kind: "release",
		ts: item.published_at ?? "",
		id: item.release_id,
		repo_full_name: item.repo_full_name,
		repo_visual: item.repo_visual,
		title: releaseTitle(item),
		body,
		body_truncated: body !== item.body,
		subtitle: item.tag_name,
		reason: null,
		subject_type: null,
		html_url: item.html_url,
		unread: null,
		translated: item.translated,
		smart: item.smart,
		reactions,
	};
}

function truncatePublicReleaseListBody(body: string | null) {
	if (!body || body.length <= PUBLIC_RELEASE_LIST_BODY_MAX_CHARS) {
		return body;
	}
	return `${body.slice(0, PUBLIC_RELEASE_LIST_BODY_MAX_CHARS).trimEnd()}\n\n...`;
}

function _ReleaseDetail({ detail }: { detail: ReleaseDetailResponse }) {
	const initialLane =
		detail.smart?.status === "ready"
			? "smart"
			: detail.translated?.status === "ready"
				? "translated"
				: "original";
	const [selectedLane, setSelectedLane] = useState<FeedLane>(initialLane);
	const feedItem = publicReleaseDetailToFeedItem(detail);

	return (
		<div className="py-6">
			<ReleaseFeedCard
				item={feedItem}
				activeLane={selectedLane}
				isTranslating={false}
				isTranslationAutoRetrying={false}
				isSmartGenerating={false}
				isSmartAutoRetrying={false}
				isReactionBusy={false}
				reactionError={null}
				showReactions={false}
				surface="article"
				onSelectLane={setSelectedLane}
				onTranslateNow={() => undefined}
				onSmartNow={() => undefined}
				onToggleReaction={() => undefined}
			/>
		</div>
	);
}

void _ReleaseDetail;

function publicReleaseDetailToFeedItem(
	detail: ReleaseDetailResponse,
): ReleaseFeedItem {
	return {
		kind: "release",
		ts: detail.published_at ?? "",
		id: detail.release_id,
		repo_full_name: detail.repo_full_name,
		repo_visual: detail.repo_visual,
		title: releaseTitle(detail),
		body: detail.body,
		body_truncated: false,
		subtitle: detail.tag_name,
		reason: null,
		subject_type: null,
		html_url: detail.html_url,
		unread: null,
		translated: detail.translated,
		smart: detail.smart,
		reactions: null,
	};
}
