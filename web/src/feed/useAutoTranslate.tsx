import { useCallback, useEffect, useRef, useState } from "react";

import {
	apiOpenTranslationRequestStream,
	apiSubmitTranslationRequest,
	type TranslationRequestItemInput,
	type TranslationRequestStreamEvent,
} from "@/api";
import type { FeedItem, TranslateResponse } from "@/feed/types";

const MAX_CONCURRENT = 2;
const BATCH_SIZE = 8;
const BATCH_FLUSH_DELAY_MS = 300;
const STREAM_RECOVERY_MAX_RETRIES = 3;

function buildReleaseSummaryRequestItem(
	item: FeedItem,
): TranslationRequestItemInput {
	const title = item.title?.trim() || `release:${item.id}`;
	const excerpt = item.excerpt?.trim();
	const metadata = [item.repo_full_name, item.reason, item.subject_type]
		.filter((value): value is string => Boolean(value?.trim()))
		.join("\n");
	return {
		producer_ref: item.id,
		kind: "release_summary",
		variant: "feed_card",
		entity_id: item.id,
		target_lang: "zh-CN",
		max_wait_ms: 4_000,
		source_blocks: [
			{ slot: "title", text: title },
			...(excerpt ? [{ slot: "excerpt" as const, text: excerpt }] : []),
			...(metadata ? [{ slot: "metadata" as const, text: metadata }] : []),
		],
		target_slots: ["title_zh", "summary_md"],
	};
}

