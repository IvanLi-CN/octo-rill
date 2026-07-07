import { useCallback, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { apiGet } from "@/api";
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
};

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
		scope?: DashboardScope | null;
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
	const scope = options?.scope ?? null;
	const scopeSignature = useMemo(
		() => buildDashboardScopeSignature(scope),
		[scope],
	);
	const queryKey = useMemo(
		() => dashboardFeedQueryKey({ userId, type, scope, cursor: null }),
		[scopeSignature, type, userId],
	);
	const initialStateMatches = initialData?.type === type;
	const [loadingMore, setLoadingMore] = useState(false);
	const [appendError, setAppendError] = useState<FeedLoadError | null>(null);
	const [freshKeys, setFreshKeys] = useState<Set<string>>(() => new Set());

	const reqIdRef = useRef(0);
	const query = useQuery<DashboardFeedQueryData>({
		queryKey,
		queryFn: async () => {
			const current =
				queryClient.getQueryData<DashboardFeedQueryData>(queryKey);
			const res = await apiGet<FeedResponse>(buildFeedUrl(30, type, scope));
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

	const currentData = query.data ?? null;
	const currentItems = currentData?.items ?? [];
	const currentNextCursor = currentData?.nextCursor ?? null;
	const currentLoadingInitial = !currentData && query.isPending;
	const hasMore = Boolean(currentNextCursor);
	const currentInitialError = query.error
		? {
				phase: "initial" as const,
				...describeNetworkAwareError(query.error, "动态加载失败，请稍后重试。"),
				at: query.errorUpdatedAt || Date.now(),
			}
		: null;
	const currentError = appendError ?? currentInitialError;

	const loadInitial = useCallback(
		async (options?: { freshKeys?: string[]; throwOnError?: boolean }) => {
			reqIdRef.current += 1;
			const reqId = reqIdRef.current;

			// Cancel any in-flight "load more" state; we are replacing the list.
			setLoadingMore(false);
			setAppendError(null);
			const result = await query.refetch();
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
		},
		[query.refetch],
	);

	const loadMore = useCallback(async () => {
		if (!currentNextCursor || loadingMore || currentLoadingInitial) return;
		const reqId = reqIdRef.current;
		setLoadingMore(true);
		setAppendError(null);
		try {
			const pageQueryKey = dashboardFeedQueryKey({
				userId,
				type,
				scope,
				cursor: currentNextCursor,
			});
			const page = await queryClient.fetchQuery<DashboardFeedQueryData>({
				queryKey: pageQueryKey,
				queryFn: async () => {
					const res = await apiGet<FeedResponse>(
						buildFeedUrl(30, type, scope, currentNextCursor),
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
		scope,
		scopeSignature,
		type,
		userId,
	]);

	const refresh = useCallback(
		async (options?: { freshKeys?: string[]; throwOnError?: boolean }) => {
			await loadInitial(options);
		},
		[loadInitial],
	);

	const applyTranslation = useCallback(
		(item: Pick<FeedItem, "kind" | "id">, translated: TranslatedItem) => {
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
									translated: { ...translated },
								};
							}),
						}
					: current,
			);
		},
		[queryClient, queryKey],
	);

	const applySmart = useCallback(
		(item: Pick<FeedItem, "kind" | "id">, smart: SmartItem) => {
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
									smart: { ...smart },
								};
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
		freshKeys,
		nextCursor: currentNextCursor,
		hasMore,
		loadingInitial: currentLoadingInitial,
		loadingMore,
		error: currentError,
		stats,
		loadInitial,
		loadMore,
		refresh,
		applyTranslation,
		applySmart,
		applyReactions,
	};
}
