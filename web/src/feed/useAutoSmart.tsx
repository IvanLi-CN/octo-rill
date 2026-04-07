import { useCallback, useEffect, useRef, useState } from "react";

import {
	apiResolveTranslationResults,
	type TranslationRequestItemInput,
	type TranslationResultItem,
	isPendingTranslationResultStatus,
} from "@/api";
import type { FeedItem, SmartItem } from "@/feed/types";

const RESOLVE_RESULTS_MAX_ITEMS = 60;
const SECONDARY_PREFETCH_COUNT = 10;
const INITIAL_SMART_PREFETCH_COUNT = 12;
const REQUEST_ERROR_RECOVERY_MAX_RETRIES = 3;
const REQUEST_STATUS_POLL_INTERVAL_MS = 250;
const AUTO_SMART_MAX_WAIT_MS = 500;
const REQUEST_STATUS_POLL_WINDOW_MS = 20_000;
const REQUEST_RESUME_WINDOW_MAX_RETRIES = 15;
const REQUEST_PENDING_MAX_AGE_MS =
	REQUEST_STATUS_POLL_WINDOW_MS * REQUEST_RESUME_WINDOW_MAX_RETRIES;
const SMART_INSUFFICIENT_REASON = "no_valuable_version_info";

function smartErrorIsRetryable(error?: string | null) {
	if (!error) return false;
	const normalized = error.trim().toLowerCase();
	return (
		normalized.includes("runtime_lease_expired") ||
		normalized.includes("repo scope required; re-login via github oauth") ||
		normalized.includes("database is locked") ||
		normalized.includes("busy") ||
		normalized.includes("timeout") ||
		normalized.includes("timed out") ||
		normalized.includes("temporarily unavailable") ||
		normalized.includes("connection reset") ||
		normalized.includes("connection refused")
	);
}

type SmartResolveResponse = {
	lang: string;
	status: "ready" | "disabled";
	title: string | null;
	summary: string | null;
};

function buildReleaseSmartRequestItem(
	item: FeedItem,
): TranslationRequestItemInput {
	const title = item.title?.trim() || `release:${item.id}`;
	const body = item.body?.trim();
	const metadata = [item.repo_full_name, title]
		.filter((value): value is string => Boolean(value?.trim()))
		.join("\n");
	return {
		producer_ref: `feed.smart:release:${item.id}`,
		kind: "release_smart",
		variant: "feed_card",
		entity_id: item.id,
		target_lang: "zh-CN",
		max_wait_ms: AUTO_SMART_MAX_WAIT_MS,
		source_blocks: [
			{ slot: "title", text: title },
			...(body ? [{ slot: "body_markdown" as const, text: body }] : []),
			...(metadata ? [{ slot: "metadata" as const, text: metadata }] : []),
		],
		target_slots: ["title_zh", "body_md"],
	};
}

function mapTranslationItemToFeedSmart(item: {
	status: "ready" | "disabled" | "missing" | "error" | "queued" | "running";
	title_zh: string | null;
	summary_md: string | null;
	body_md?: string | null;
	error?: string | null;
}): SmartItem | null {
	const summary = item.body_md ?? item.summary_md;
	switch (item.status) {
		case "ready":
			return {
				lang: "zh-CN",
				status: "ready",
				title: item.title_zh,
				summary,
			};
		case "disabled":
			return {
				lang: "zh-CN",
				status: "disabled",
				title: null,
				summary: null,
			};
		case "missing":
			return {
				lang: "zh-CN",
				status:
					item.error === SMART_INSUFFICIENT_REASON ? "insufficient" : "missing",
				title: null,
				summary: null,
				auto_translate: false,
			};
		case "error":
			return {
				lang: "zh-CN",
				status: smartErrorIsRetryable(item.error) ? "missing" : "error",
				title: null,
				summary: null,
				auto_translate: smartErrorIsRetryable(item.error),
			};
		default:
			return null;
	}
}

