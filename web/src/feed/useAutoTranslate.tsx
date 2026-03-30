import { useCallback, useEffect, useRef, useState } from "react";

import {
	apiResolveTranslationResults,
	type TranslationRequestItemInput,
	type TranslationResultItem,
	isPendingTranslationResultStatus,
} from "@/api";
import type { FeedItem, TranslateResponse } from "@/feed/types";

const SECONDARY_PREFETCH_COUNT = 10;
const REQUEST_ERROR_RECOVERY_MAX_RETRIES = 3;
const REQUEST_STATUS_POLL_INTERVAL_MS = 250;
const AUTO_TRANSLATE_MAX_WAIT_MS = 500;
const REQUEST_STATUS_POLL_WINDOW_MS = 20_000;
const REQUEST_RESUME_WINDOW_MAX_RETRIES = 15;
const REQUEST_PENDING_MAX_AGE_MS =
	REQUEST_STATUS_POLL_WINDOW_MS * REQUEST_RESUME_WINDOW_MAX_RETRIES;

function buildReleaseSummaryRequestItem(
	item: FeedItem,
): TranslationRequestItemInput {
	const title = item.title?.trim() || `release:${item.id}`;
	const excerpt = item.excerpt?.trim();
	const metadata = [item.repo_full_name, item.reason, item.subject_type]
		.filter((value): value is string => Boolean(value?.trim()))
		.join("\n");
	return {
		producer_ref: `feed.auto_translate:release:${item.id}`,
		kind: "release_summary",
		variant: "feed_card",
		entity_id: item.id,
		target_lang: "zh-CN",
		max_wait_ms: AUTO_TRANSLATE_MAX_WAIT_MS,
		source_blocks: [
			{ slot: "title", text: title },
			...(excerpt ? [{ slot: "excerpt" as const, text: excerpt }] : []),
			...(metadata ? [{ slot: "metadata" as const, text: metadata }] : []),
		],
		target_slots: ["title_zh", "summary_md"],
	};
}

function mapTranslationItemToFeedResponse(item: {
	status: "ready" | "disabled" | "missing" | "error" | "queued" | "running";
	title_zh: string | null;
	summary_md: string | null;
}): TranslateResponse | null {
	if (item.status !== "ready" && item.status !== "disabled") {
		return null;
	}
	return {
		lang: "zh-CN",
		status: item.status === "disabled" ? "disabled" : "ready",
		title: item.title_zh,
		summary: item.summary_md,
	};
}

