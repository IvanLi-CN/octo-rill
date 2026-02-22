import { useCallback, useMemo, useRef, useState } from "react";

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

export function useFeed(params?: { types: string | null }) {
	const types = params?.types ?? null;
	const typesParam = types ? `&types=${encodeURIComponent(types)}` : "";

	const [items, setItems] = useState<FeedItem[]>([]);
	const [nextCursor, setNextCursor] = useState<string | null>(null);
	const [loadingInitial, setLoadingInitial] = useState(false);
	const [loadingMore, setLoadingMore] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const reqIdRef = useRef(0);
	const lastTypesParamRef = useRef<string>("");

	const hasMore = Boolean(nextCursor);

	const loadInitial = useCallback(async () => {
		reqIdRef.current += 1;
		const reqId = reqIdRef.current;

		// Cancel any in-flight "load more" state; we are replacing the list.
		setLoadingMore(false);

		// Avoid flashing wrong results when switching filters (types).
		if (lastTypesParamRef.current !== typesParam) {
			lastTypesParamRef.current = typesParam;
			setItems([]);
			setNextCursor(null);
		}

		setLoadingInitial(true);
		setError(null);
		try {
			const res = await apiGet<FeedResponse>(`/api/feed?limit=30${typesParam}`);
			if (reqId !== reqIdRef.current) return;
			setItems(res.items);
			setNextCursor(res.next_cursor);
		} catch (err) {
			if (reqId !== reqIdRef.current) return;
			setError(err instanceof Error ? err.message : String(err));
		} finally {
			if (reqId === reqIdRef.current) {
				setLoadingInitial(false);
			}
		}
	}, [typesParam]);

	const loadMore = useCallback(async () => {
		if (!nextCursor || loadingMore) return;
		const reqId = reqIdRef.current;
		setLoadingMore(true);
		setError(null);
		try {
			const res = await apiGet<FeedResponse>(
				`/api/feed?limit=30${typesParam}&cursor=${encodeURIComponent(nextCursor)}`,
			);
			if (reqId !== reqIdRef.current) return;
			setItems((prev) => mergeByKey(prev, res.items));
			setNextCursor(res.next_cursor);
		} catch (err) {
			if (reqId !== reqIdRef.current) return;
			setError(err instanceof Error ? err.message : String(err));
		} finally {
			setLoadingMore(false);
		}
	}, [nextCursor, loadingMore, typesParam]);

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
