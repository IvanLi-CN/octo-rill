import { useCallback, useEffect, useRef, useState } from "react";

import {
	apiSubmitTranslationRequest,
	type TranslationRequestItemInput,
} from "@/api";
import type { FeedItem, TranslateResponse } from "@/feed/types";

const MAX_CONCURRENT = 2;
const QUEUE_FLUSH_DELAY_MS = 300;
const WAIT_RECOVERY_MAX_RETRIES = 3;

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
	const waitRetryCountRef = useRef(new Map<string, number>());
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

	const translateSingle = useCallback(async (item: FeedItem) => {
		const response = await apiSubmitTranslationRequest({
			mode: "wait",
			item: buildReleaseSummaryRequestItem(item),
		});
		const translated = response.result;
		const mapped = translated
			? mapTranslationItemToFeedResponse(translated)
			: null;
		if (!translated || !mapped) {
			throw new Error(translated?.error ?? "translate failed");
		}
		return { translated, mapped };
	}, []);

	const requeueAfterFailure = useCallback((item: FeedItem) => {
		const key = keyOf(item);
		if (failedRef.current.has(key)) return;
		const retries = waitRetryCountRef.current.get(key) ?? 0;
		if (retries >= WAIT_RECOVERY_MAX_RETRIES) {
			failedRef.current.add(key);
			waitRetryCountRef.current.delete(key);
			return;
		}
		waitRetryCountRef.current.set(key, retries + 1);
		if (!queuedRef.current.includes(key) && !inFlightRef.current.has(key)) {
			queuedRef.current.push(key);
		}
	}, []);

	const pump = useCallback(() => {
		if (!enabled) return;
		while (
			runningRef.current < MAX_CONCURRENT &&
			queuedRef.current.length > 0
		) {
			const key = queuedRef.current.shift();
			if (!key || inFlightRef.current.has(key)) {
				continue;
			}

			const item = itemByKeyRef.current.get(key);
			if (!item || !shouldAutoTranslate(item)) {
				continue;
			}

			inFlightRef.current.add(key);
			runningRef.current += 1;
			forceRender((x) => x + 1);

			void translateSingle(item)
				.then(({ translated, mapped }) => {
					waitRetryCountRef.current.delete(key);
					onTranslated(item, mapped);
					if (translated.status === "disabled") {
						failedRef.current.add(key);
					}
				})
				.catch(() => {
					requeueAfterFailure(item);
				})
				.finally(() => {
					inFlightRef.current.delete(key);
					runningRef.current -= 1;
					forceRender((x) => x + 1);
					pump();
				});
		}
	}, [
		enabled,
		onTranslated,
		requeueAfterFailure,
		shouldAutoTranslate,
		translateSingle,
	]);

	const schedulePump = useCallback(() => {
		if (!enabled) return;
		if (flushTimerRef.current !== null) return;
		flushTimerRef.current = window.setTimeout(() => {
			flushTimerRef.current = null;
			pump();
		}, QUEUE_FLUSH_DELAY_MS);
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
			waitRetryCountRef.current.delete(key);
			queuedRef.current = queuedRef.current.filter(
				(queuedKey) => queuedKey !== key,
			);
			inFlightRef.current.add(key);
			forceRender((x) => x + 1);

			try {
				const { translated, mapped } = await translateSingle(item);
				onTranslated(item, mapped);
				if (translated.status === "disabled") {
					failedRef.current.add(key);
				}
				return mapped;
			} catch (err) {
				failedRef.current.add(key);
				throw err;
			} finally {
				inFlightRef.current.delete(key);
				forceRender((x) => x + 1);
			}
		},
		[onTranslated, translateSingle],
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
