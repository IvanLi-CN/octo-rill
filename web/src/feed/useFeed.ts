import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { ApiError, apiGet } from "@/api";
import {
	applySmartIfSourceMatches,
	applyTranslationIfSourceMatches,
} from "@/feed/autoLaneRequests";
import {
	buildDashboardScopeSignature,
	type DashboardScope,
} from "@/dashboard/routeState";
import type {
	FeedItem,
	FeedResponse,
	ReleaseReactions,
	SmartItem,
	TranslatedItem,
} from "@/feed/types";
import { isReleaseFeedItem, isSocialFeedItem } from "@/feed/types";
import {
	describeNetworkAwareError,
	type NetworkErrorKind,
} from "@/lib/errorPresentation";
import {
	DASHBOARD_QUERY_MAX_AGE_MS,
	DASHBOARD_QUERY_STALE_MS,
	dashboardFeedQueryKey,
	type DashboardFeedQueryData,
} from "@/query/dashboardQueryKeys";

export type FeedRequestType = "all" | "releases" | "stars" | "followers";
export type FeedLoadErrorPhase = "initial" | "append";
export type FeedLoadError = {
	phase: FeedLoadErrorPhase;
	message: string;
	kind: NetworkErrorKind;
	detail: string | null;
	at: number;
	authorizationDenied: boolean;
};

function isProtectedFeedUnavailable(error: unknown) {
	return (
		error instanceof ApiError &&
		(error.status === 401 ||
			error.status === 403 ||
			(error.status === 404 && error.code === "not_found"))
	);
}

function itemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

function mergeReleaseFeedItem(current: FeedItem, incoming: FeedItem): FeedItem {
	if (!isReleaseFeedItem(current) || !isReleaseFeedItem(incoming)) {
		return incoming;
	}
	return {
		...current,
		...incoming,
		actor: null,
		reactions:
			current.reactions?.status === "ready" &&
			incoming.reactions?.status === "ready"
				? { ...incoming.reactions, viewer: current.reactions.viewer }
				: incoming.reactions,
	};
}

function preserveReleaseViewers(existing: FeedItem[], incoming: FeedItem[]) {
	const existingByKey = new Map<string, FeedItem>();
	for (const item of existing) {
		existingByKey.set(itemKey(item), item);
	}
	return incoming.map((item) => {
		const current = existingByKey.get(itemKey(item));
		if (current?.kind === "release" && item.kind === "release") {
			return mergeReleaseFeedItem(current, item);
		}
		return item;
	});
}

function mergeByKey(existing: FeedItem[], incoming: FeedItem[]) {
	const out = existing.slice();
	const indexByKey = new Map<string, number>();
	for (let i = 0; i < out.length; i += 1) {
		indexByKey.set(itemKey(out[i]), i);
	}

	for (const n of incoming) {
		const key = itemKey(n);
		const idx = indexByKey.get(key);
		if (idx === undefined) {
			indexByKey.set(key, out.length);
			out.push(n);
		} else {
			const current = out[idx];
			if (current.kind !== n.kind) {
				out[idx] = n;
				continue;
			}
			if (current.kind === "release" && n.kind === "release") {
				out[idx] = mergeReleaseFeedItem(current, n);
				continue;
			}
			if (isSocialFeedItem(current) && isSocialFeedItem(n)) {
				out[idx] = {
					...current,
					...n,
					actor: n.actor ?? current.actor,
				};
				continue;
			}
			out[idx] = n;
		}
	}
	return out;
}

function buildFeedUrl(
	limit: number,
	type: FeedRequestType,
	scope?: DashboardScope | null,
	cursor?: string | null,
) {
	const params = new URLSearchParams();
	params.set("limit", String(limit));
	if (type !== "all") {
		params.set("types", type);
	}
	if (scope) {
		params.set("scope", scope.kind);
		if (scope.kind === "repo") {
			params.set("items", `${scope.owner}/${scope.repo}`);
		} else if (scope.kind === "repos" && scope.items.length > 0) {
			params.set("items", scope.items.join(","));
		} else if (scope.kind === "org") {
			params.set("org", scope.org);
		}
	}
	if (cursor) {
		params.set("cursor", cursor);
	}
	return `/api/feed?${params.toString()}`;
}

