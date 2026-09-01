import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { ApiError, apiGet } from "@/api";
import type {
	DashboardReadableFeedResponse,
	DashboardReadableSection,
	FeedItem,
	FeedResponse,
	ReleaseReactions,
	SmartItem,
	TranslatedItem,
} from "@/feed/types";
import { isLaneCapableFeedItem, isReleaseFeedItem } from "@/feed/types";
import {
	describeNetworkAwareError,
	type NetworkErrorKind,
} from "@/lib/errorPresentation";

export type ReadableSectionsError = {
	phase: "initial" | "append";
	message: string;
	kind: NetworkErrorKind;
	detail: string | null;
	at: number;
};

export type ReadableSectionDetails = {
	items: FeedItem[];
	nextCursor: string | null;
	loading: boolean;
	error: string | null;
};

function itemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

function mergeItems(existing: FeedItem[], incoming: FeedItem[]) {
	const out = existing.slice();
	const indexes = new Map(out.map((item, index) => [itemKey(item), index]));
	for (const item of incoming) {
		const key = itemKey(item);
		const index = indexes.get(key);
		if (index === undefined) {
			indexes.set(key, out.length);
			out.push(item);
			continue;
		}
		const current = out[index];
		if (isReleaseFeedItem(current) && isReleaseFeedItem(item)) {
			out[index] = {
				...current,
				...item,
				reactions:
					current.reactions?.status === "ready" &&
					item.reactions?.status === "ready"
						? { ...item.reactions, viewer: current.reactions.viewer }
						: item.reactions,
			};
		} else {
			out[index] = item;
		}
	}
	return out;
}

function updateItems(
	sections: DashboardReadableSection[],
	updater: (item: FeedItem) => FeedItem,
) {
	return sections.map((section) => ({
		...section,
		items: (section.items ?? []).map(updater),
		supplemental_items: (section.supplemental_items ?? []).map(updater),
	}));
}