function keyOf(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

type Deferred<T> = {
	promise: Promise<T>;
	resolve: (value: T | PromiseLike<T>) => void;
	reject: (reason?: unknown) => void;
};

type TranslationTask = {
	sourceKey: string;
	requestItem: TranslationRequestItemInput;
	createdAtMs: number;
	deferred: Deferred<TranslateResponse | null>;
	promise: Promise<TranslateResponse | null>;
};

type TranslationCandidate = {
	key: string;
	item: FeedItem;
	requestItem: TranslationRequestItemInput;
	sourceKey: string;
	top: number;
	bottom: number;
};

function createDeferred<T>(): Deferred<T> {
	let resolve!: (value: T | PromiseLike<T>) => void;
	let reject!: (reason?: unknown) => void;
	const promise = new Promise<T>((nextResolve, nextReject) => {
		resolve = nextResolve;
		reject = nextReject;
	});
	return { promise, resolve, reject };
}

function buildRequestSourceKey(item: TranslationRequestItemInput) {
	return JSON.stringify({
		producer_ref: item.producer_ref,
		kind: item.kind,
		variant: item.variant,
		entity_id: item.entity_id,
		target_lang: item.target_lang,
		source_blocks: item.source_blocks,
		target_slots: item.target_slots,
	});
}

function intersectsViewportRange(
	top: number,
	bottom: number,
	rangeTop: number,
	rangeBottom: number,
) {
	return bottom > rangeTop && top < rangeBottom;
}

function buildVisibleWindowPlan(
	candidates: TranslationCandidate[],
	viewportHeight: number,
) {
	if (viewportHeight <= 0 || candidates.length === 0) {
		return [] as TranslationCandidate[];
	}

	const visible: TranslationCandidate[] = [];
	let lastVisibleIndex = -1;
	for (let index = 0; index < candidates.length; index += 1) {
		const candidate = candidates[index];
		if (
			intersectsViewportRange(
				candidate.top,
				candidate.bottom,
				0,
				viewportHeight,
			)
		) {
			visible.push(candidate);
			lastVisibleIndex = index;
		}
	}

	if (lastVisibleIndex < 0) {
		return candidates.slice(0, SECONDARY_PREFETCH_COUNT + 1);
	}

	return [
		...visible,
		...candidates.slice(
			lastVisibleIndex + 1,
			lastVisibleIndex + 1 + SECONDARY_PREFETCH_COUNT,
		),
	];
}

function resultLabel(result: TranslationResultItem) {
	return result.error ?? `translate returned ${result.status}`;
}

export function useAutoTranslate(params: {
	enabled: boolean;
	onTranslated: (
		item: Pick<FeedItem, "kind" | "id">,
		res: TranslateResponse,
	) => void;
}) {
	const { enabled, onTranslated } = params;

	const mountedRef = useRef(false);
	const resizeObserverRef = useRef<ResizeObserver | null>(null);
	const plannerFrameRef = useRef<number | null>(null);
	const plannerBusyRef = useRef(false);
	const plannerDirtyRef = useRef(false);
	const pollTimerRef = useRef<number | null>(null);
	const pollBusyRef = useRef(false);

	const keyToElementRef = useRef(new Map<string, HTMLElement>());
	const itemByKeyRef = useRef(new Map<string, FeedItem>());
	const requestTasksRef = useRef(new Map<string, TranslationTask>());
	const failedRef = useRef(new Set<string>());
	const retryCountRef = useRef(new Map<string, number>());
	const pollErrorCountRef = useRef(new Map<string, number>());

	const [, forceRender] = useState(0);

	const bumpRender = useCallback(() => {
		if (!mountedRef.current) return;
		forceRender((value) => value + 1);
	}, []);

	const shouldAutoTranslate = useCallback(
		(item: FeedItem) =>
			enabled &&
			item.translated?.status === "missing" &&
			!failedRef.current.has(keyOf(item)),
		[enabled],
	);

	const clearTask = useCallback(
		(key: string, task: TranslationTask) => {
			if (requestTasksRef.current.get(key) === task) {
				requestTasksRef.current.delete(key);
				pollErrorCountRef.current.delete(key);
				bumpRender();
			}
		},
		[bumpRender],
	);

	const scheduleViewportPlanRef = useRef<() => void>(() => {});
	const schedulePendingPollRef = useRef<() => void>(() => {});

	const finalizeSuccess = useCallback(
		(
			candidate: TranslationCandidate,
			task: TranslationTask,
			mapped: TranslateResponse | null,
		) => {
			clearTask(candidate.key, task);
			if (!mapped) {
				failedRef.current.add(candidate.key);
			} else {
				failedRef.current.delete(candidate.key);
				retryCountRef.current.delete(candidate.key);
				onTranslated(
					{ kind: candidate.item.kind, id: candidate.item.id },
					mapped,
				);
			}
			scheduleViewportPlanRef.current();
		},
		[clearTask, onTranslated],
	);

	const finalizeFailure = useCallback(
		(
			candidate: TranslationCandidate,
			task: TranslationTask,
			error?: unknown,
		) => {
			clearTask(candidate.key, task);
			const retries = retryCountRef.current.get(candidate.key) ?? 0;
			if (retries + 1 >= REQUEST_ERROR_RECOVERY_MAX_RETRIES) {
				failedRef.current.add(candidate.key);
				retryCountRef.current.delete(candidate.key);
			} else {
				retryCountRef.current.set(candidate.key, retries + 1);
			}
			task.deferred.reject(error);
			scheduleViewportPlanRef.current();
		},
		[clearTask],
	);

	const resolveRequestItems = useCallback(
		async (
			items: TranslationRequestItemInput[],
			options?: { retryOnError?: boolean },
		) => {
			return apiResolveTranslationResults({
				items,
				retry_on_error: options?.retryOnError ?? false,
			});
		},
		[],
	);

	const applyResolvedResults = useCallback(
		(
			candidates: Array<{
				candidate: TranslationCandidate;
				task: TranslationTask;
			}>,
			results: TranslationResultItem[],
		) => {
			const resultsByProducerRef = new Map(
				results.map((result) => [result.producer_ref, result]),
			);

			for (const { candidate, task } of candidates) {
				if (requestTasksRef.current.get(candidate.key) !== task) {
					continue;
				}

				pollErrorCountRef.current.delete(candidate.key);
				const resolved = resultsByProducerRef.get(
					candidate.requestItem.producer_ref,
				);
				if (!resolved) {
					finalizeFailure(
						candidate,
						task,
						new Error("translation result missing from resolve response"),
					);
					continue;
				}

				if (isPendingTranslationResultStatus(resolved.status)) {
					if (Date.now() - task.createdAtMs > REQUEST_PENDING_MAX_AGE_MS) {
						finalizeFailure(
							candidate,
							task,
							new Error("translation request exceeded resume window"),
						);
					}
					continue;
				}

				const mapped = mapTranslationItemToFeedResponse(resolved);
				if (!mapped) {
					finalizeFailure(candidate, task, new Error(resultLabel(resolved)));
					continue;
				}

				task.deferred.resolve(mapped);
				finalizeSuccess(candidate, task, mapped);
			}
		},
		[finalizeFailure, finalizeSuccess],
	);

	const pollPendingTasks = useCallback(async () => {
		if (!enabled || !mountedRef.current || pollBusyRef.current) return;
		pollBusyRef.current = true;
		try {
			const pending = Array.from(requestTasksRef.current.entries()).map(
				([key, task]) => {
					const item = itemByKeyRef.current.get(key);
					if (!item) return null;
					return {
						candidate: {
							key,
							item,
							requestItem: task.requestItem,
							sourceKey: task.sourceKey,
							top: 0,
							bottom: 0,
						},
						task,
					};
				},
			);
			const active = pending.filter(
				(
					entry,
				): entry is {
					candidate: TranslationCandidate;
					task: TranslationTask;
				} => Boolean(entry),
			);
			if (active.length === 0) return;

			const response = await resolveRequestItems(
				active.map(({ task }) => task.requestItem),
			);
			applyResolvedResults(active, response.items);
		} catch (error) {
			for (const [key, task] of requestTasksRef.current) {
				const item = itemByKeyRef.current.get(key);
				if (!item) continue;
				const count = (pollErrorCountRef.current.get(key) ?? 0) + 1;
				if (count >= REQUEST_ERROR_RECOVERY_MAX_RETRIES) {
					finalizeFailure(
						{
							key,
							item,
							requestItem: task.requestItem,
							sourceKey: task.sourceKey,
							top: 0,
							bottom: 0,
						},
						task,
						error,
					);
				} else {
					pollErrorCountRef.current.set(key, count);
				}
			}
		} finally {
			pollBusyRef.current = false;
			if (requestTasksRef.current.size > 0) {
				schedulePendingPollRef.current();
			}
		}
	}, [applyResolvedResults, enabled, finalizeFailure, resolveRequestItems]);

	const schedulePendingPoll = useCallback(() => {
		if (!enabled || !mountedRef.current || requestTasksRef.current.size === 0)
			return;
		if (pollTimerRef.current !== null || pollBusyRef.current) return;
		pollTimerRef.current = window.setTimeout(() => {
			pollTimerRef.current = null;
			void pollPendingTasks();
		}, REQUEST_STATUS_POLL_INTERVAL_MS);
	}, [enabled, pollPendingTasks]);

	schedulePendingPollRef.current = schedulePendingPoll;

	const submitCandidates = useCallback(
		async (rawCandidates: TranslationCandidate[]) => {
			const byKey = new Map<string, Promise<TranslateResponse | null>>();
			const candidates: Array<{
				candidate: TranslationCandidate;
				task: TranslationTask;
			}> = [];

			for (const candidate of rawCandidates) {
				const latestItem =
					itemByKeyRef.current.get(candidate.key) ?? candidate.item;
				const requestItem = buildReleaseSummaryRequestItem(latestItem);
				const sourceKey = buildRequestSourceKey(requestItem);
				const existing = requestTasksRef.current.get(candidate.key);
				if (existing && existing.sourceKey === sourceKey) {
					byKey.set(candidate.key, existing.promise);
					continue;
				}
				if (existing) {
					byKey.set(candidate.key, existing.promise);
					continue;
				}

				const deferred = createDeferred<TranslateResponse | null>();
				const task: TranslationTask = {
					sourceKey,
					requestItem,
					createdAtMs: Date.now(),
					deferred,
					promise: deferred.promise,
				};
				requestTasksRef.current.set(candidate.key, task);
				byKey.set(candidate.key, deferred.promise);
				candidates.push({
					candidate: {
						...candidate,
						item: latestItem,
						requestItem,
						sourceKey,
					},
					task,
				});
			}

			if (candidates.length === 0) {
				return byKey;
			}

			bumpRender();

			try {
				const response = await resolveRequestItems(
					candidates.map(({ candidate }) => candidate.requestItem),
				);
				applyResolvedResults(candidates, response.items);
			} catch (error) {
				for (const { candidate, task } of candidates) {
					finalizeFailure(candidate, task, error);
				}
				return byKey;
			}

			if (requestTasksRef.current.size > 0) {
				schedulePendingPoll();
			}

			return byKey;
		},
		[
			applyResolvedResults,
			bumpRender,
			finalizeFailure,
			resolveRequestItems,
			schedulePendingPoll,
		],
	);

	const prepareCandidates = useCallback(() => {
		const candidates: TranslationCandidate[] = [];
		for (const [key, element] of keyToElementRef.current) {
			const item = itemByKeyRef.current.get(key);
			if (!item || !shouldAutoTranslate(item)) continue;
			if (requestTasksRef.current.has(key)) continue;
			const requestItem = buildReleaseSummaryRequestItem(item);
			const rect = element.getBoundingClientRect();
			if (rect.bottom <= 0) continue;
			candidates.push({
				key,
				item,
				requestItem,
				sourceKey: buildRequestSourceKey(requestItem),
				top: rect.top,
				bottom: rect.bottom,
			});
		}

		return candidates.sort((left, right) => {
			if (left.top !== right.top) {
				return left.top - right.top;
			}
			return left.bottom - right.bottom;
		});
	}, [shouldAutoTranslate]);

	const runViewportPlan = useCallback(async () => {
		if (!enabled || !mountedRef.current) return;
		if (plannerBusyRef.current) {
			plannerDirtyRef.current = true;
			return;
		}

		plannerBusyRef.current = true;
		try {
			do {
				plannerDirtyRef.current = false;
				const viewportHeight =
					window.innerHeight || document.documentElement.clientHeight || 0;
				const plan = buildVisibleWindowPlan(
					prepareCandidates(),
					viewportHeight,
				);
				if (plan.length > 0) {
					await submitCandidates(plan);
				}
			} while (plannerDirtyRef.current);
		} finally {
			plannerBusyRef.current = false;
		}
	}, [enabled, prepareCandidates, submitCandidates]);

	const scheduleViewportPlan = useCallback(() => {
		if (!enabled || !mountedRef.current) return;
		if (plannerFrameRef.current !== null) return;
		plannerFrameRef.current = window.requestAnimationFrame(() => {
			plannerFrameRef.current = null;
			void runViewportPlan();
		});
	}, [enabled, runViewportPlan]);

	scheduleViewportPlanRef.current = scheduleViewportPlan;

	const register = useCallback(
		(item: FeedItem) => (element: HTMLElement | null) => {
			const key = keyOf(item);
			itemByKeyRef.current.set(key, item);

			const previous = keyToElementRef.current.get(key);
			if (previous && previous !== element) {
				resizeObserverRef.current?.unobserve(previous);
				keyToElementRef.current.delete(key);
			}

			if (!element || !enabled) {
				if (!element && previous) {
					keyToElementRef.current.delete(key);
				}
				scheduleViewportPlanRef.current();
				return;
			}

			keyToElementRef.current.set(key, element);
			resizeObserverRef.current?.observe(element);
			scheduleViewportPlanRef.current();
		},
		[enabled],
	);

	const translateNow = useCallback(
		async (item: FeedItem) => {
			const key = keyOf(item);
			const requestItem = buildReleaseSummaryRequestItem(item);
			const sourceKey = buildRequestSourceKey(requestItem);
			failedRef.current.delete(key);
			retryCountRef.current.delete(key);

			const existing = requestTasksRef.current.get(key);
			if (existing && existing.sourceKey === sourceKey) {
				return existing.promise;
			}

			const candidate: TranslationCandidate = {
				key,
				item,
				requestItem,
				sourceKey,
				top: 0,
				bottom: 0,
			};

			const deferred = createDeferred<TranslateResponse | null>();
			const task: TranslationTask = {
				sourceKey,
				requestItem,
				createdAtMs: Date.now(),
				deferred,
				promise: deferred.promise,
			};
			requestTasksRef.current.set(key, task);
			bumpRender();
			try {
				const response = await resolveRequestItems([requestItem], {
					retryOnError: true,
				});
				applyResolvedResults([{ candidate, task }], response.items);
				if (requestTasksRef.current.size > 0) {
					schedulePendingPollRef.current();
				}
				return await task.promise;
			} catch (error) {
				finalizeFailure(candidate, task, error);
				failedRef.current.add(key);
				throw error;
			}
		},
		[applyResolvedResults, bumpRender, finalizeFailure, resolveRequestItems],
	);

	useEffect(() => {
		mountedRef.current = true;
		return () => {
			mountedRef.current = false;
		};
	}, []);

	useEffect(() => {
		if (!enabled) {
			if (plannerFrameRef.current !== null) {
				window.cancelAnimationFrame(plannerFrameRef.current);
				plannerFrameRef.current = null;
			}
			if (pollTimerRef.current !== null) {
				window.clearTimeout(pollTimerRef.current);
				pollTimerRef.current = null;
			}
			resizeObserverRef.current?.disconnect();
			resizeObserverRef.current = null;
			return;
		}

		resizeObserverRef.current = new ResizeObserver(() => {
			scheduleViewportPlan();
		});

		for (const element of keyToElementRef.current.values()) {
			resizeObserverRef.current.observe(element);
		}

		const handleViewportChange = () => {
			scheduleViewportPlan();
		};

		window.addEventListener("scroll", handleViewportChange, { passive: true });
		window.addEventListener("resize", handleViewportChange);
		scheduleViewportPlan();
		schedulePendingPoll();

		return () => {
			window.removeEventListener("scroll", handleViewportChange);
			window.removeEventListener("resize", handleViewportChange);
			if (plannerFrameRef.current !== null) {
				window.cancelAnimationFrame(plannerFrameRef.current);
				plannerFrameRef.current = null;
			}
			if (pollTimerRef.current !== null) {
				window.clearTimeout(pollTimerRef.current);
				pollTimerRef.current = null;
			}
			resizeObserverRef.current?.disconnect();
			resizeObserverRef.current = null;
		};
	}, [enabled, schedulePendingPoll, scheduleViewportPlan]);

	const inFlightKeys = new Set(requestTasksRef.current.keys());

	return { register, translateNow, inFlightKeys };
}
