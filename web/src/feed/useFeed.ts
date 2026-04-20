import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { apiGet } from "@/api";
import type {
	FeedItem,
	FeedResponse,
	ReleaseReactions,
	SmartItem,
	TranslatedItem,
} from "@/feed/types";
import { isReleaseFeedItem } from "@/feed/types";

export type FeedRequestType = "all" | "releases" | "stars" | "followers";
export type FeedLoadErrorPhase = "initial" | "append";
export type FeedLoadError = {
	phase: FeedLoadErrorPhase;
	message: string;
	at: number;
};

function itemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
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
				out[idx] = {
					...current,
					...n,
					actor: null,
				};
				continue;
			}
			if (
				(current.kind === "repo_star_received" ||
					current.kind === "follower_received") &&
				(n.kind === "repo_star_received" || n.kind === "follower_received")
			) {
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
	cursor?: string | null,
) {
	const params = new URLSearchParams();
	params.set("limit", String(limit));
	if (type !== "all") {
		params.set("types", type);
	}
	if (cursor) {
		params.set("cursor", cursor);
	}
	return `/api/feed?${params.toString()}`;
}

export function useFeed(
	type: FeedRequestType = "all",
	options?: {
		initialData?: {
			type: FeedRequestType;
			items: FeedItem[];
			nextCursor: string | null;
		} | null;
	},
) {
	const initialData = options?.initialData;
	const initialStateMatches = initialData?.type === type;
	const [dataType, setDataType] = useState<FeedRequestType>(
		initialStateMatches ? initialData.type : type,
	);
	const [items, setItems] = useState<FeedItem[]>(
		initialStateMatches ? initialData.items : [],
	);
	const [nextCursor, setNextCursor] = useState<string | null>(
		initialStateMatches ? initialData.nextCursor : null,
	);
	const [loadingInitial, setLoadingInitial] = useState(!initialStateMatches);
	const [loadingMore, setLoadingMore] = useState(false);
	const [error, setError] = useState<FeedLoadError | null>(null);

	const reqIdRef = useRef(0);
	const isCurrentType = dataType === type;
	const currentItems = isCurrentType ? items : [];
	const currentNextCursor = isCurrentType ? nextCursor : null;
	const currentError = isCurrentType ? error : null;
	const currentLoadingInitial = !isCurrentType || loadingInitial;
	const hasMore = Boolean(currentNextCursor);

	useEffect(() => {
		reqIdRef.current += 1;
		setDataType(type);
		setItems([]);
		setNextCursor(null);
		setLoadingInitial(false);
		setLoadingMore(false);
		setError(null);
	}, [type]);

	const loadInitial = useCallback(async () => {
		reqIdRef.current += 1;
		const reqId = reqIdRef.current;

		// Cancel any in-flight "load more" state; we are replacing the list.
		setLoadingMore(false);

		setLoadingInitial(true);
		setError(null);
		try {
			const res = await apiGet<FeedResponse>(buildFeedUrl(30, type));
			if (reqId !== reqIdRef.current) return;
			setDataType(type);
			setItems(res.items);
			setNextCursor(res.next_cursor);
		} catch (err) {
			if (reqId !== reqIdRef.current) return;
			setError({
				phase: "initial",
				message: err instanceof Error ? err.message : String(err),
				at: Date.now(),
			});
		} finally {
			if (reqId === reqIdRef.current) {
				setLoadingInitial(false);
			}
		}
	}, [type]);

	const loadMore = useCallback(async () => {
		if (!currentNextCursor || loadingMore || currentLoadingInitial) return;
		const reqId = reqIdRef.current;
		setLoadingMore(true);
		setError(null);
		try {
			const res = await apiGet<FeedResponse>(
				buildFeedUrl(30, type, currentNextCursor),
			);
			if (reqId !== reqIdRef.current) return;
			setDataType(type);
			setItems((prev) => mergeByKey(prev, res.items));
			setNextCursor(res.next_cursor);
		} catch (err) {
			if (reqId !== reqIdRef.current) return;
			setError({
				phase: "append",
				message: err instanceof Error ? err.message : String(err),
				at: Date.now(),
			});
		} finally {
			if (reqId === reqIdRef.current) {
				setLoadingMore(false);
			}
		}
	}, [currentLoadingInitial, currentNextCursor, loadingMore, type]);

	const refresh = useCallback(async () => {
		await loadInitial();
	}, [loadInitial]);

	const applyTranslation = useCallback(
		(item: Pick<FeedItem, "kind" | "id">, translated: TranslatedItem) => {
			const key = itemKey(item);
			setItems((prev) =>
				prev.map((it) => {
					if (itemKey(it) !== key) return it;
					if (!isReleaseFeedItem(it)) return it;
					return {
						...it,
						translated: { ...translated },
					};
				}),
			);
		},
		[],
	);

	const applySmart = useCallback(
		(item: Pick<FeedItem, "kind" | "id">, smart: SmartItem) => {
			const key = itemKey(item);
			setItems((prev) =>
				prev.map((it) => {
					if (itemKey(it) !== key) return it;
					if (!isReleaseFeedItem(it)) return it;
					return {
						...it,
						smart: { ...smart },
					};
				}),
			);
		},
		[],
	);

	const applyReactions = useCallback(
		(item: Pick<FeedItem, "kind" | "id">, reactions: ReleaseReactions) => {
			const key = itemKey(item);
			setItems((prev) =>
				prev.map((it) => {
					if (itemKey(it) !== key) return it;
					if (!isReleaseFeedItem(it)) return it;
					return {
						...it,
						reactions,
					};
				}),
			);
		},
		[],
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
