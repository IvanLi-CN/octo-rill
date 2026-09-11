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
	phase: "initial" | "refresh" | "append";
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

function sectionPageChanged(
	previous: DashboardReadableSection,
	next: DashboardReadableSection,
) {
	if (
		(previous.items_next_cursor ?? null) !== (next.items_next_cursor ?? null) ||
		(previous.item_count ?? previous.activity_count ?? 0) !==
			(next.item_count ?? next.activity_count ?? 0)
	)
		return true;
	const previousKeys = (previous.items ?? []).map(itemKey);
	const nextKeys = (next.items ?? []).map(itemKey);
	if (
		previousKeys.length !== nextKeys.length ||
		previousKeys.some((key, index) => key !== nextKeys[index])
	)
		return true;
	const previousSupplementalKeys = (previous.supplemental_items ?? []).map(
		itemKey,
	);
	const nextSupplementalKeys = (next.supplemental_items ?? []).map(itemKey);
	return (
		previousSupplementalKeys.length !== nextSupplementalKeys.length ||
		previousSupplementalKeys.some(
			(key, index) => key !== nextSupplementalKeys[index],
		)
	);
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
	const [loadingRefresh, setLoadingRefresh] = useState(false);
	const [loadingMore, setLoadingMore] = useState(false);
	const [error, setError] = useState<ReadableSectionsError | null>(null);
	const [legacyFallback, setLegacyFallback] = useState(false);
	const [details, setDetails] = useState<
		Record<string, ReadableSectionDetails>
	>({});
	const sectionsRef = useRef(sections);
	sectionsRef.current = sections;
	const detailsRef = useRef(details);
	detailsRef.current = details;
	const requestIdRef = useRef(0);
	const refreshInFlightRef = useRef(false);
	const cursorInFlightRef = useRef(new Set<string>());
	const cursorCompletedRef = useRef(new Set<string>());
	const detailCursorInFlightRef = useRef(new Set<string>());
	const detailCursorCompletedRef = useRef(new Set<string>());

	const loadSections = useCallback(async (preserveContent: boolean) => {
		const requestId = ++requestIdRef.current;
		refreshInFlightRef.current = preserveContent;
		cursorInFlightRef.current.clear();
		cursorCompletedRef.current.clear();
		detailCursorInFlightRef.current.clear();
		detailCursorCompletedRef.current.clear();
		if (!preserveContent) {
			setLoadingInitial(true);
		}
		setLoadingRefresh(preserveContent);
		setLoadingMore(false);
		setError(null);
		if (!preserveContent) {
			setLegacyFallback(false);
			setSections([]);
			sectionsRef.current = [];
			setNextCursor(null);
			setDetails({});
		} else {
			setDetails((current) =>
				Object.fromEntries(
					Object.entries(current).map(([sectionId, detail]) => [
						sectionId,
						{ ...detail, loading: false },
					]),
				),
			);
		}
		let usedLegacyFallback = false;
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
				const endpointUnavailable =
					(cause instanceof ApiError && cause.status === 404) ||
					cause instanceof TypeError;
				if (!endpointUnavailable) throw cause;
				const legacy = await apiGet<FeedResponse>("/api/feed?limit=30");
				if (requestId !== requestIdRef.current) return;
				usedLegacyFallback = true;
				const legacyItems = legacy.items ?? [];
				const firstTimestamp = legacyItems[0]?.ts ?? new Date(0).toISOString();
				response = {
					sections:
						legacyItems.length > 0
							? [
									{
										id: "legacy-feed",
										date: firstTimestamp.slice(0, 10),
										kind: "raw",
										item_count: legacyItems.length,
										brief: null,
										items: legacyItems,
										items_next_cursor: null,
										supplemental_items: [],
									},
								]
							: [],
					next_cursor: legacy.next_cursor ?? null,
				};
			}
			if (requestId !== requestIdRef.current) return;
			setLegacyFallback(usedLegacyFallback);
			const nextSections = response.sections ?? [];
			const previousSections = new Map(
				sectionsRef.current.map((section) => [section.id, section]),
			);
			setSections(nextSections);
			setNextCursor(response.next_cursor ?? null);
			if (preserveContent) {
				const nextSectionsById = new Map(
					nextSections.map((section) => [section.id, section]),
				);
				setDetails((current) =>
					Object.fromEntries(
						Object.entries(current).filter(([sectionId]) => {
							const previous = previousSections.get(sectionId);
							const next = nextSectionsById.get(sectionId);
							return Boolean(
								previous && next && !sectionPageChanged(previous, next),
							);
						}),
					),
				);
			}
		} catch (cause) {
			if (requestId !== requestIdRef.current) return;
			const description = describeNetworkAwareError(
				cause,
				"可读动态加载失败，请稍后重试。",
			);
			setError({
				phase: preserveContent ? "refresh" : "initial",
				...description,
				at: Date.now(),
			});
		} finally {
			if (requestId === requestIdRef.current) {
				refreshInFlightRef.current = false;
				setLoadingRefresh(false);
				setLoadingInitial(false);
			}
		}
	}, []);

	const loadInitial = useCallback(() => loadSections(false), [loadSections]);
	const refresh = useCallback(() => loadSections(true), [loadSections]);

	useEffect(() => {
		if (!enabled) {
			refreshInFlightRef.current = false;
			setLoadingRefresh(false);
			setLoadingInitial(false);
			setLoadingMore(false);
			setError(null);
			setLegacyFallback(false);
			setSections([]);
			sectionsRef.current = [];
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
			refreshInFlightRef.current ||
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
		if (error?.phase === "refresh") {
			await refresh();
			return;
		}
		await loadInitial();
	}, [error?.phase, loadInitial, loadMore, refresh]);

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
		loadingRefresh,
		loadingMore,
		error,
		legacyFallback,
		details,
		stats,
		loadInitial,
		refresh,
		loadMore,
		retry,
		loadSectionItems,
		applyTranslation,
		applySmart,
		applyReactions,
	};
}