export function useDashboardReadableSections(options?: {
	userId?: string;
	viewerStateKey?: string | null;
	enabled?: boolean;
}) {
	const userId = options?.userId ?? "anonymous";
	const viewerStateKey = options?.viewerStateKey ?? null;
	const enabled = options?.enabled ?? true;
	const signature = useMemo(
		() => `${userId}:${viewerStateKey ?? ""}`,
		[userId, viewerStateKey],
	);
	const [sections, setSections] = useState<DashboardReadableSection[]>([]);
	const [nextCursor, setNextCursor] = useState<string | null>(null);
	const [loadingInitial, setLoadingInitial] = useState(true);
	const [loadingMore, setLoadingMore] = useState(false);
	const [error, setError] = useState<ReadableSectionsError | null>(null);
	const [details, setDetails] = useState<
		Record<string, ReadableSectionDetails>
	>({});
	const detailsRef = useRef(details);
	detailsRef.current = details;
	const requestIdRef = useRef(0);
	const cursorInFlightRef = useRef(new Set<string>());
	const cursorCompletedRef = useRef(new Set<string>());
	const detailCursorInFlightRef = useRef(new Set<string>());
	const detailCursorCompletedRef = useRef(new Set<string>());

	const loadInitial = useCallback(async () => {
		const requestId = ++requestIdRef.current;
		cursorInFlightRef.current.clear();
		cursorCompletedRef.current.clear();
		detailCursorInFlightRef.current.clear();
		detailCursorCompletedRef.current.clear();
		setLoadingInitial(true);
		setLoadingMore(false);
		setError(null);
		setSections([]);
		setNextCursor(null);
		setDetails({});
		try {
			let response: DashboardReadableFeedResponse;
			try {
				response = await apiGet<DashboardReadableFeedResponse>(
					"/api/dashboard/feed",
				);
			} catch (cause) {
				// Keep rolling deployments and older test fixtures usable while the
				// readable endpoint is introduced. A successful readable response
				// always remains the only normal root-feed path.
				if (!(cause instanceof ApiError) || cause.status !== 404) throw cause;
				const legacy = await apiGet<FeedResponse>("/api/feed");
				const legacyItems = legacy.items ?? [];
				const firstTimestamp = legacyItems[0]?.ts ?? new Date(0).toISOString();
				response = {
					sections:
						legacyItems.length > 0
							? [
									{
										id: "legacy-feed",
										date: firstTimestamp.slice(0, 10),
										item_count: legacyItems.length,
										brief: null,
										items: legacyItems,
										items_next_cursor: null,
										supplemental_items: [],
									},
								]
							: [],
					next_cursor: null,
				};
			}
			if (requestId !== requestIdRef.current) return;
			setSections(response.sections ?? []);
			setNextCursor(response.next_cursor ?? null);
		} catch (cause) {
			if (requestId !== requestIdRef.current) return;
			const description = describeNetworkAwareError(
				cause,
				"可读动态加载失败，请稍后重试。",
			);
			setError({ phase: "initial", ...description, at: Date.now() });
		} finally {
			if (requestId === requestIdRef.current) setLoadingInitial(false);
		}
	}, []);

	useEffect(() => {
		if (!enabled) {
			setLoadingInitial(false);
			setLoadingMore(false);
			setError(null);
			setSections([]);
			setNextCursor(null);
			setDetails({});
			return;
		}
		void loadInitial();
		return () => {
			requestIdRef.current += 1;
		};
	}, [enabled, loadInitial, signature]);

	const loadMore = useCallback(async () => {
		const cursor = nextCursor;
		const requestId = requestIdRef.current;
		const requestKey = `${requestId}:${cursor ?? ""}`;
		if (
			!cursor ||
			loadingMore ||
			loadingInitial ||
			cursorInFlightRef.current.has(requestKey) ||
			cursorCompletedRef.current.has(requestKey)
		) {
			return;
		}
		cursorInFlightRef.current.add(requestKey);
		setLoadingMore(true);
		setError(null);
		try {
			const params = new URLSearchParams({ cursor });
			const response = await apiGet<DashboardReadableFeedResponse>(
				`/api/dashboard/feed?${params.toString()}`,
			);
			if (requestId !== requestIdRef.current) return;
			cursorCompletedRef.current.add(requestKey);
			setSections((current) => {
				const byId = new Map(current.map((section) => [section.id, section]));
				for (const section of response.sections ?? []) {
					const previous = byId.get(section.id);
					byId.set(
						section.id,
						previous
							? {
									...section,
									items: mergeItems(previous.items ?? [], section.items ?? []),
									supplemental_items: mergeItems(
										previous.supplemental_items ?? [],
										section.supplemental_items ?? [],
									),
								}
							: section,
					);
				}
				return Array.from(byId.values());
			});
			setNextCursor(
				response.next_cursor && response.next_cursor !== cursor
					? response.next_cursor
					: null,
			);
		} catch (cause) {
			if (requestId !== requestIdRef.current) return;
			const description = describeNetworkAwareError(
				cause,
				"更多可读动态加载失败，请稍后重试。",
			);
			setError({ phase: "append", ...description, at: Date.now() });
		} finally {
			cursorInFlightRef.current.delete(requestKey);
			if (requestId === requestIdRef.current) setLoadingMore(false);
		}
	}, [loadingInitial, loadingMore, nextCursor]);

	const retry = useCallback(async () => {
		if (error?.phase === "append") {
			await loadMore();
			return;
		}
		await loadInitial();
	}, [error?.phase, loadInitial, loadMore]);

	const loadSectionItems = useCallback(
		async (sectionId: string, cursor?: string | null) => {
			if (!enabled) return;
			const requestId = requestIdRef.current;
			const normalizedCursor = cursor ?? "__initial__";
			const requestKey = `${requestId}:${sectionId}:${normalizedCursor}`;
			const current = detailsRef.current[sectionId];
			if (
				current?.loading ||
				detailCursorInFlightRef.current.has(requestKey) ||
				detailCursorCompletedRef.current.has(requestKey)
			)
				return;
			detailCursorInFlightRef.current.add(requestKey);
			setDetails((previous) => ({
				...previous,
				[sectionId]: {
					...(previous[sectionId] ?? {
						items: [],
						nextCursor: null,
						error: null,
					}),
					loading: true,
					error: null,
				},
			}));
			try {
				const suffix = cursor ? `?cursor=${encodeURIComponent(cursor)}` : "";
				const response = await apiGet<FeedResponse>(
					`/api/dashboard/feed/sections/${encodeURIComponent(sectionId)}/items${suffix}`,
				);
				if (requestId !== requestIdRef.current) return;
				detailCursorCompletedRef.current.add(requestKey);
				setDetails((previous) => {
					const before = previous[sectionId] ?? {
						items: [],
						nextCursor: null,
						loading: false,
						error: null,
					};
					return {
						...previous,
						[sectionId]: {
							items: mergeItems(before.items, response.items ?? []),
							nextCursor:
								response.next_cursor && response.next_cursor !== cursor
									? response.next_cursor
									: null,
							loading: false,
							error: null,
						},
					};
				});
			} catch (cause) {
				if (requestId !== requestIdRef.current) return;
				setDetails((previous) => ({
					...previous,
					[sectionId]: {
						...(previous[sectionId] ?? { items: [], nextCursor: null }),
						loading: false,
						error: describeNetworkAwareError(
							cause,
							"列表加载失败，请稍后重试。",
						).message,
					},
				}));
			} finally {
				detailCursorInFlightRef.current.delete(requestKey);
			}
		},
		[enabled],
	);

	const applyToItems = useCallback((updater: (item: FeedItem) => FeedItem) => {
		setSections((current) => updateItems(current, updater));
		setDetails((current) =>
			Object.fromEntries(
				Object.entries(current).map(([id, detail]) => [
					id,
					{ ...detail, items: detail.items.map(updater) },
				]),
			),
		);
	}, []);

	const applyTranslation = useCallback(
		(target: Pick<FeedItem, "kind" | "id">, translated: TranslatedItem) => {
			const key = itemKey(target);
			applyToItems((item) =>
				itemKey(item) === key && isLaneCapableFeedItem(item)
					? { ...item, translated }
					: item,
			);
		},
		[applyToItems],
	);
	const applySmart = useCallback(
		(target: Pick<FeedItem, "kind" | "id">, smart: SmartItem) => {
			const key = itemKey(target);
			applyToItems((item) =>
				itemKey(item) === key && isLaneCapableFeedItem(item)
					? { ...item, smart }
					: item,
			);
		},
		[applyToItems],
	);
	const applyReactions = useCallback(
		(target: Pick<FeedItem, "kind" | "id">, reactions: ReleaseReactions) => {
			const key = itemKey(target);
			applyToItems((item) =>
				itemKey(item) === key && isReleaseFeedItem(item)
					? { ...item, reactions }
					: item,
			);
		},
		[applyToItems],
	);

	const stats = useMemo(() => {
		const items = sections.flatMap((section) => [
			...(section.items ?? []),
			...(section.supplemental_items ?? []),
		]);
		return {
			total: items.length,
			releases: items.filter((item) => item.kind === "release").length,
			stars: items.filter((item) => item.kind === "repo_star_received").length,
			followers: items.filter((item) => item.kind === "follower_received")
				.length,
		};
	}, [sections]);

	return {
		sections,
		nextCursor,
		hasMore: Boolean(nextCursor),
		loadingInitial,
		loadingMore,
		error,
		details,
		stats,
		loadInitial,
		loadMore,
		retry,
		loadSectionItems,
		applyTranslation,
		applySmart,
		applyReactions,
	};
}
