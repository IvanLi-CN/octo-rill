import { useLayoutEffect, useRef, useState } from "react";

const FOOTER_GAP_PX = 16;
const FALLBACK_REPOSITORY_ITEM_HEIGHT_PX = 76;

type RepositoryPanelLayout = {
	availableHeight: number | null;
	shortList: boolean;
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
		shortList: false,
	});

	useLayoutEffect(() => {
		if (!options.enabled) {
			setLayout({ availableHeight: null, shortList: false });
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

			setLayout((current) => {
				if (
					current.availableHeight === availableHeight &&
					current.shortList === shortList
				) {
					return current;
				}
				return { availableHeight, shortList };
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
					maxHeight: `${layout.availableHeight}px`,
					...(layout.shortList
						? { height: `${layout.availableHeight}px` }
						: {}),
				}
			: undefined;

	return {
		panelRef,
		listRef,
		panelStyle,
		shortList: layout.shortList,
	};
}

export const repositoryPanelViewportConstants = {
	footerGapPx: FOOTER_GAP_PX,
	fallbackItemHeightPx: FALLBACK_REPOSITORY_ITEM_HEIGHT_PX,
} as const;
