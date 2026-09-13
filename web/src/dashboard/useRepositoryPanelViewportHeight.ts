import { useLayoutEffect, useRef, useState } from "react";

const FOOTER_GAP_PX = 16;
const FALLBACK_REPOSITORY_ITEM_HEIGHT_PX = 76;
const COMPACT_EXIT_HYSTERESIS_PX = 64;

type RepositoryPanelLayout = {
	availableHeight: number | null;
	minimumListHeight: number | null;
	minimumPanelHeight: number | null;
	shortList: boolean;
	contentCapped: boolean;
	compact: boolean;
};

const EMPTY_LAYOUT: RepositoryPanelLayout = {
	availableHeight: null,
	minimumListHeight: null,
	minimumPanelHeight: null,
	shortList: false,
	contentCapped: false,
	compact: false,
};

export function useRepositoryPanelViewportHeight(options: {
	enabled: boolean;
	itemCount: number;
	itemsKey?: string;
}) {
	const panelRef = useRef<HTMLDivElement>(null);
	const listRef = useRef<HTMLUListElement>(null);
	const [layout, setLayout] = useState<RepositoryPanelLayout>(EMPTY_LAYOUT);

	useLayoutEffect(() => {
		if (!options.enabled) {
			setLayout(EMPTY_LAYOUT);
			return;
		}

		const panel = panelRef.current;
		const list = listRef.current;
		if (!panel) return;

		let frame: number | null = null;
		const measure = () => {
			const panelRect = panel.getBoundingClientRect();
			const panelTop = panelRect.top;
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
			const secondItem = items[1];
			const itemHeight =
				firstItem?.getBoundingClientRect().height ??
				FALLBACK_REPOSITORY_ITEM_HEIGHT_PX;
			const listRect = list?.getBoundingClientRect();
			const minimumListHeight =
				secondItem && listRect
					? Math.max(
							0,
							secondItem.getBoundingClientRect().bottom - listRect.top,
						)
					: itemHeight * 2;
			const listStyle = list ? getComputedStyle(list) : null;
			const naturalListHeight =
				items.reduce(
					(total, item) => total + item.getBoundingClientRect().height,
					0,
				) +
				Number.parseFloat(listStyle?.paddingTop ?? "0") +
				Number.parseFloat(listStyle?.paddingBottom ?? "0");
			const panelComputedStyle = getComputedStyle(panel);
			const panelBottomInset =
				Number.parseFloat(panelComputedStyle.paddingBottom ?? "0") +
				Number.parseFloat(panelComputedStyle.borderBottomWidth ?? "0");
			const panelChromeHeight = listRect
				? Math.max(0, listRect.top - panelRect.top) + panelBottomInset
				: panelRect.height;
			const minimumPanelHeight = Math.ceil(
				panelChromeHeight + minimumListHeight,
			);
			const availableListHeight = Math.max(
				0,
				availableHeight - panelChromeHeight,
			);
			const shortList =
				naturalListHeight < minimumListHeight ||
				availableListHeight <= minimumListHeight + 1;
			const contentCapped =
				panelChromeHeight + naturalListHeight > availableHeight;

			setLayout((current) => {
				const compactThreshold =
					panelChromeHeight +
					minimumListHeight +
					(current.compact ? COMPACT_EXIT_HYSTERESIS_PX : 0);
				const compact = availableHeight < compactThreshold;
				if (
					current.availableHeight === availableHeight &&
					current.minimumListHeight === minimumListHeight &&
					current.minimumPanelHeight === minimumPanelHeight &&
					current.shortList === shortList &&
					current.contentCapped === contentCapped &&
					current.compact === compact
				) {
					return current;
				}
				return {
					availableHeight,
					minimumListHeight,
					minimumPanelHeight,
					shortList,
					contentCapped,
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
		const observer = new ResizeObserver(scheduleMeasure);
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
		window.visualViewport?.addEventListener("resize", scheduleMeasure);
		return () => {
			if (frame !== null) window.cancelAnimationFrame(frame);
			observer.disconnect();
			window.removeEventListener("resize", scheduleMeasure);
			window.visualViewport?.removeEventListener("resize", scheduleMeasure);
		};
	}, [options.enabled, options.itemCount, options.itemsKey]);

	const panelStyle =
		layout.availableHeight !== null && layout.minimumPanelHeight !== null
			? {
					minHeight: `${layout.minimumPanelHeight}px`,
					...(layout.availableHeight >= layout.minimumPanelHeight
						? {
								maxHeight: `${layout.availableHeight}px`,
								...(layout.shortList || layout.contentCapped
									? { height: `${layout.availableHeight}px` }
									: {}),
							}
						: { height: `${layout.minimumPanelHeight}px` }),
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
		contentCapped: layout.contentCapped,
		compact: layout.compact,
	};
}

export const repositoryPanelViewportConstants = {
	footerGapPx: FOOTER_GAP_PX,
	fallbackItemHeightPx: FALLBACK_REPOSITORY_ITEM_HEIGHT_PX,
	compactExitHysteresisPx: COMPACT_EXIT_HYSTERESIS_PX,
} as const;
