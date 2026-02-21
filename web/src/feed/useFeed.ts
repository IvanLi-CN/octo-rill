import { useCallback, useMemo, useState } from "react";

import { apiGet } from "@/api";
import type { FeedItem, FeedResponse, TranslateResponse } from "@/feed/types";

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
			out[idx] = { ...out[idx], ...n };
		}
	}
	return out;
}

export function useFeed() {
	const [items, setItems] = useState<FeedItem[]>([]);
	const [nextCursor, setNextCursor] = useState<string | null>(null);
	const [loadingInitial, setLoadingInitial] = useState(false);
	const [loadingMore, setLoadingMore] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const hasMore = Boolean(nextCursor);

	const loadInitial = useCallback(async () => {
		setLoadingInitial(true);
		setError(null);
		try {
			const res = await apiGet<FeedResponse>("/api/feed?limit=30");
			setItems(res.items);
			setNextCursor(res.next_cursor);
		} catch (err) {
			setError(err instanceof Error ? err.message : String(err));
		} finally {
			setLoadingInitial(false);
		}
	}, []);

	const loadMore = useCallback(async () => {
		if (!nextCursor || loadingMore) return;
		setLoadingMore(true);
		setError(null);
		try {
			const res = await apiGet<FeedResponse>(
				`/api/feed?limit=30&cursor=${encodeURIComponent(nextCursor)}`,
			);
			setItems((prev) => mergeByKey(prev, res.items));
			setNextCursor(res.next_cursor);
		} catch (err) {
			setError(err instanceof Error ? err.message : String(err));
		} finally {
			setLoadingMore(false);
		}
	}, [nextCursor, loadingMore]);

	const refresh = useCallback(async () => {
		await loadInitial();
	}, [loadInitial]);

	const applyTranslation = useCallback(
		(item: Pick<FeedItem, "kind" | "id">, res: TranslateResponse) => {
			const key = itemKey(item);
			setItems((prev) =>
				prev.map((it) => {
					if (itemKey(it) !== key) return it;
					return {
						...it,
						translated: {
							lang: res.lang,
							status: res.status === "disabled" ? "disabled" : "ready",
							title: res.title,
							summary: res.summary,
						},
					};
				}),
			);
		},
		[],
	);

	const stats = useMemo(() => {
		let releases = 0;
		let notifications = 0;
		for (const it of items) {
			if (it.kind === "release") releases += 1;
			if (it.kind === "notification") notifications += 1;
		}
		return { releases, notifications, total: items.length };
	}, [items]);

	return {
		items,
		nextCursor,
		hasMore,
		loadingInitial,
		loadingMore,
		error,
		stats,
		loadInitial,
		loadMore,
		refresh,
		applyTranslation,
	};
}