function mapSmartToResolveResponse(
	item: SmartItem | null,
): SmartResolveResponse | null {
	if (!item || (item.status !== "ready" && item.status !== "disabled")) {
		return null;
	}
	return {
		lang: "zh-CN",
		status: item.status === "disabled" ? "disabled" : "ready",
		title: item.title,
		summary: item.summary,
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
	rejectOnFailure: boolean;
	deferred: Deferred<SmartResolveResponse | null>;
	promise: Promise<SmartResolveResponse | null>;
};

type TranslationCandidate = {
	key: string;
	item: FeedItem;
	requestItem: TranslationRequestItemInput;
	sourceKey: string;
	top: number;
	bottom: number;
};

type ViewportPlanEntry = {
	key: string;
	top: number;
	bottom: number;
	candidate: TranslationCandidate | null;
};

type CandidateTask = {
	candidate: TranslationCandidate;
	task: TranslationTask;
};

type WindowEntry = {
	key: string;
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

function buildVisibleWindowPlan<T extends { top: number; bottom: number }>(
	candidates: T[],
	viewportHeight: number,
) {
	if (viewportHeight <= 0 || candidates.length === 0) {
		return [] as T[];
	}

	const visible: T[] = [];
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

function buildVisibleWindowKeys(
	candidates: WindowEntry[],
	viewportHeight: number,
) {
	return new Set(
		buildVisibleWindowPlan(candidates, viewportHeight).map(
			(entry) => entry.key,
		),
	);
}

function resultLabel(result: TranslationResultItem) {
	return result.error ?? `translate returned ${result.status}`;
}

function isTerminalTranslationResultStatus(
	status: TranslationResultItem["status"],
) {
	return status === "error" || status === "missing";
}

export function useAutoSmart(params: {
	enabled: boolean;
	onSmart: (item: Pick<FeedItem, "kind" | "id">, smart: SmartItem) => void;
}) {
	const { enabled, onSmart } = params;

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

	const shouldAutoSmart = useCallback(
		(item: FeedItem) =>
			enabled &&
			((item.smart?.status === "missing" &&
				item.smart.auto_translate !== false) ||
				(item.smart?.status === "ready" &&
					item.smart.auto_translate === true)) &&
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

	const retireTask = useCallback((key: string, task: TranslationTask) => {
		if (requestTasksRef.current.get(key) === task) {
			requestTasksRef.current.delete(key);
			pollErrorCountRef.current.delete(key);
		}
		task.deferred.resolve(null);
	}, []);

	const scheduleViewportPlanRef = useRef<() => void>(() => {});
	const schedulePendingPollRef = useRef<() => void>(() => {});

	const buildQueuedCandidate = useCallback(
		(item: FeedItem): TranslationCandidate | null => {
			if (!shouldAutoSmart(item)) {
				return null;
			}
			const key = keyOf(item);
			itemByKeyRef.current.set(key, item);
			const requestItem = buildReleaseSmartRequestItem(item);
			const sourceKey = buildRequestSourceKey(requestItem);
			const existing = requestTasksRef.current.get(key);
			if (existing) {
				if (existing.sourceKey === sourceKey) {
					return null;
				}
				retireTask(key, existing);
			}
			return {
				key,
				item,
				requestItem,
				sourceKey,
				top: 0,
				bottom: 0,
			};
		},
		[retireTask, shouldAutoSmart],
	);

	const settleTaskPromise = useCallback(
		(
			task: TranslationTask,
			mapped: SmartResolveResponse | null,
			error?: unknown,
		) => {
			if (mapped) {
				task.deferred.resolve(mapped);
				return;
			}
			if (task.rejectOnFailure) {
				task.deferred.reject(error);
				return;
			}
			task.deferred.resolve(null);
		},
		[],
	);

	const finalizeSuccess = useCallback(
		(
			candidate: TranslationCandidate,
			task: TranslationTask,
			smart: SmartItem,
		) => {
			clearTask(candidate.key, task);
			const mapped = mapSmartToResolveResponse(smart);
			if (!mapped) {
				failedRef.current.add(candidate.key);
			} else {
				failedRef.current.delete(candidate.key);
				retryCountRef.current.delete(candidate.key);
				onSmart({ kind: candidate.item.kind, id: candidate.item.id }, smart);
			}
			settleTaskPromise(task, mapped);
			scheduleViewportPlanRef.current();
		},
		[clearTask, onSmart, settleTaskPromise],
	);

	const finalizeFailure = useCallback(
		(
			candidate: TranslationCandidate,
			task: TranslationTask,
			error?: unknown,
		) => {
			clearTask(candidate.key, task);
			const retries = retryCountRef.current.get(candidate.key) ?? 0;
			const keepsVisibleReady =
				!task.rejectOnFailure &&
				candidate.item.smart?.status === "ready" &&
				candidate.item.smart.auto_translate === true;
			if (retries + 1 >= REQUEST_ERROR_RECOVERY_MAX_RETRIES) {
				if (keepsVisibleReady) {
					retryCountRef.current.delete(candidate.key);
				} else {
					failedRef.current.add(candidate.key);
					retryCountRef.current.delete(candidate.key);
				}
			} else {
				retryCountRef.current.set(candidate.key, retries + 1);
			}
			settleTaskPromise(task, null, error);
			scheduleViewportPlanRef.current();
		},
		[clearTask, settleTaskPromise],
	);

	const finalizeTerminal = useCallback(
		(
			candidate: TranslationCandidate,
			task: TranslationTask,
			smart: SmartItem | null,
			error?: unknown,
		) => {
			clearTask(candidate.key, task);
			if (smart) {
				onSmart({ kind: candidate.item.kind, id: candidate.item.id }, smart);
			}
			failedRef.current.add(candidate.key);
			retryCountRef.current.delete(candidate.key);
			settleTaskPromise(task, null, error);
			scheduleViewportPlanRef.current();
		},
		[clearTask, onSmart, settleTaskPromise],
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

				const smart = mapTranslationItemToFeedSmart(resolved);
				if (!smart) {
					if (isTerminalTranslationResultStatus(resolved.status)) {
						finalizeTerminal(
							candidate,
							task,
							null,
							new Error(resultLabel(resolved)),
						);
						continue;
					}
					finalizeFailure(candidate, task, new Error(resultLabel(resolved)));
					continue;
				}
				if (isTerminalTranslationResultStatus(resolved.status)) {
					finalizeTerminal(
						candidate,
						task,
						smart,
						new Error(resultLabel(resolved)),
					);
					continue;
				}
				finalizeSuccess(candidate, task, smart);
			}
		},
		[finalizeFailure, finalizeSuccess, finalizeTerminal],
	);

	const hasPollableTasks = useCallback((windowKeys: Set<string>) => {
		for (const [key, task] of requestTasksRef.current) {
			if (task.rejectOnFailure || windowKeys.has(key)) {
				return true;
			}
		}
		return false;
	}, []);

	const resolveCandidateTasks = useCallback(
		async (
			entries: CandidateTask[],
			options?: {
				retryOnError?: boolean;
				onChunkError?: (chunk: CandidateTask[], error: unknown) => void;
			},
		) => {
			for (
				let index = 0;
				index < entries.length;
				index += RESOLVE_RESULTS_MAX_ITEMS
			) {
				const chunk = entries.slice(index, index + RESOLVE_RESULTS_MAX_ITEMS);
				try {
					const retryOnError =
						options?.retryOnError ??
						chunk.some(
							({ candidate }) => candidate.item.smart?.auto_translate === true,
						);
					const response = await apiResolveTranslationResults({
						items: chunk.map(({ candidate }) => candidate.requestItem),
						retry_on_error: retryOnError,
					});
					applyResolvedResults(chunk, response.items);
				} catch (error) {
					options?.onChunkError?.(chunk, error);
				}
			}
		},
		[applyResolvedResults],
	);

	const pollPendingTasks = useCallback(async () => {
		if (!enabled || !mountedRef.current || pollBusyRef.current) return;
		pollBusyRef.current = true;
		let shouldReschedule = false;
		try {
			const viewportHeight =
				window.innerHeight || document.documentElement.clientHeight || 0;
			const windowEntries: WindowEntry[] = [];
			for (const [key, element] of keyToElementRef.current) {
				if (!itemByKeyRef.current.has(key)) continue;
				const rect = element.getBoundingClientRect();
				if (rect.bottom <= 0) continue;
				windowEntries.push({
					key,
					top: rect.top,
					bottom: rect.bottom,
				});
			}
			const windowKeys = buildVisibleWindowKeys(windowEntries, viewportHeight);
			const pending = Array.from(requestTasksRef.current.entries()).map(
				([key, task]) => {
					if (!task.rejectOnFailure && !windowKeys.has(key)) {
						return null;
					}
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
			shouldReschedule = hasPollableTasks(windowKeys);
			if (active.length === 0) return;

			await resolveCandidateTasks(active, {
				onChunkError: (chunk, error) => {
					for (const { candidate, task } of chunk) {
						const count =
							(pollErrorCountRef.current.get(candidate.key) ?? 0) + 1;
						if (count >= REQUEST_ERROR_RECOVERY_MAX_RETRIES) {
							finalizeFailure(candidate, task, error);
						} else {
							pollErrorCountRef.current.set(candidate.key, count);
						}
					}
				},
			});
			shouldReschedule = hasPollableTasks(windowKeys);
		} finally {
			pollBusyRef.current = false;
			if (shouldReschedule) {
				schedulePendingPollRef.current();
			}
		}
	}, [enabled, finalizeFailure, hasPollableTasks, resolveCandidateTasks]);

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
			const byKey = new Map<string, Promise<SmartResolveResponse | null>>();
			const candidates: Array<{
				candidate: TranslationCandidate;
				task: TranslationTask;
			}> = [];

			for (const candidate of rawCandidates) {
				const latestItem =
					itemByKeyRef.current.get(candidate.key) ?? candidate.item;
				const requestItem = buildReleaseSmartRequestItem(latestItem);
				const sourceKey = buildRequestSourceKey(requestItem);
				const existing = requestTasksRef.current.get(candidate.key);
				if (existing && existing.sourceKey === sourceKey) {
					byKey.set(candidate.key, existing.promise);
					continue;
				}
				if (existing) {
					retireTask(candidate.key, existing);
				}

				const deferred = createDeferred<SmartResolveResponse | null>();
				const task: TranslationTask = {
					sourceKey,
					requestItem,
					createdAtMs: Date.now(),
					rejectOnFailure: false,
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

			await resolveCandidateTasks(candidates, {
				onChunkError: (chunk, error) => {
					for (const { candidate, task } of chunk) {
						finalizeFailure(candidate, task, error);
					}
				},
			});

			if (requestTasksRef.current.size > 0) {
				schedulePendingPoll();
			}

			return byKey;
		},
		[
			bumpRender,
			finalizeFailure,
			resolveCandidateTasks,
			schedulePendingPoll,
			retireTask,
		],
	);

	const prime = useCallback(
		async (items: FeedItem[]) => {
			if (!enabled || !mountedRef.current || items.length === 0) {
				return;
			}
			const candidates: TranslationCandidate[] = [];
			for (const item of items) {
				const candidate = buildQueuedCandidate(item);
				if (!candidate) continue;
				candidates.push(candidate);
				if (candidates.length >= INITIAL_SMART_PREFETCH_COUNT) {
					break;
				}
			}
			if (candidates.length === 0) {
				return;
			}
			await submitCandidates(candidates);
		},
		[buildQueuedCandidate, enabled, submitCandidates],
	);

	const prepareCandidates = useCallback(() => {
		const candidates: ViewportPlanEntry[] = [];
		for (const [key, element] of keyToElementRef.current) {
			const item = itemByKeyRef.current.get(key);
			if (!item) continue;
			const rect = element.getBoundingClientRect();
			if (rect.bottom <= 0) continue;
			let candidate: TranslationCandidate | null = null;
			if (shouldAutoSmart(item)) {
				const queued = buildQueuedCandidate(item);
				if (queued) {
					candidate = {
						...queued,
						top: rect.top,
						bottom: rect.bottom,
					};
				}
			}
			candidates.push({
				key,
				top: rect.top,
				bottom: rect.bottom,
				candidate,
			});
		}

		return candidates.sort((left, right) => {
			if (left.top !== right.top) {
				return left.top - right.top;
			}
			return left.bottom - right.bottom;
		});
	}, [buildQueuedCandidate, shouldAutoSmart]);

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
				const plannedEntries = buildVisibleWindowPlan(
					prepareCandidates(),
					viewportHeight,
				);
				const plan = plannedEntries
					.map((entry) => entry.candidate)
					.filter((candidate): candidate is TranslationCandidate =>
						Boolean(candidate),
					);
				if (plan.length > 0) {
					await submitCandidates(plan);
				} else {
					const windowKeys = new Set(plannedEntries.map((entry) => entry.key));
					if (hasPollableTasks(windowKeys)) {
						schedulePendingPollRef.current();
					}
				}
			} while (plannerDirtyRef.current);
		} finally {
			plannerBusyRef.current = false;
		}
	}, [enabled, hasPollableTasks, prepareCandidates, submitCandidates]);

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

	const smartNow = useCallback(
		async (item: FeedItem) => {
			const key = keyOf(item);
			const requestItem = buildReleaseSmartRequestItem(item);
			const sourceKey = buildRequestSourceKey(requestItem);
			failedRef.current.delete(key);
			retryCountRef.current.delete(key);

			const existing = requestTasksRef.current.get(key);
			if (existing && existing.sourceKey === sourceKey) {
				existing.rejectOnFailure = true;
				return existing.promise;
			}
			if (existing) {
				retireTask(key, existing);
			}

			const candidate: TranslationCandidate = {
				key,
				item,
				requestItem,
				sourceKey,
				top: 0,
				bottom: 0,
			};

			const deferred = createDeferred<SmartResolveResponse | null>();
			const task: TranslationTask = {
				sourceKey,
				requestItem,
				createdAtMs: Date.now(),
				rejectOnFailure: true,
				deferred,
				promise: deferred.promise,
			};
			requestTasksRef.current.set(key, task);
			bumpRender();
			try {
				await resolveCandidateTasks([{ candidate, task }], {
					retryOnError: true,
					onChunkError: (chunk, error) => {
						for (const { candidate, task } of chunk) {
							finalizeFailure(candidate, task, error);
						}
					},
				});
				if (requestTasksRef.current.size > 0) {
					schedulePendingPollRef.current();
				}
				return await task.promise;
			} catch (error) {
				if (requestTasksRef.current.get(key) === task) {
					finalizeFailure(candidate, task, error);
				}
				throw error;
			}
		},
		[bumpRender, finalizeFailure, resolveCandidateTasks, retireTask],
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

	return { prime, register, smartNow, inFlightKeys };
}
