import { useCallback, useEffect, useRef, useState } from "react";

import { apiPostJson } from "@/api";
import type {
	FeedItem,
	TranslateBatchItem,
	TranslateBatchResponse,
	TranslateBatchStreamEvent,
	TranslateResponse,
} from "@/feed/types";

const MAX_CONCURRENT = 2;
const BATCH_SIZE = 8;
const BATCH_FLUSH_DELAY_MS = 300;
const STREAM_RECOVERY_MAX_RETRIES = 3;

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
		return apiPostJson<TranslateBatchResponse>(
			"/api/translate/releases/batch",
			{
				release_ids: items.map((item) => item.id),
			},
		);
	}, []);

	const translateBatchStream = useCallback(
		async (
			items: FeedItem[],
			onItem: (item: TranslateBatchItem) => void,
		): Promise<void> => {
			const res = await fetch("/api/translate/releases/batch/stream", {
				method: "POST",
				credentials: "include",
				headers: { "content-type": "application/json" },
				body: JSON.stringify({
					release_ids: items.map((item) => item.id),
				}),
			});

			if (!res.ok) {
				let msg = `translate stream failed (${res.status})`;
				try {
					const body = (await res.json()) as {
						error?: { message?: string };
					};
					if (body?.error?.message) msg = body.error.message;
				} catch {
					// Keep fallback message.
				}
				throw new Error(msg);
			}

			if (!res.body) {
				throw new Error("translate stream missing response body");
			}

			const reader = res.body.getReader();
			const decoder = new TextDecoder();
			let buffer = "";
			let doneEventSeen = false;

			const handleLine = (rawLine: string) => {
				const line = rawLine.trim();
				if (!line) return;
				let evt: TranslateBatchStreamEvent;
				try {
					evt = JSON.parse(line) as TranslateBatchStreamEvent;
				} catch {
					// Ignore malformed lines to keep stream resilient.
					return;
				}
				if (evt.event === "item" && evt.item) {
					onItem(evt.item);
				}
				if (evt.event === "error") {
					throw new Error(evt.error ?? "translate stream failed");
				}
				if (evt.event === "done") {
					doneEventSeen = true;
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

			if (!doneEventSeen) {
				throw new Error("translate stream ended before done event");
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

			void translateBatchStream(batchItems, (translated) => {
				const item = byId.get(translated.id);
				if (!item) return;
				const key = keyOf(item);

				if (translated.status === "processing") {
					// Keep the item in-flight; final ready/missing/disabled status will arrive later.
					return;
				}
				if (handled.has(key)) return;
				const terminal =
					translated.status === "ready" ||
					translated.status === "disabled" ||
					translated.status === "missing" ||
					translated.status === "error";
				if (!terminal) return;
				handled.add(key);
				streamRetryCountRef.current.delete(key);

				inFlightRef.current.delete(key);
				if (translated.status === "ready" || translated.status === "disabled") {
					onTranslated(item, {
						lang: translated.lang,
						status: translated.status === "disabled" ? "disabled" : "ready",
						title: translated.title,
						summary: translated.summary,
					});
					if (translated.status === "disabled") {
						failedRef.current.add(key);
					}
				} else {
					failedRef.current.add(key);
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
				const translated = batch.items[0];
				if (
					!translated ||
					(translated.status !== "ready" && translated.status !== "disabled")
				) {
					throw new Error(translated?.error ?? "translate failed");
				}
				const res: TranslateResponse = {
					lang: translated.lang,
					status: translated.status === "disabled" ? "disabled" : "ready",
					title: translated.title,
					summary: translated.summary,
				};
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
