import { useLayoutEffect, useRef, useState } from "react";

const FOOTER_GAP_PX = 16;
const FALLBACK_REPOSITORY_ITEM_HEIGHT_PX = 76;
const COMPACT_EXIT_HYSTERESIS_PX = 64;

type RepositoryPanelLayout = {
	availableHeight: number | null;
	minimumListHeight: number | null;
	shortList: boolean;
	compact: boolean;
};

export function useRepositoryPanelViewportHeight(options: {
	enabled: boolean;
	itemCount: number;
	itemsKey?: string;
}) {
	const panelRef = useRef<HTMLDivElement>(null);
	const listRef = useRef<HTMLUListElement>(null);
	const [layout, setLayout] = useState<RepositoryPanelLayout>({
		availableHeight: null,
		minimumListHeight: null,
		shortList: false,
		compact: false,
	});

	useLayoutEffect(() => {
		if (!options.enabled) {
			setLayout({
				availableHeight: null,
				minimumListHeight: null,
				shortList: false,
				compact: false,
			});
			return;
		}

		const panel = panelRef.current;
		const list = listRef.current;
		if (!panel) return;

		let frame: number | null = null;
		const measure = () => {
			const panelTop = panel.getBoundingClientRect().top;
			const footer = document.querySelector<HTMLElement>(
				'[data-app-meta-footer="true"]',
			);
			const footerTop =
				footer?.getBoundingClientRect().top ??
				window.visualViewport?.height ??
				window.innerHeight;
			const availableHeight = Math.max(0, footerTop - panelTop - FOOTER_GAP_PX);
			const items = list
				? Array.from(
						list.querySelectorAll<HTMLElement>(
							"[data-dashboard-repository-item]",
						),
					)
				: [];
			const firstItem = items[0];
			const itemHeight =
				firstItem?.getBoundingClientRect().height ??
				FALLBACK_REPOSITORY_ITEM_HEIGHT_PX;
			const twoItemHeight = itemHeight * 2;
			const listStyle = list ? getComputedStyle(list) : null;
			const naturalListHeight =
				items.reduce(
					(total, item) => total + item.getBoundingClientRect().height,
					0,
				) +
				Number.parseFloat(listStyle?.paddingTop ?? "0") +
				Number.parseFloat(listStyle?.paddingBottom ?? "0");
			const shortList = naturalListHeight < twoItemHeight;
			const panelChromeHeight = Math.max(
				0,
				panel.getBoundingClientRect().height -
					(list?.getBoundingClientRect().height ?? 0),
			);

			setLayout((current) => {
				const compactThreshold =
					panelChromeHeight +
					twoItemHeight +
					(current.compact ? COMPACT_EXIT_HYSTERESIS_PX : 0);
				const compact = availableHeight < compactThreshold;
				if (
					current.availableHeight === availableHeight &&
					current.minimumListHeight === twoItemHeight &&
					current.shortList === shortList &&
					current.compact === compact
				) {
					return current;
				}
				return {
					availableHeight,
					minimumListHeight: twoItemHeight,
					shortList,
					compact,
				};
			});
		};

		const scheduleMeasure = () => {
			if (frame !== null) return;
			frame = window.requestAnimationFrame(() => {
				frame = null;
				measure();
			});
		};

		scheduleMeasure();
		const observer = new ResizeObserver(measure);
		observer.observe(panel);
		const footer = document.querySelector<HTMLElement>(
			'[data-app-meta-footer="true"]',
		);
		if (footer) observer.observe(footer);
		if (list) {
			observer.observe(list);
			for (const item of list.querySelectorAll<HTMLElement>(
				"[data-dashboard-repository-item]",
			)) {
				observer.observe(item);
			}
		}

		window.addEventListener("resize", scheduleMeasure);
		window.addEventListener("scroll", scheduleMeasure, { passive: true });
		document.addEventListener("scroll", scheduleMeasure, {
			capture: true,
			passive: true,
		});
		window.visualViewport?.addEventListener("resize", scheduleMeasure);
		window.visualViewport?.addEventListener("scroll", scheduleMeasure);
		return () => {
			if (frame !== null) window.cancelAnimationFrame(frame);
			observer.disconnect();
			window.removeEventListener("resize", scheduleMeasure);
			window.removeEventListener("scroll", scheduleMeasure);
			document.removeEventListener("scroll", scheduleMeasure, true);
			window.visualViewport?.removeEventListener("resize", scheduleMeasure);
			window.visualViewport?.removeEventListener("scroll", scheduleMeasure);
		};
	}, [options.enabled, options.itemCount, options.itemsKey]);

	const panelStyle =
		layout.availableHeight !== null
			? {
					height: `${layout.availableHeight}px`,
					maxHeight: `${layout.availableHeight}px`,
				}
			: undefined;
	const listStyle =
		layout.minimumListHeight !== null
			? { minHeight: `${layout.minimumListHeight}px` }
			: undefined;

	return {
		panelRef,
		listRef,
		panelStyle,
		listStyle,
		shortList: layout.shortList,
		compact: layout.compact,
	};
}

export const repositoryPanelViewportConstants = {
	footerGapPx: FOOTER_GAP_PX,
	fallbackItemHeightPx: FALLBACK_REPOSITORY_ITEM_HEIGHT_PX,
	compactExitHysteresisPx: COMPACT_EXIT_HYSTERESIS_PX,
} as const;