function mapTranslationItemToFeedResponse(item: {
	status: "ready" | "disabled" | "missing" | "error" | "queued";
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

export function useAutoTranslate(params: {
	enabled: boolean;
	onTranslated: (
		item: Pick<FeedItem, "kind" | "id">,
		res: TranslateResponse,
	) => void;
}) {
	const { enabled, onTranslated } = params;

	const observerRef = useRef<IntersectionObserver | null>(null);
	const keyToElementRef = useRef(new Map<string, Element>());
	const elementToKeyRef = useRef(new Map<Element, string>());
	const itemByKeyRef = useRef(new Map<string, FeedItem>());

	const queuedRef = useRef<string[]>([]);
	const inFlightRef = useRef(new Set<string>());
	const failedRef = useRef(new Set<string>());
	const streamRetryCountRef = useRef(new Map<string, number>());
	const runningRef = useRef(0);
	const flushTimerRef = useRef<number | null>(null);

	// Force re-renders when in-flight state changes (refs don't trigger renders).
	const [, forceRender] = useState(0);

	const shouldAutoTranslate = useCallback(
		(item: FeedItem) =>
			enabled &&
			item.translated?.status === "missing" &&
			!failedRef.current.has(keyOf(item)),
		[enabled],
	);

	const translateBatch = useCallback(async (items: FeedItem[]) => {
		return apiSubmitTranslationRequest({
			mode: "wait",
			items: items.map(buildReleaseSummaryRequestItem),
		});
	}, []);

	const translateBatchStream = useCallback(
		async (
			items: FeedItem[],
			onItems: (
				items: Array<{
					id: string;
					status: "ready" | "disabled" | "missing" | "error" | "queued";
					title_zh: string | null;
					summary_md: string | null;
					error: string | null;
				}>,
			) => void,
		): Promise<void> => {
			const res = await apiOpenTranslationRequestStream({
				mode: "stream",
				items: items.map(buildReleaseSummaryRequestItem),
			});

			if (!res.body) {
				throw new Error("translate stream missing response body");
			}

			const reader = res.body.getReader();
			const decoder = new TextDecoder();
			let buffer = "";
			let terminalSeen = false;

			const handleLine = (rawLine: string) => {
				const line = rawLine.trim();
				if (!line) return;
				let evt: TranslationRequestStreamEvent;
				try {
					evt = JSON.parse(line) as TranslationRequestStreamEvent;
				} catch {
					return;
				}
				if (evt.items?.length) {
					onItems(
						evt.items.map((item) => ({
							id: item.entity_id,
							status: item.status,
							title_zh: item.title_zh,
							summary_md: item.summary_md,
							error: item.error,
						})),
					);
				}
				if (evt.event === "completed" || evt.event === "failed") {
					terminalSeen = true;
				}
			};

			while (true) {
				const { done, value } = await reader.read();
				if (done) break;
				buffer += decoder.decode(value, { stream: true });
				let newlineIdx = buffer.indexOf("\n");
				while (newlineIdx >= 0) {
					const line = buffer.slice(0, newlineIdx);
					buffer = buffer.slice(newlineIdx + 1);
					handleLine(line);
					newlineIdx = buffer.indexOf("\n");
				}
			}

			buffer += decoder.decode();
			if (buffer.trim()) {
				handleLine(buffer);
			}

			if (!terminalSeen) {
				throw new Error("translate stream ended before terminal event");
			}
		},
		[],
	);

	const pump = useCallback(() => {
		if (!enabled) return;
		while (
			runningRef.current < MAX_CONCURRENT &&
			queuedRef.current.length > 0
		) {
			const keys: string[] = [];
			while (keys.length < BATCH_SIZE && queuedRef.current.length > 0) {
				const key = queuedRef.current.shift();
				if (!key) break;
				if (inFlightRef.current.has(key)) continue;
				keys.push(key);
			}
			if (keys.length === 0) continue;

			const batchItems = keys
				.map((key) => itemByKeyRef.current.get(key))
				.filter((item): item is FeedItem =>
					Boolean(item && shouldAutoTranslate(item)),
				);
			if (batchItems.length === 0) continue;

			for (const item of batchItems) {
				inFlightRef.current.add(keyOf(item));
			}
			runningRef.current += 1;
			forceRender((x) => x + 1);

			const byId = new Map(batchItems.map((item) => [item.id, item]));
			const handled = new Set<string>();
			const requeueAfterStreamFailure = (item: FeedItem) => {
				const key = keyOf(item);
				if (handled.has(key)) return;
				if (failedRef.current.has(key)) return;
				const retries = streamRetryCountRef.current.get(key) ?? 0;
				if (retries >= STREAM_RECOVERY_MAX_RETRIES) {
					failedRef.current.add(key);
					streamRetryCountRef.current.delete(key);
					return;
				}
				streamRetryCountRef.current.set(key, retries + 1);
				if (!queuedRef.current.includes(key) && !inFlightRef.current.has(key)) {
					queuedRef.current.push(key);
				}
			};
			let streamErrored = false;

			void translateBatchStream(batchItems, (translatedItems) => {
				for (const translated of translatedItems) {
					const item = byId.get(translated.id);
					if (!item) continue;
					const key = keyOf(item);
					if (handled.has(key)) continue;
					handled.add(key);
					streamRetryCountRef.current.delete(key);

					inFlightRef.current.delete(key);
					const mapped = mapTranslationItemToFeedResponse(translated);
					if (mapped) {
						onTranslated(item, mapped);
						if (translated.status === "disabled") {
							failedRef.current.add(key);
						}
					} else {
						failedRef.current.add(key);
					}
				}
				forceRender((x) => x + 1);
			})
				.catch(() => {
					streamErrored = true;
					for (const item of batchItems) {
						requeueAfterStreamFailure(item);
					}
				})
				.finally(() => {
					for (const item of batchItems) {
						const key = keyOf(item);
						if (!handled.has(key) && !streamErrored) {
							requeueAfterStreamFailure(item);
						}
						inFlightRef.current.delete(key);
					}
					runningRef.current -= 1;
					forceRender((x) => x + 1);
					pump();
				});
		}
	}, [enabled, onTranslated, shouldAutoTranslate, translateBatchStream]);

	const schedulePump = useCallback(() => {
		if (!enabled) return;
		if (flushTimerRef.current !== null) return;
		flushTimerRef.current = window.setTimeout(() => {
			flushTimerRef.current = null;
			pump();
		}, BATCH_FLUSH_DELAY_MS);
	}, [enabled, pump]);

	const enqueue = useCallback(
		(key: string) => {
			if (!enabled) return;
			if (inFlightRef.current.has(key) || failedRef.current.has(key)) return;
			if (queuedRef.current.includes(key)) return;
			queuedRef.current.push(key);
			schedulePump();
		},
		[enabled, schedulePump],
	);

	const register = useCallback(
		(item: FeedItem) => (el: HTMLElement | null) => {
			const key = keyOf(item);
			itemByKeyRef.current.set(key, item);

			const prev = keyToElementRef.current.get(key);
			if (prev && observerRef.current) {
				observerRef.current.unobserve(prev);
				elementToKeyRef.current.delete(prev);
				keyToElementRef.current.delete(key);
			}

			if (!el || !enabled) return;
			keyToElementRef.current.set(key, el);
			elementToKeyRef.current.set(el, key);
			observerRef.current?.observe(el);
		},
		[enabled],
	);

	const translateNow = useCallback(
		async (item: FeedItem) => {
			const key = keyOf(item);
			failedRef.current.delete(key);
			inFlightRef.current.add(key);
			forceRender((x) => x + 1);

			try {
				const batch = await translateBatch([item]);
				const translated = batch.items?.[0];
				const res = translated
					? mapTranslationItemToFeedResponse(translated)
					: null;
				if (!translated || !res) {
					throw new Error(translated?.error ?? "translate failed");
				}
				onTranslated(item, res);
				return res;
			} catch (err) {
				failedRef.current.add(key);
				throw err;
			} finally {
				inFlightRef.current.delete(key);
				forceRender((x) => x + 1);
			}
		},
		[onTranslated, translateBatch],
	);

	useEffect(() => {
		if (!enabled) {
			if (flushTimerRef.current !== null) {
				window.clearTimeout(flushTimerRef.current);
				flushTimerRef.current = null;
			}
			observerRef.current?.disconnect();
			observerRef.current = null;
			return;
		}

		observerRef.current = new IntersectionObserver(
			(entries) => {
				for (const e of entries) {
					if (!e.isIntersecting) continue;
					const key = elementToKeyRef.current.get(e.target);
					if (!key) continue;
					const item = itemByKeyRef.current.get(key);
					if (item && shouldAutoTranslate(item)) enqueue(key);
				}
			},
			{ rootMargin: "600px 0px", threshold: 0.01 },
		);

		for (const el of keyToElementRef.current.values()) {
			observerRef.current.observe(el);
		}

		return () => {
			if (flushTimerRef.current !== null) {
				window.clearTimeout(flushTimerRef.current);
				flushTimerRef.current = null;
			}
			observerRef.current?.disconnect();
			observerRef.current = null;
		};
	}, [enabled, enqueue, shouldAutoTranslate]);

	// Re-render is driven by forceRender; derive the current in-flight keys from the ref.
	const inFlightKeys = new Set(inFlightRef.current);

	return { register, translateNow, inFlightKeys };
}
