import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { ApiError, apiGet } from "@/api";
import {
	applySmartIfSourceMatches,
	applyTranslationIfSourceMatches,
} from "@/feed/autoLaneRequests";
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
	authorizationDenied?: boolean;
};

export type ReadableSectionDetails = {
	items: FeedItem[];
	nextCursor: string | null;
	loading: boolean;
	error: string | null;
	refreshPending?: boolean;
};

export type ReadableRefreshResult =
	| "applied"
	| "fallback"
	| "superseded"
	| "failed";

function isAuthorizationDenied(error: unknown) {
	return (
		error instanceof ApiError && (error.status === 401 || error.status === 403)
	);
}

function itemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

function itemContentChanged(previous: FeedItem, next: FeedItem) {
	if (previous.kind !== next.kind || previous.id !== next.id) return true;
	const comparable = (item: FeedItem) => {
		const base = { ...item } as Record<string, unknown>;
		const lanes = isLaneCapableFeedItem(item)
			? { translated: item.translated, smart: item.smart }
			: null;
		delete base.translated;
		delete base.smart;
		return { base, lanes };
	};
	const previousComparable = comparable(previous);
	const nextComparable = comparable(next);
	return JSON.stringify(previousComparable) !== JSON.stringify(nextComparable);
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
	const previousItems = previous.items ?? [];
	const nextItems = next.items ?? [];
	const previousKeys = previousItems.map(itemKey);
	const nextKeys = nextItems.map(itemKey);
	if (
		previousKeys.length !== nextKeys.length ||
		previousKeys.some(
			(key, index) =>
				key !== nextKeys[index] ||
				itemContentChanged(previousItems[index], nextItems[index]),
		)
	)
		return true;
	const previousSupplementalItems = previous.supplemental_items ?? [];
	const nextSupplementalItems = next.supplemental_items ?? [];
	const previousSupplementalKeys = previousSupplementalItems.map(itemKey);
	const nextSupplementalKeys = nextSupplementalItems.map(itemKey);
	return (
		previousSupplementalKeys.length !== nextSupplementalKeys.length ||
		previousSupplementalKeys.some(
			(key, index) =>
				key !== nextSupplementalKeys[index] ||
				itemContentChanged(
					previousSupplementalItems[index],
					nextSupplementalItems[index],
				),
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
	const [stateSignature, setStateSignature] = useState(signature);
	const [sections, setSections] = useState<DashboardReadableSection[]>([]);
	const [nextCursor, setNextCursor] = useState<string | null>(null);
	const [loadingInitial, setLoadingInitial] = useState(true);
	const [loadingRefresh, setLoadingRefresh] = useState(false);
	const [loadingMore, setLoadingMore] = useState(false);
	const [error, setError] = useState<ReadableSectionsError | null>(null);
	const [legacyFallback, setLegacyFallback] = useState(false);
	const legacyFallbackRef = useRef(false);
	const [details, setDetails] = useState<
		Record<string, ReadableSectionDetails>
	>({});
	const sectionsRef = useRef(sections);
	sectionsRef.current = sections;
	const detailsRef = useRef(details);
	detailsRef.current = details;
	const requestIdRef = useRef(0);
	const lifecycleGenerationRef = useRef(0);
	const lastSignatureRef = useRef(signature);
	const contentWriteGenerationRef = useRef(0);
	if (lastSignatureRef.current !== signature) {
		lastSignatureRef.current = signature;
		contentWriteGenerationRef.current += 1;
	}
	const contentWriteGeneration = contentWriteGenerationRef.current;
	const refreshQueueRef = useRef<Promise<ReadableRefreshResult | undefined>>(
		Promise.resolve(undefined),
	);
	const refreshPriorityRef = useRef(0);
	const strictRefreshRef = useRef<Promise<ReadableRefreshResult> | null>(null);
	const refreshInFlightRef = useRef(false);
	const cursorInFlightRef = useRef(new Set<string>());
	const cursorCompletedRef = useRef(new Set<string>());
	const detailCursorInFlightRef = useRef(new Set<string>());
	const detailCursorCompletedRef = useRef(new Set<string>());
	const clearSectionData = useCallback((loadingInitial: boolean) => {
		contentWriteGenerationRef.current += 1;
		lifecycleGenerationRef.current += 1;
		requestIdRef.current += 1;
		refreshPriorityRef.current += 1;
		refreshInFlightRef.current = false;
		cursorInFlightRef.current.clear();
		cursorCompletedRef.current.clear();
		detailCursorInFlightRef.current.clear();
		detailCursorCompletedRef.current.clear();
		sectionsRef.current = [];
		detailsRef.current = {};
		setSections([]);
		setNextCursor(null);
		setLoadingInitial(loadingInitial);
		setLoadingRefresh(false);
		setLoadingMore(false);
		setError(null);
		setDetails({});
	}, []);
	const clearContent = useCallback(
		(options?: { preserveLegacyFallback?: boolean }) => {
			const preserveLegacyFallback = options?.preserveLegacyFallback ?? false;
			clearSectionData(!preserveLegacyFallback);
			legacyFallbackRef.current = preserveLegacyFallback;
			setLegacyFallback(preserveLegacyFallback);
		},
		[clearSectionData],
	);
	const clearContentPreservingMode = useCallback(
		() => clearSectionData(false),
		[clearSectionData],
	);

	const loadSections = useCallback(
		async (
			preserveContent: boolean,
			options?: {
				throwOnError?: boolean;
				onStart?: (cancel: () => void) => void;
				onQueue?: (cancel: () => void) => void;
			},
		) => {
			if (preserveContent && legacyFallbackRef.current) return "fallback";

			const requestId = ++requestIdRef.current;
			let cancelled = false;
			const cancelRefresh = () => {
				if (cancelled || requestId !== requestIdRef.current) return;
				cancelled = true;
				requestIdRef.current += 1;
				refreshInFlightRef.current = false;
				setLoadingRefresh(false);
				setLoadingInitial(false);
			};
			options?.onStart?.(cancelRefresh);
			const isSuperseded = () => requestId !== requestIdRef.current;
			const rejectIfSuperseded = (): ReadableRefreshResult | null => {
				if (!isSuperseded()) return null;
				if (options?.throwOnError) throw new Error("刷新已取消");
				return "superseded";
			};
			const enterLegacyFallback = () => {
				if (legacyFallbackRef.current) return;
				legacyFallbackRef.current = true;
				requestIdRef.current += 1;
				refreshInFlightRef.current = false;
				cursorInFlightRef.current.clear();
				detailCursorInFlightRef.current.clear();
				setLegacyFallback(true);
				setLoadingInitial(false);
				setLoadingRefresh(false);
				setLoadingMore(false);
				setDetails((current) =>
					Object.fromEntries(
						Object.entries(current).map(([sectionId, detail]) => [
							sectionId,
							{ ...detail, loading: false },
						]),
					),
				);
			};
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
				legacyFallbackRef.current = false;
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
			try {
				let response: DashboardReadableFeedResponse;
				try {
					response = await apiGet<DashboardReadableFeedResponse>(
						"/api/dashboard/feed",
					);
				} catch (cause) {
					// Older deployments can explicitly signal that readable evidence is
					// unavailable. Other API failures must remain visible to the user.
					const endpointUnavailable =
						cause instanceof ApiError &&
						cause.code === "release_reading_evidence_unavailable";
					if (!endpointUnavailable) throw cause;
					const superseded = rejectIfSuperseded();
					if (superseded) return superseded;
					enterLegacyFallback();
					return "fallback";
				}
				const superseded = rejectIfSuperseded();
				if (superseded) return superseded;
				legacyFallbackRef.current = false;
				setLegacyFallback(false);
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
							Object.entries(current).flatMap(([sectionId, detail]) => {
								const previous = previousSections.get(sectionId);
								const next = nextSectionsById.get(sectionId);
								if (!previous || !next) return [];
								if (!sectionPageChanged(previous, next)) {
									return [[sectionId, detail]];
								}
								return [
									[
										sectionId,
										{
											...detail,
											nextCursor: null,
											loading: false,
											error: null,
											refreshPending: true,
										},
									],
								];
							}),
						),
					);
				}
				return "applied";
			} catch (cause) {
				const superseded = rejectIfSuperseded();
				if (superseded) return superseded;
				const authorizationDenied = isAuthorizationDenied(cause);
				if (authorizationDenied) {
					clearContent();
					setLoadingInitial(false);
				}
				const description = describeNetworkAwareError(
					cause,
					"可读动态加载失败，请稍后重试。",
				);
				setError({
					phase:
						authorizationDenied || !preserveContent ? "initial" : "refresh",
					...description,
					at: Date.now(),
					authorizationDenied,
				});
				if (options?.throwOnError) throw cause;
				return "failed";
			} finally {
				if (requestId === requestIdRef.current) {
					refreshInFlightRef.current = false;
					setLoadingRefresh(false);
					setLoadingInitial(false);
				}
			}
		},
		[clearContent],
	);

	const loadInitial = useCallback(() => loadSections(false), [loadSections]);
	const refresh = useCallback(
		(options?: {
			throwOnError?: boolean;
			onStart?: (cancel: () => void) => void;
			onQueue?: (cancel: () => void) => void;
		}) => {
			if (legacyFallbackRef.current) {
				return Promise.resolve<ReadableRefreshResult>("fallback");
			}
			const generation = lifecycleGenerationRef.current;
			const priority = options?.throwOnError
				? ++refreshPriorityRef.current
				: refreshPriorityRef.current;
			if (options?.throwOnError) {
				const previous =
					strictRefreshRef.current ??
					Promise.resolve<ReadableRefreshResult>("applied");
				let cancelOperation!: () => void;
				let cancelled = false;
				const released = new Promise<never>((_, reject) => {
					cancelOperation = () => {
						if (cancelled) return;
						cancelled = true;
						reject(new Error("刷新已取消"));
					};
				});
				options.onQueue?.(cancelOperation);
				const operation = previous.then(() => {
					if (
						cancelled ||
						generation !== lifecycleGenerationRef.current ||
						!enabled
					) {
						return Promise.reject(new Error("刷新已取消"));
					}
					if (legacyFallbackRef.current) return "fallback";
					return loadSections(true, {
						...options,
						onStart: (cancel) =>
							options?.onStart?.(() => {
								cancel();
								cancelOperation();
							}),
					});
				});
				const immediate = Promise.race([operation, released]);
				strictRefreshRef.current = immediate.catch(() => "failed");
				refreshQueueRef.current = strictRefreshRef.current;
				return immediate;
			}
			const queued = refreshQueueRef.current.then(
				async (): Promise<ReadableRefreshResult> => {
					if (
						generation !== lifecycleGenerationRef.current ||
						priority !== refreshPriorityRef.current ||
						!enabled
					) {
						if (options?.throwOnError) throw new Error("刷新已取消");
						return "superseded";
					}
					if (legacyFallbackRef.current) return "fallback";
					const result = await loadSections(true, options);
					return result;
				},
			);
			refreshQueueRef.current = queued.catch(() => undefined);
			return queued;
		},
		[enabled, loadSections],
	);

	useEffect(() => {
		if (!enabled) {
			lifecycleGenerationRef.current += 1;
			legacyFallbackRef.current = false;
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
			setStateSignature(signature);
			return;
		}
		setStateSignature(signature);
		void loadInitial();
		return () => {
			lifecycleGenerationRef.current += 1;
			requestIdRef.current += 1;
		};
	}, [enabled, loadInitial, signature]);

	const loadMore = useCallback(async () => {
		const cursor = nextCursor;
		const requestId = requestIdRef.current;
		const requestKey = `${requestId}:${cursor ?? ""}`;
		if (
			legacyFallbackRef.current ||
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
			if (requestId !== requestIdRef.current || legacyFallbackRef.current)
				return;
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
			if (requestId !== requestIdRef.current || legacyFallbackRef.current)
				return;
			const authorizationDenied = isAuthorizationDenied(cause);
			if (authorizationDenied) {
				clearContent();
				setLoadingInitial(false);
			}
			const description = describeNetworkAwareError(
				cause,
				"更多可读动态加载失败，请稍后重试。",
			);
			setError({
				phase: authorizationDenied ? "initial" : "append",
				...description,
				at: Date.now(),
				authorizationDenied,
			});
		} finally {
			cursorInFlightRef.current.delete(requestKey);
			if (requestId === requestIdRef.current) setLoadingMore(false);
		}
	}, [clearContent, loadingInitial, loadingMore, nextCursor]);

	const retry = useCallback(async () => {
		if (error?.phase === "append") {
			return loadMore();
		}
		if (error?.phase === "refresh") {
			return refresh();
		}
		return loadInitial();
	}, [error?.phase, loadInitial, loadMore, refresh]);

	const loadSectionItems = useCallback(
		async (sectionId: string, cursor?: string | null) => {
			if (!enabled || legacyFallbackRef.current) return;
			const requestId = requestIdRef.current;
			const normalizedCursor = cursor ?? "__initial__";
			const requestKey = `${requestId}:${sectionId}:${normalizedCursor}`;
			const current = detailsRef.current[sectionId];
			const replaceAfterRefresh = Boolean(current?.refreshPending && !cursor);
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
				if (requestId !== requestIdRef.current || legacyFallbackRef.current)
					return;
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
							items: replaceAfterRefresh
								? (response.items ?? [])
								: mergeItems(before.items, response.items ?? []),
							nextCursor:
								response.next_cursor && response.next_cursor !== cursor
									? response.next_cursor
									: null,
							loading: false,
							error: null,
							refreshPending: false,
						},
					};
				});
			} catch (cause) {
				if (requestId !== requestIdRef.current || legacyFallbackRef.current)
					return;
				if (isAuthorizationDenied(cause)) {
					clearContent();
					setLoadingInitial(false);
					setError({
						phase: "initial",
						...describeNetworkAwareError(
							cause,
							"可读动态加载失败，请稍后重试。",
						),
						at: Date.now(),
						authorizationDenied: true,
					});
					return;
				}
				setDetails((previous) => ({
					...previous,
					[sectionId]: {
						...(previous[sectionId] ?? { items: [], nextCursor: null }),
						loading: false,
						error: describeNetworkAwareError(
							cause,
							"列表加载失败，请稍后重试。",
						).message,
						refreshPending: false,
					},
				}));
			} finally {
				detailCursorInFlightRef.current.delete(requestKey);
				if (requestId === requestIdRef.current && legacyFallbackRef.current) {
					setDetails((previous) => {
						const detail = previous[sectionId];
						if (!detail?.loading) return previous;
						return {
							...previous,
							[sectionId]: {
								...detail,
								loading: false,
								refreshPending: false,
							},
						};
					});
				}
			}
		},
		[clearContent, enabled],
	);

	const applyToItems = useCallback(
		(updater: (item: FeedItem) => FeedItem) => {
			if (contentWriteGeneration !== contentWriteGenerationRef.current) return;
			setSections((current) =>
				contentWriteGeneration === contentWriteGenerationRef.current
					? updateItems(current, updater)
					: current,
			);
			setDetails((current) =>
				contentWriteGeneration === contentWriteGenerationRef.current
					? Object.fromEntries(
							Object.entries(current).map(([id, detail]) => [
								id,
								{ ...detail, items: detail.items.map(updater) },
							]),
						)
					: current,
			);
		},
		[contentWriteGeneration],
	);

	const applyTranslation = useCallback(
		(
			target: Pick<FeedItem, "kind" | "id">,
			sourceKey: string,
			translated: TranslatedItem,
		) => {
			const key = itemKey(target);
			applyToItems((item) =>
				itemKey(item) === key
					? applyTranslationIfSourceMatches(item, sourceKey, translated)
					: item,
			);
		},
		[applyToItems],
	);
	const applySmart = useCallback(
		(
			target: Pick<FeedItem, "kind" | "id">,
			sourceKey: string,
			smart: SmartItem,
		) => {
			const key = itemKey(target);
			applyToItems((item) =>
				itemKey(item) === key
					? applySmartIfSourceMatches(item, sourceKey, smart)
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

	const stateCurrent = stateSignature === signature;
	const visibleSections = stateCurrent ? sections : [];
	const visibleDetails = stateCurrent ? details : {};
	const stats = useMemo(() => {
		const items = visibleSections.flatMap((section) => [
			...(section.items ?? []),
			...(section.supplemental_items ?? []),
			...(visibleDetails[section.id]?.items ?? []),
		]);
		return {
			total: items.length,
			releases: items.filter((item) => item.kind === "release").length,
			stars: items.filter((item) => item.kind === "repo_star_received").length,
			followers: items.filter((item) => item.kind === "follower_received")
				.length,
		};
	}, [visibleDetails, visibleSections]);

	return {
		sections: visibleSections,
		nextCursor: stateCurrent ? nextCursor : null,
		hasMore: stateCurrent && Boolean(nextCursor),
		loadingInitial: !stateCurrent || loadingInitial,
		loadingRefresh: stateCurrent && loadingRefresh,
		loadingMore: stateCurrent && loadingMore,
		error: stateCurrent ? error : null,
		legacyFallback: stateCurrent && legacyFallback,
		details: visibleDetails,
		stats,
		clearContent,
		clearContentPreservingMode,
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
