import { useCallback, useEffect, useRef, useState } from "react";

import { apiPostJson } from "@/api";
import type { FeedItem, TranslateResponse } from "@/feed/types";

const MAX_CONCURRENT = 2;

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
	const runningRef = useRef(0);

	// Force re-renders when in-flight state changes (refs don't trigger renders).
	const [, forceRender] = useState(0);

	const shouldAutoTranslate = useCallback(
		(item: FeedItem) =>
			enabled &&
			item.translated?.status === "missing" &&
			!failedRef.current.has(keyOf(item)),
		[enabled],
	);

	const translate = useCallback(async (item: FeedItem) => {
		return apiPostJson<TranslateResponse>("/api/translate/release", {
			release_id: item.id,
		});
	}, []);

	const pump = useCallback(() => {
		if (!enabled) return;
		while (
			runningRef.current < MAX_CONCURRENT &&
			queuedRef.current.length > 0
		) {
			const key = queuedRef.current.shift();
			if (!key) break;
			if (inFlightRef.current.has(key)) continue;

			const item = itemByKeyRef.current.get(key);
			if (!item || !shouldAutoTranslate(item)) continue;

			inFlightRef.current.add(key);
			runningRef.current += 1;
			forceRender((x) => x + 1);

			void translate(item)
				.then((res) => {
					onTranslated(item, res);
					if (res.status === "disabled") {
						// Stop auto attempts; the backend says AI is off.
						failedRef.current.add(key);
					}
				})
				.catch(() => {
					// Avoid infinite retries; allow manual retry from the UI.
					failedRef.current.add(key);
				})
				.finally(() => {
					inFlightRef.current.delete(key);
					runningRef.current -= 1;
					forceRender((x) => x + 1);
					pump();
				});
		}
	}, [enabled, onTranslated, shouldAutoTranslate, translate]);

	const enqueue = useCallback(
		(key: string) => {
			if (!enabled) return;
			if (inFlightRef.current.has(key) || failedRef.current.has(key)) return;
			if (queuedRef.current.includes(key)) return;
			queuedRef.current.push(key);
			pump();
		},
		[enabled, pump],
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
				const res = await translate(item);
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
		[onTranslated, translate],
	);

	useEffect(() => {
		if (!enabled) {
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
			observerRef.current?.disconnect();
			observerRef.current = null;
		};
	}, [enabled, enqueue, shouldAutoTranslate]);

	// Re-render is driven by forceRender; derive the current in-flight keys from the ref.
	const inFlightKeys = new Set(inFlightRef.current);

	return { register, translateNow, inFlightKeys };
}