export function useFeed(
	type: FeedRequestType = "all",
	options?: {
		userId?: string;
		viewerStateKey?: string | null;
		scope?: DashboardScope | null;
		enabled?: boolean;
		initialData?: {
			type: FeedRequestType;
			items: FeedItem[];
			nextCursor: string | null;
			updatedAt?: number;
		} | null;
	},
) {
	const queryClient = useQueryClient();
	const initialData = options?.initialData;
	const userId = options?.userId ?? "anonymous";
	const viewerStateKey = options?.viewerStateKey ?? null;
	const scope = options?.scope ?? null;
	const enabled = options?.enabled ?? true;
	const scopeSignature = useMemo(
		() => buildDashboardScopeSignature(scope),
		[scope],
	);
	const queryKey = useMemo(
		() =>
			dashboardFeedQueryKey({
				userId,
				type,
				scope,
				viewerStateKey,
				cursor: null,
			}),
		[scopeSignature, type, userId, viewerStateKey],
	);
	const initialStateMatches = initialData?.type === type;
	const [loadingMore, setLoadingMore] = useState(false);
	const [appendError, setAppendError] = useState<FeedLoadError | null>(null);
	const [authorizationDenied, setAuthorizationDenied] = useState<{
		queryKey: string;
		error: ApiError;
	} | null>(null);
	const queryKeySignature = JSON.stringify(queryKey);
	const [freshKeys, setFreshKeys] = useState<Set<string>>(() => new Set());
	const [refreshing, setRefreshing] = useState(false);

	const reqIdRef = useRef(0);
	const refreshingReqIdRef = useRef<number | null>(null);
	const queryRequestSeqRef = useRef(0);
	const invalidateRequests = useCallback(() => {
		reqIdRef.current += 1;
		refreshingReqIdRef.current = null;
		queryRequestSeqRef.current += 1;
		void queryClient.cancelQueries({
			queryKey: ["dashboard", "feed", { userId }],
		});
		setRefreshing(false);
		setLoadingMore(false);
		setAppendError(null);
	}, [queryClient, userId]);
	const clearCachedFeedContent = useCallback(() => {
		invalidateRequests();
		queryClient.setQueryData<DashboardFeedQueryData>(queryKey, {
			type,
			scopeSignature,
			items: [],
			nextCursor: null,
		});
		queryClient.setQueriesData<DashboardFeedQueryData>(
			{ queryKey: ["dashboard", "feed", { userId }] },
			(current) =>
				current ? { ...current, items: [], nextCursor: null } : current,
		);
	}, [invalidateRequests, queryClient, queryKey, scopeSignature, type, userId]);
	const query = useQuery<DashboardFeedQueryData>({
		queryKey,
		enabled,
		retry: (failureCount, error) =>
			!isProtectedFeedUnavailable(error) && failureCount < 1,
		queryFn: async ({ signal }) => {
			const requestSeq = ++queryRequestSeqRef.current;
			const requestIsCurrent = () =>
				requestSeq === queryRequestSeqRef.current && !signal.aborted;
			const current =
				queryClient.getQueryData<DashboardFeedQueryData>(queryKey);
			let res: FeedResponse;
			try {
				res = await apiGet<FeedResponse>(buildFeedUrl(30, type, scope), {
					signal,
				});
				if (requestIsCurrent()) {
					setAuthorizationDenied((current) =>
						current?.queryKey === queryKeySignature ? null : current,
					);
				}
			} catch (error) {
				if (
					requestIsCurrent() &&
					error instanceof ApiError &&
					isProtectedFeedUnavailable(error)
				) {
					setAuthorizationDenied({ queryKey: queryKeySignature, error });
				}
				throw error;
			}
			return {
				type,
				scopeSignature,
				items: preserveReleaseViewers(current?.items ?? [], res.items),
				nextCursor: res.next_cursor,
			};
		},
		initialData: initialStateMatches
			? {
					type: initialData.type,
					scopeSignature,
					items: initialData.items,
					nextCursor: initialData.nextCursor,
				}
			: undefined,
		initialDataUpdatedAt:
			initialStateMatches && initialData.updatedAt !== undefined
				? initialData.updatedAt
				: undefined,
		staleTime: DASHBOARD_QUERY_STALE_MS,
		gcTime: DASHBOARD_QUERY_MAX_AGE_MS,
	});
	const authorizationDeniedError =
		authorizationDenied?.queryKey === queryKeySignature
			? authorizationDenied.error
			: null;
	const feedAuthorizationDenied =
		Boolean(authorizationDeniedError) ||
		isProtectedFeedUnavailable(query.error);
	useEffect(() => {
		if (!feedAuthorizationDenied || query.isFetching) return;
		clearCachedFeedContent();
	}, [clearCachedFeedContent, feedAuthorizationDenied, query.isFetching]);

	const currentData = query.data ?? null;
	const hasSuccessfulRead =
		feedAuthorizationDenied ||
		(queryClient.getQueryState<DashboardFeedQueryData>(queryKey)
			?.dataUpdateCount ?? 0) > 0;
	const currentItems = feedAuthorizationDenied
		? []
		: (currentData?.items ?? []);
	const currentNextCursor = feedAuthorizationDenied
		? null
		: (currentData?.nextCursor ?? null);
	const currentLoadingInitial = enabled && !currentData && query.isPending;
	const hasMore = Boolean(currentNextCursor);
	const currentInitialError =
		(authorizationDeniedError ?? query.error)
			? {
					phase: "initial" as const,
					...describeNetworkAwareError(
						authorizationDeniedError ?? query.error,
						"动态加载失败，请稍后重试。",
					),
					at: query.errorUpdatedAt || Date.now(),
					authorizationDenied: feedAuthorizationDenied,
				}
			: null;
	const currentError = feedAuthorizationDenied
		? currentInitialError
		: (appendError ?? currentInitialError);

	useEffect(() => {
		reqIdRef.current += 1;
		refreshingReqIdRef.current = null;
		setRefreshing(false);
		setLoadingMore(false);
		setAppendError(null);
		setAuthorizationDenied(null);
		setFreshKeys(new Set());
	}, [queryKey]);

	const loadInitial = useCallback(
		async (options?: {
			freshKeys?: string[];
			throwOnError?: boolean;
			force?: boolean;
			onStart?: (cancel: () => void) => void;
		}) => {
			if (!enabled && !options?.force) return;
			reqIdRef.current += 1;
			const reqId = reqIdRef.current;
			refreshingReqIdRef.current = reqId;
			setRefreshing(true);
			try {
				const cancelled = Symbol("feed-refresh-cancelled");
				let cancelRefresh = () => undefined;
				const cancellation = new Promise<typeof cancelled>((resolve) => {
					cancelRefresh = () => {
						if (reqId !== reqIdRef.current) return;
						reqIdRef.current += 1;
						void queryClient.cancelQueries({ queryKey, exact: true });
						resolve(cancelled);
					};
				});
				options?.onStart?.(cancelRefresh);

				// Cancel any in-flight "load more" state; we are replacing the list.
				setLoadingMore(false);
				setAppendError(null);
				const currentPageQueries = {
					predicate: (query: { queryKey: readonly unknown[] }) => {
						const [root, name, dimensions] = query.queryKey;
						if (
							root !== "dashboard" ||
							name !== "feed" ||
							!dimensions ||
							typeof dimensions !== "object"
						) {
							return false;
						}
						const pageKey = dimensions as Record<string, unknown>;
						return (
							pageKey.userId === userId &&
							pageKey.type === type &&
							pageKey.scope === scopeSignature &&
							pageKey.viewerStateKey === viewerStateKey &&
							typeof pageKey.cursor === "string"
						);
					},
				};
				await queryClient.cancelQueries(currentPageQueries);
				queryClient.removeQueries(currentPageQueries);
				if (reqId !== reqIdRef.current) return;
				const result = await Promise.race([query.refetch(), cancellation]);
				if (result === cancelled) return;
				if (reqId !== reqIdRef.current) return;
				if (result.error) {
					if (options?.throwOnError) {
						throw result.error;
					}
					return;
				}
				if (result.data) {
					setFreshKeys(new Set(options?.freshKeys ?? []));
				}
			} finally {
				if (refreshingReqIdRef.current === reqId) {
					refreshingReqIdRef.current = null;
					setRefreshing(false);
				}
			}
		},
		[
			enabled,
			query.refetch,
			queryClient,
			queryKey,
			scopeSignature,
			type,
			userId,
			viewerStateKey,
		],
	);

	const loadMore = useCallback(async () => {
		if (refreshingReqIdRef.current !== null) return;
		if (!currentNextCursor || loadingMore || currentLoadingInitial) return;
		const reqId = reqIdRef.current;
		setLoadingMore(true);
		setAppendError(null);
		try {
			const pageQueryKey = dashboardFeedQueryKey({
				userId,
				type,
				scope,
				viewerStateKey,
				cursor: currentNextCursor,
			});
			const page = await queryClient.fetchQuery<DashboardFeedQueryData>({
				queryKey: pageQueryKey,
				queryFn: async ({ signal }) => {
					const res = await apiGet<FeedResponse>(
						buildFeedUrl(30, type, scope, currentNextCursor),
						{ signal },
					);
					return {
						type,
						scopeSignature,
						items: res.items,
						nextCursor: res.next_cursor,
					};
				},
				staleTime: DASHBOARD_QUERY_STALE_MS,
				gcTime: DASHBOARD_QUERY_MAX_AGE_MS,
			});
			if (reqId !== reqIdRef.current) return;
			queryClient.setQueryData<DashboardFeedQueryData>(queryKey, (current) => ({
				type,
				scopeSignature,
				items: mergeByKey(current?.items ?? [], page.items),
				nextCursor: page.nextCursor,
			}));
		} catch (err) {
			if (reqId !== reqIdRef.current) return;
			const authorizationDenied = isProtectedFeedUnavailable(err);
			if (err instanceof ApiError && authorizationDenied) {
				setAuthorizationDenied({ queryKey: queryKeySignature, error: err });
			}
			const description = describeNetworkAwareError(
				err,
				"更多动态加载失败，请稍后重试。",
			);
			setAppendError({
				phase: "append",
				message: description.message,
				kind: description.kind,
				detail: description.detail,
				at: Date.now(),
				authorizationDenied,
			});
		} finally {
			if (reqId === reqIdRef.current) {
				setLoadingMore(false);
			}
		}
	}, [
		currentLoadingInitial,
		currentNextCursor,
		loadingMore,
		queryClient,
		queryKey,
		clearCachedFeedContent,
		queryKeySignature,
		scope,
		scopeSignature,
		type,
		userId,
		viewerStateKey,
	]);

	const refresh = useCallback(
		async (options?: {
			freshKeys?: string[];
			throwOnError?: boolean;
			force?: boolean;
			onStart?: (cancel: () => void) => void;
		}) => {
			await loadInitial(options);
		},
		[loadInitial],
	);

	const applyTranslation = useCallback(
		(
			item: Pick<FeedItem, "kind" | "id">,
			sourceKey: string,
			translated: TranslatedItem,
		) => {
			const key = itemKey(item);
			queryClient.setQueryData<DashboardFeedQueryData>(queryKey, (current) =>
				current
					? {
							...current,
							items: current.items.map((it) => {
								if (itemKey(it) !== key) return it;
								return applyTranslationIfSourceMatches(
									it,
									sourceKey,
									translated,
								);
							}),
						}
					: current,
			);
		},
		[queryClient, queryKey],
	);

	const applySmart = useCallback(
		(
			item: Pick<FeedItem, "kind" | "id">,
			sourceKey: string,
			smart: SmartItem,
		) => {
			const key = itemKey(item);
			queryClient.setQueryData<DashboardFeedQueryData>(queryKey, (current) =>
				current
					? {
							...current,
							items: current.items.map((it) => {
								if (itemKey(it) !== key) return it;
								return applySmartIfSourceMatches(it, sourceKey, smart);
							}),
						}
					: current,
			);
		},
		[queryClient, queryKey],
	);

	const applyReactions = useCallback(
		(item: Pick<FeedItem, "kind" | "id">, reactions: ReleaseReactions) => {
			const key = itemKey(item);
			queryClient.setQueryData<DashboardFeedQueryData>(queryKey, (current) =>
				current
					? {
							...current,
							items: current.items.map((it) => {
								if (itemKey(it) !== key) return it;
								if (!isReleaseFeedItem(it)) return it;
								return {
									...it,
									reactions,
								};
							}),
						}
					: current,
			);
		},
		[queryClient, queryKey],
	);

	const stats = useMemo(() => {
		const releases = currentItems.filter(
			(item) => item.kind === "release",
		).length;
		const stars = currentItems.filter(
			(item) => item.kind === "repo_star_received",
		).length;
		const followers = currentItems.filter(
			(item) => item.kind === "follower_received",
		).length;
		return { releases, stars, followers, total: currentItems.length };
	}, [currentItems]);

	return {
		items: currentItems,
		hasSuccessfulRead,
		freshKeys,
		nextCursor: currentNextCursor,
		hasMore,
		loadingInitial: currentLoadingInitial,
		refreshing,
		loadingMore,
		error: currentError,
		stats,
		invalidateRequests,
		loadInitial,
		loadMore,
		refresh,
		applyTranslation,
		applySmart,
		applyReactions,
	};
}
