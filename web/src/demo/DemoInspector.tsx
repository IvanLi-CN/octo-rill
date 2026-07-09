import {
	ChevronDown,
	Copy,
	GripHorizontal,
	Inspect,
	Minimize2,
	RefreshCcw,
} from "lucide-react";
import {
	type ReactNode,
	type RefObject,
	useEffect,
	useId,
	useLayoutEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { useRouterState } from "@tanstack/react-router";

import { DEMO_INSPECTOR_PANEL_WIDTH_PX } from "@/demo/layout";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
	Sheet,
	SheetContent,
	SheetDescription,
	SheetHeader,
	SheetTitle,
} from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import { useAppShellChrome } from "@/layout/AppShell";
import { useOptionalRouter } from "@/lib/internalNavigation";
import { useMediaQuery } from "@/lib/useMediaQuery";
import { cn } from "@/lib/utils";
import { buildDefaultDemoShareState, DEMO_SCENES } from "@/demo/registry";
import {
	applyDemoShareStateInPlace,
	buildCurrentDemoHref,
	buildCurrentDemoShareHref,
	patchDemoShareState,
	resolveCurrentDemoScene,
	setPendingDemoRouteSyncHref,
	syncDemoRuntimeWithHref,
	updateDemoPanelLayout,
	useDemoSnapshot,
} from "@/demo/runtime";

function clamp(value: number, min: number, max: number) {
	return Math.min(max, Math.max(min, value));
}

const DESKTOP_PANEL_WIDTH = DEMO_INSPECTOR_PANEL_WIDTH_PX;
const DESKTOP_PANEL_GAP = 16;
const DESKTOP_PANEL_MIN_HEIGHT = 360;
const DESKTOP_PANEL_FALLBACK_HEIGHT = 640;
const DESKTOP_PANEL_COLLAPSED_HEIGHT = 52;
const DESKTOP_PANEL_TOAST_GAP = 12;
const INSPECTOR_SCROLL_CUE_THRESHOLD = 12;
const WIDE_DOCKED_BUBBLE_X = 16;
const WIDE_DOCKED_BUBBLE_Y = 88;
const PERSONA_OPTIONS = [
	{ value: "guest", label: "Guest" },
	{ value: "member", label: "Member" },
	{ value: "admin", label: "Admin" },
] as const;
const NETWORK_OPTIONS = [
	{ value: "normal", label: "Normal" },
	{ value: "slow", label: "Slow" },
	{ value: "faulty", label: "Faulty" },
] as const;
const PUBLICATION_OPTIONS = [
	{ value: "unpublished", label: "Unpublished" },
	{ value: "published", label: "Published" },
] as const;

function InspectorSelect<Value extends string>(props: {
	id: string;
	value: Value;
	options: ReadonlyArray<{
		value: Value;
		label: string;
	}>;
	compact?: boolean;
	onValueChange: (value: Value) => void;
}) {
	const { compact = false } = props;

	return (
		<div className="relative">
			<select
				id={props.id}
				value={props.value}
				className={cn(
					"h-9 w-full appearance-none rounded-md border border-input bg-transparent px-3 pr-9 text-sm shadow-xs transition-[color,box-shadow] outline-none focus:border-ring focus:ring-[3px] focus:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-input/30 dark:hover:bg-input/50",
					compact && "h-7.5 text-xs",
				)}
				onChange={(event) => props.onValueChange(event.target.value as Value)}
				onPointerDown={(event) => event.stopPropagation()}
				onClick={(event) => event.stopPropagation()}
			>
				{props.options.map((option) => (
					<option key={option.value} value={option.value}>
						{option.label}
					</option>
				))}
			</select>
			<ChevronDown className="pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2 text-muted-foreground" />
		</div>
	);
}

function getOpenToastBottomInViewport(bounds: { left: number; right: number }) {
	if (typeof document === "undefined") return 0;
	const viewport = document.querySelector<HTMLElement>(
		'[data-slot="toast-viewport"]',
	);
	if (!viewport) return 0;
	const viewportRect = viewport.getBoundingClientRect();
	const overlapsHorizontally =
		bounds.right > viewportRect.left - DESKTOP_PANEL_TOAST_GAP &&
		bounds.left < viewportRect.right + DESKTOP_PANEL_TOAST_GAP;
	if (!overlapsHorizontally) return 0;

	const openToasts = Array.from(
		viewport.querySelectorAll<HTMLElement>(
			'[data-slot="toast"][data-state="open"]',
		),
	).filter((toast) => {
		const rect = toast.getBoundingClientRect();
		return rect.width > 0 && rect.height > 0;
	});
	if (openToasts.length === 0) return 0;
	return Math.max(
		...openToasts.map((toast) => toast.getBoundingClientRect().bottom),
	);
}

function resolveDesktopPanelMetrics(input: {
	collapsed: boolean;
	edge: "left" | "right";
	x: number;
	y: number;
	headerHeight: number;
	viewportTopInset: number;
	viewportBottomInset: number;
	frameWidth: number;
}) {
	if (typeof window === "undefined") {
		return {
			x: input.x,
			y: input.y,
			maxHeight: undefined as number | undefined,
		};
	}

	const nextX = clamp(
		input.x,
		DESKTOP_PANEL_GAP,
		Math.max(
			DESKTOP_PANEL_GAP,
			window.innerWidth - input.frameWidth - DESKTOP_PANEL_GAP,
		),
	);
	const frameLeft =
		input.edge === "left"
			? nextX
			: window.innerWidth - input.frameWidth - nextX;
	const frameRight = frameLeft + input.frameWidth;
	const chromeTop =
		input.viewportTopInset + input.headerHeight + DESKTOP_PANEL_GAP;
	const toastBottom = getOpenToastBottomInViewport({
		left: frameLeft,
		right: frameRight,
	});
	const minY = Math.max(
		DESKTOP_PANEL_GAP,
		chromeTop,
		toastBottom > 0 ? Math.ceil(toastBottom + DESKTOP_PANEL_TOAST_GAP) : 0,
	);
	const minVisibleHeight = input.collapsed
		? DESKTOP_PANEL_COLLAPSED_HEIGHT
		: DESKTOP_PANEL_MIN_HEIGHT;
	const maxY = Math.max(
		minY,
		window.innerHeight -
			input.viewportBottomInset -
			DESKTOP_PANEL_GAP -
			minVisibleHeight,
	);
	const nextY = clamp(input.y, minY, maxY);
	const availableHeight = Math.max(
		0,
		window.innerHeight - input.viewportBottomInset - DESKTOP_PANEL_GAP - nextY,
	);

	return {
		x: nextX,
		y: nextY,
		maxHeight: availableHeight,
	};
}

function useViewportRevision() {
	const [viewportRevision, setViewportRevision] = useState(0);

	useEffect(() => {
		if (typeof window === "undefined") return;
		const syncViewport = () => {
			setViewportRevision((current) => current + 1);
		};
		const visualViewport = window.visualViewport;
		window.addEventListener("resize", syncViewport);
		visualViewport?.addEventListener("resize", syncViewport);
		visualViewport?.addEventListener("scroll", syncViewport);
		return () => {
			window.removeEventListener("resize", syncViewport);
			visualViewport?.removeEventListener("resize", syncViewport);
			visualViewport?.removeEventListener("scroll", syncViewport);
		};
	}, []);

	useEffect(() => {
		if (typeof document === "undefined") return;
		const observer = new MutationObserver(() => {
			setViewportRevision((current) => current + 1);
		});
		observer.observe(document.body, {
			subtree: true,
			childList: true,
			attributes: true,
			attributeFilter: ["data-state"],
		});
		return () => observer.disconnect();
	}, []);

	return viewportRevision;
}

function useInspectorScrollCues(
	scrollerRef: RefObject<HTMLDivElement | null>,
	input: {
		disabled: boolean;
		viewportRevision: number;
	},
) {
	const { disabled, viewportRevision } = input;
	const [scrollCues, setScrollCues] = useState({
		showTop: false,
		showBottom: false,
	});

	useEffect(() => {
		if (disabled) {
			setScrollCues({ showTop: false, showBottom: false });
			return;
		}
		const scroller = scrollerRef.current;
		if (!scroller) return;

		const syncScrollCues = () => {
			const remainingScroll = scroller.scrollHeight - scroller.clientHeight;
			const nextShowTop =
				remainingScroll > INSPECTOR_SCROLL_CUE_THRESHOLD &&
				scroller.scrollTop > INSPECTOR_SCROLL_CUE_THRESHOLD;
			const nextShowBottom =
				remainingScroll > INSPECTOR_SCROLL_CUE_THRESHOLD &&
				scroller.scrollTop < remainingScroll - INSPECTOR_SCROLL_CUE_THRESHOLD;
			setScrollCues((current) =>
				current.showTop === nextShowTop && current.showBottom === nextShowBottom
					? current
					: {
							showTop: nextShowTop,
							showBottom: nextShowBottom,
						},
			);
		};

		syncScrollCues();
		scroller.addEventListener("scroll", syncScrollCues, { passive: true });
		const resizeObserver =
			typeof ResizeObserver === "undefined"
				? null
				: new ResizeObserver(syncScrollCues);
		resizeObserver?.observe(scroller);
		const content = scroller.firstElementChild;
		if (content) {
			resizeObserver?.observe(content);
		}
		const mutationObserver = new MutationObserver(syncScrollCues);
		mutationObserver.observe(scroller, {
			subtree: true,
			childList: true,
			characterData: true,
			attributes: true,
		});
		window.addEventListener("resize", syncScrollCues);
		const visualViewport = window.visualViewport;
		visualViewport?.addEventListener("resize", syncScrollCues);
		visualViewport?.addEventListener("scroll", syncScrollCues);
		return () => {
			scroller.removeEventListener("scroll", syncScrollCues);
			resizeObserver?.disconnect();
			mutationObserver.disconnect();
			window.removeEventListener("resize", syncScrollCues);
			visualViewport?.removeEventListener("resize", syncScrollCues);
			visualViewport?.removeEventListener("scroll", syncScrollCues);
		};
	}, [disabled, scrollerRef, viewportRevision]);

	return scrollCues;
}

export function DemoInspector(props: {
	desktopMode?: "floating" | "docked-sidebar";
}) {
	const snapshot = useDemoSnapshot();
	const router = useOptionalRouter();
	const scene = resolveCurrentDemoScene();
	const isMobile = useMediaQuery("(max-width: 767px)");
	const [mobileOpen, setMobileOpen] = useState(false);

	const routeLocationKey = useRouterState({
		select: (state) =>
			JSON.stringify({
				pathname: state.location.pathname,
				search: state.location.search,
				hash: state.location.hash,
			}),
	});

	useEffect(() => {
		if (!snapshot.active) return;
		syncDemoRuntimeWithHref(
			`${window.location.pathname}${window.location.search}${window.location.hash}`,
		);
	}, [routeLocationKey, snapshot.active]);

	const shareHref = useMemo(
		() => buildCurrentDemoShareHref(),
		[routeLocationKey, snapshot.shareState],
	);

	if (!snapshot.active) {
		return null;
	}

	const copyShareLink = async () => {
		const absoluteHref = new URL(shareHref, window.location.origin).toString();
		try {
			await navigator.clipboard.writeText(absoluteHref);
		} catch {
			// ignore clipboard failures in demo-only tooling
		}
	};

	const replaceDemoLocation = (href: string) => {
		window.location.replace(new URL(href, window.location.origin).toString());
	};

	const navigateDemoHref = async (
		href: string,
		next: Partial<typeof snapshot.shareState>,
		options?: {
			reseed?: boolean;
		},
	) => {
		if (!router) {
			replaceDemoLocation(href);
			return;
		}

		const previousShareState = snapshot.shareState;
		setPendingDemoRouteSyncHref(href);
		patchDemoShareState(next, { reseed: options?.reseed });

		try {
			await router.navigate({
				href,
				replace: true,
			});
		} catch (error) {
			setPendingDemoRouteSyncHref(null);
			patchDemoShareState(previousShareState, { reseed: options?.reseed });
			throw error;
		}
	};

	const navigateWithShareState = (
		next: Partial<typeof snapshot.shareState>,
		options?: {
			reseed?: boolean;
		},
	) => {
		const nextSceneId = next.sceneId ?? snapshot.shareState.sceneId;

		if (
			options?.reseed !== false &&
			nextSceneId === snapshot.shareState.sceneId
		) {
			applyDemoShareStateInPlace(next, { reseed: true });
			return;
		}

		if (options?.reseed !== false) {
			void navigateDemoHref(buildCurrentDemoHref(next), next, {
				reseed: true,
			});
		} else {
			applyDemoShareStateInPlace(next, { reseed: false });
		}
	};

	const setCollapsed = (collapsed: boolean) => {
		updateDemoPanelLayout({ collapsed });
	};

	const collapseToBubble = () => {
		updateDemoPanelLayout({
			collapsed: true,
			edge: "left",
			x: WIDE_DOCKED_BUBBLE_X,
			y: WIDE_DOCKED_BUBBLE_Y,
		});
	};

	const resetToSceneDefaults = () => {
		const nextShareState = buildDefaultDemoShareState(
			snapshot.shareState.sceneId,
		);
		void navigateDemoHref(
			buildCurrentDemoHref(nextShareState),
			nextShareState,
			{
				reseed: true,
			},
		);
	};

	const panelProps = {
		snapshot,
		sceneTitle: scene.title,
		shareHref,
		onSceneChange: (sceneId: (typeof DEMO_SCENES)[number]["id"]) =>
			navigateWithShareState({ sceneId }, { reseed: true }),
		onPersonaChange: (personaId: "guest" | "member" | "admin") =>
			navigateWithShareState({ personaId }, { reseed: true }),
		onNetworkChange: (networkMode: "normal" | "slow" | "faulty") =>
			navigateWithShareState({ networkMode }, { reseed: false }),
		onIncludeOwnReleasesChange: (includeOwnReleases: boolean) =>
			navigateWithShareState({ includeOwnReleases }, { reseed: true }),
		onPublicationStateChange: (publicationState: "published" | "unpublished") =>
			navigateWithShareState({ publicationState }, { reseed: true }),
		onReset: resetToSceneDefaults,
		onCopyShareLink: copyShareLink,
	} satisfies DemoInspectorPanelProps;

	if (isMobile) {
		return (
			<>
				<Button
					type="button"
					size="sm"
					className="fixed bottom-4 right-4 z-50 rounded-full shadow-lg"
					onClick={() => setMobileOpen(true)}
				>
					<Inspect className="size-4" />
					Demo
				</Button>
				<Sheet open={mobileOpen} onOpenChange={setMobileOpen}>
					<SheetContent side="bottom" className="h-[82vh] rounded-t-3xl p-0">
						<SheetHeader className="border-b pb-3">
							<SheetTitle>Demo Inspector</SheetTitle>
							<SheetDescription>
								切换 scene / persona / network，并复制当前 share deep link。
							</SheetDescription>
						</SheetHeader>
						<div className="h-full overflow-y-auto p-4">
							<DemoInspectorPanel {...panelProps} />
						</div>
					</SheetContent>
				</Sheet>
			</>
		);
	}

	if (props.desktopMode === "docked-sidebar") {
		return (
			<DemoInspectorDockedRail onCollapse={collapseToBubble}>
				{({ density }) => (
					<DemoInspectorPanel {...panelProps} density={density} />
				)}
			</DemoInspectorDockedRail>
		);
	}

	return (
		<DesktopInspectorChrome
			collapsed={snapshot.panelLayout.collapsed}
			edge={snapshot.panelLayout.edge}
			x={snapshot.panelLayout.x}
			y={snapshot.panelLayout.y}
			onCollapse={() => setCollapsed(true)}
			onExpand={() => setCollapsed(false)}
		>
			{({ density }) => (
				<DemoInspectorPanel {...panelProps} density={density} />
			)}
		</DesktopInspectorChrome>
	);
}

export function DemoInspectorDockedRail(props: {
	onCollapse: () => void;
	children: (input: { density: "default" | "compact" }) => ReactNode;
}) {
	const scrollerRef = useRef<HTMLDivElement | null>(null);
	const viewportRevision = useViewportRevision();
	const scrollCues = useInspectorScrollCues(scrollerRef, {
		disabled: false,
		viewportRevision,
	});
	const density =
		typeof window !== "undefined" && window.innerHeight <= 900
			? "compact"
			: "default";

	return (
		<div
			className="fixed inset-y-0 left-0 z-40 flex h-dvh min-h-0 w-[380px] flex-col overflow-hidden border-r bg-background/95 shadow-[16px_0_44px_-32px_rgba(15,23,42,0.48)] backdrop-blur"
			data-demo-inspector-chrome="desktop"
			data-demo-inspector-layout="sidebar"
			data-demo-inspector-mode="docked"
			data-demo-inspector-pinned="true"
		>
			<div
				className={cn(
					"flex items-center justify-between gap-3 border-b px-4 py-4",
					density === "compact" && "px-3 py-3",
				)}
				data-demo-inspector-title="true"
			>
				<div className="flex min-w-0 items-center gap-3">
					<div className="flex size-9 shrink-0 items-center justify-center rounded-2xl border bg-muted/35">
						<Inspect className="size-4 text-muted-foreground" />
					</div>
					<div className="min-w-0">
						<p className="font-medium text-sm">Demo Inspector</p>
						<p className="text-muted-foreground text-xs">
							Pinned on wide desktop
						</p>
					</div>
				</div>
				<Button
					type="button"
					size="icon"
					variant="ghost"
					className={cn("shrink-0", density === "compact" && "size-8")}
					aria-label="Collapse Demo Inspector"
					data-demo-inspector-collapse="true"
					onClick={props.onCollapse}
				>
					<Minimize2 className="size-4" />
				</Button>
			</div>
			<div className="relative min-h-0 flex-1">
				{scrollCues.showTop ? (
					<div
						className="pointer-events-none absolute inset-x-0 top-0 z-10 h-5 bg-linear-to-b from-background via-background/85 to-transparent"
						data-demo-inspector-scroll-cue="top"
					/>
				) : null}
				<div
					ref={scrollerRef}
					className={cn(
						"h-full min-h-0 overflow-y-auto p-4 pb-6",
						density === "compact" && "p-2.5 pb-3",
					)}
					data-demo-inspector-scroller="true"
				>
					{props.children({ density })}
				</div>
				{scrollCues.showBottom ? (
					<>
						<div
							className="pointer-events-none absolute inset-x-0 bottom-0 z-10 h-12 bg-linear-to-t from-background via-background/86 to-transparent"
							data-demo-inspector-scroll-cue="bottom"
						/>
						<div className="pointer-events-none absolute inset-x-0 bottom-3 z-10 flex justify-center">
							<div className="rounded-full border bg-background/92 px-2.5 py-1 font-medium text-[10px] text-muted-foreground shadow-sm backdrop-blur">
								向下滚动查看更多
							</div>
						</div>
					</>
				) : null}
			</div>
		</div>
	);
}

function DesktopInspectorChrome(props: {
	children: (input: { density: "default" | "compact" }) => ReactNode;
	collapsed: boolean;
	edge: "left" | "right";
	x: number;
	y: number;
	onCollapse: () => void;
	onExpand: () => void;
}) {
	const { children, collapsed, edge, x, y, onCollapse, onExpand } = props;
	const { headerHeight, viewportBottomInset, viewportTopInset } =
		useAppShellChrome();
	const panelRef = useRef<HTMLDivElement | null>(null);
	const titleRef = useRef<HTMLDivElement | null>(null);
	const scrollerRef = useRef<HTMLDivElement | null>(null);
	const dragOffsetRef = useRef<{ x: number; y: number } | null>(null);
	const [dragging, setDragging] = useState(false);
	const [contentHeight, setContentHeight] = useState<number | null>(null);
	const viewportRevision = useViewportRevision();
	const scrollCues = useInspectorScrollCues(scrollerRef, {
		disabled: collapsed,
		viewportRevision,
	});

	useLayoutEffect(() => {
		if (collapsed || typeof window === "undefined") {
			setContentHeight(null);
			return;
		}

		const syncContentHeight = () => {
			const titleHeight = titleRef.current?.offsetHeight ?? 0;
			const scrollerHeight = scrollerRef.current?.scrollHeight ?? 0;
			if (scrollerHeight === 0) return;
			const nextHeight = titleHeight + scrollerHeight;
			setContentHeight((current) =>
				current !== null && Math.abs(current - nextHeight) < 1
					? current
					: nextHeight,
			);
		};

		syncContentHeight();
		const resizeObserver =
			typeof ResizeObserver === "undefined"
				? null
				: new ResizeObserver(syncContentHeight);
		if (titleRef.current) {
			resizeObserver?.observe(titleRef.current);
		}
		if (scrollerRef.current) {
			resizeObserver?.observe(scrollerRef.current);
			if (scrollerRef.current.firstElementChild instanceof HTMLElement) {
				resizeObserver?.observe(scrollerRef.current.firstElementChild);
			}
		}
		const mutationObserver = new MutationObserver(syncContentHeight);
		if (scrollerRef.current) {
			mutationObserver.observe(scrollerRef.current, {
				subtree: true,
				childList: true,
				characterData: true,
				attributes: true,
			});
		}
		window.addEventListener("resize", syncContentHeight);
		const visualViewport = window.visualViewport;
		visualViewport?.addEventListener("resize", syncContentHeight);
		visualViewport?.addEventListener("scroll", syncContentHeight);
		return () => {
			resizeObserver?.disconnect();
			mutationObserver.disconnect();
			window.removeEventListener("resize", syncContentHeight);
			visualViewport?.removeEventListener("resize", syncContentHeight);
			visualViewport?.removeEventListener("scroll", syncContentHeight);
		};
	}, [collapsed, viewportRevision]);

	const frameWidth =
		panelRef.current?.offsetWidth ?? (collapsed ? 176 : DESKTOP_PANEL_WIDTH);
	const panelMetrics = resolveDesktopPanelMetrics({
		collapsed,
		edge,
		x,
		y,
		headerHeight,
		viewportTopInset,
		viewportBottomInset,
		frameWidth,
	});
	const desiredPanelHeight = Math.max(
		DESKTOP_PANEL_MIN_HEIGHT,
		contentHeight ?? DESKTOP_PANEL_FALLBACK_HEIGHT,
	);
	const density =
		!collapsed && (panelMetrics.maxHeight ?? Number.POSITIVE_INFINITY) <= 590
			? "compact"
			: "default";
	const panelHeight = collapsed
		? undefined
		: Math.min(
				panelMetrics.maxHeight ?? desiredPanelHeight,
				desiredPanelHeight,
			);

	useEffect(() => {
		if (!dragging) return;
		const initialOffset = dragOffsetRef.current;
		if (!initialOffset) return;

		let currentY = panelMetrics.y;

		const onMove = (event: PointerEvent) => {
			const panelWidth = panelRef.current?.offsetWidth ?? DESKTOP_PANEL_WIDTH;
			const currentX = clamp(
				event.clientX - initialOffset.x,
				DESKTOP_PANEL_GAP,
				Math.max(
					DESKTOP_PANEL_GAP,
					window.innerWidth - panelWidth - DESKTOP_PANEL_GAP,
				),
			);
			const nextMetrics = resolveDesktopPanelMetrics({
				collapsed: false,
				edge: "left",
				x: currentX,
				y: event.clientY - initialOffset.y,
				headerHeight,
				viewportTopInset,
				viewportBottomInset,
				frameWidth: panelWidth,
			});
			currentY = nextMetrics.y;
			updateDemoPanelLayout(
				{ edge: "left", x: currentX, y: currentY },
				{ emit: true },
			);
		};

		const onUp = (event: PointerEvent) => {
			const snapLeft = event.clientX < window.innerWidth / 2;
			const nextMetrics = resolveDesktopPanelMetrics({
				collapsed: false,
				edge: snapLeft ? "left" : "right",
				x: DESKTOP_PANEL_GAP,
				y: currentY,
				headerHeight,
				viewportTopInset,
				viewportBottomInset,
				frameWidth: panelRef.current?.offsetWidth ?? DESKTOP_PANEL_WIDTH,
			});
			updateDemoPanelLayout({
				edge: snapLeft ? "left" : "right",
				x: DESKTOP_PANEL_GAP,
				y: nextMetrics.y,
			});
			dragOffsetRef.current = null;
			setDragging(false);
			window.removeEventListener("pointermove", onMove);
			window.removeEventListener("pointerup", onUp);
		};

		window.addEventListener("pointermove", onMove);
		window.addEventListener("pointerup", onUp);
		return () => {
			window.removeEventListener("pointermove", onMove);
			window.removeEventListener("pointerup", onUp);
		};
	}, [
		dragging,
		headerHeight,
		panelMetrics.y,
		viewportBottomInset,
		viewportRevision,
		viewportTopInset,
	]);

	const positionStyle =
		edge === "left"
			? { left: panelMetrics.x, top: panelMetrics.y }
			: { right: panelMetrics.x, top: panelMetrics.y };

	if (collapsed) {
		return (
			<Button
				type="button"
				size="sm"
				className="fixed z-50 max-w-[calc(100vw-24px)] rounded-full px-4 py-5 shadow-lg"
				data-demo-inspector-bubble="desktop"
				data-demo-inspector-mode="floating"
				style={positionStyle}
				onClick={onExpand}
			>
				<Inspect className="size-4" />
				Demo Inspector
			</Button>
		);
	}

	return (
		<div
			ref={panelRef}
			className="fixed z-50 flex w-[380px] max-w-[calc(100vw-24px)] overflow-hidden"
			data-demo-inspector-chrome="desktop"
			data-demo-inspector-mode="floating"
			style={{
				...positionStyle,
				height: panelHeight,
				maxHeight: panelMetrics.maxHeight,
			}}
		>
			<div
				className="flex h-full min-h-0 w-full flex-1 flex-col overflow-hidden rounded-3xl border bg-background/95 shadow-2xl backdrop-blur"
				data-demo-inspector-surface="true"
			>
				<div
					ref={titleRef}
					className={cn(
						"flex items-center justify-between border-b px-4 py-3",
						"cursor-move",
						density === "compact" && "px-3 py-2",
					)}
					data-demo-inspector-title="true"
					onPointerDown={(event) => {
						if (
							event.target instanceof HTMLElement &&
							event.target.closest("button")
						) {
							return;
						}
						const rect = panelRef.current?.getBoundingClientRect();
						if (!rect) return;
						dragOffsetRef.current = {
							x: event.clientX - rect.left,
							y: event.clientY - rect.top,
						};
						setDragging(true);
					}}
				>
					<div className="flex items-center gap-2">
						<GripHorizontal className="size-4 text-muted-foreground" />
						<p className="font-medium text-sm">Demo Inspector</p>
					</div>
					<Button
						type="button"
						size="icon"
						variant="ghost"
						className={cn(density === "compact" && "size-8")}
						aria-label="Collapse Demo Inspector"
						data-demo-inspector-collapse="true"
						onPointerDown={(event) => event.stopPropagation()}
						onClick={onCollapse}
					>
						<Minimize2 className="size-4" />
					</Button>
				</div>
				<div className="relative min-h-0 flex-1">
					{scrollCues.showTop ? (
						<div
							className="pointer-events-none absolute inset-x-0 top-0 z-10 h-5 bg-linear-to-b from-background via-background/85 to-transparent"
							data-demo-inspector-scroll-cue="top"
						/>
					) : null}
					<div
						ref={scrollerRef}
						className={cn(
							"h-full min-h-0 overflow-y-auto p-4 pb-5",
							density === "compact" && "p-2.5 pb-2.5",
						)}
						data-demo-inspector-scroller="true"
					>
						{children({ density })}
					</div>
					{scrollCues.showBottom ? (
						<>
							<div
								className="pointer-events-none absolute inset-x-0 bottom-0 z-10 h-12 bg-linear-to-t from-background via-background/86 to-transparent"
								data-demo-inspector-scroll-cue="bottom"
							/>
							<div className="pointer-events-none absolute inset-x-0 bottom-3 z-10 flex justify-center">
								<div className="rounded-full border bg-background/92 px-2.5 py-1 font-medium text-[10px] text-muted-foreground shadow-sm backdrop-blur">
									向下滚动查看更多
								</div>
							</div>
						</>
					) : null}
				</div>
			</div>
		</div>
	);
}

export type DemoInspectorPanelProps = {
	snapshot: ReturnType<typeof useDemoSnapshot>;
	sceneTitle: string;
	shareHref: string;
	density?: "default" | "compact";
	onSceneChange: (sceneId: (typeof DEMO_SCENES)[number]["id"]) => void;
	onPersonaChange: (personaId: "guest" | "member" | "admin") => void;
	onNetworkChange: (networkMode: "normal" | "slow" | "faulty") => void;
	onIncludeOwnReleasesChange: (checked: boolean) => void;
	onPublicationStateChange: (state: "published" | "unpublished") => void;
	onReset: () => void;
	onCopyShareLink: () => void;
};

export function DemoInspectorPanel(props: DemoInspectorPanelProps) {
	const { snapshot } = props;
	const isCompact = props.density === "compact";
	const sceneSelectId = useId();
	const personaSelectId = useId();
	const networkSelectId = useId();
	const includeOwnReleasesSwitchId = useId();
	const publicationStateSelectId = useId();
	const shareInputId = useId();

	return (
		<div className={cn("space-y-1.5", isCompact && "space-y-0.5")}>
			<Card className="border-dashed">
				<CardHeader className={cn("p-3 pb-2", isCompact && "p-2 pb-1")}>
					<CardTitle
						className={cn(
							"flex items-center justify-between text-base",
							isCompact && "text-sm",
						)}
					>
						<span>{props.sceneTitle}</span>
						<Badge variant="outline">Simulated Writes</Badge>
					</CardTitle>
				</CardHeader>
				<CardContent
					className={cn(
						"space-y-2.5 px-3 pb-3",
						isCompact && "space-y-1.5 px-2 pb-2",
					)}
				>
					<section className={cn("space-y-1.5", isCompact && "space-y-0.5")}>
						<Label htmlFor={sceneSelectId}>Scene</Label>
						<InspectorSelect
							id={sceneSelectId}
							value={snapshot.shareState.sceneId}
							onValueChange={(value) =>
								props.onSceneChange(value as (typeof DEMO_SCENES)[number]["id"])
							}
							options={DEMO_SCENES.map((demoScene) => ({
								value: demoScene.id,
								label: demoScene.title,
							}))}
							compact={isCompact}
						/>
					</section>

					<section
						className={cn("grid gap-2.5 sm:grid-cols-2", isCompact && "gap-2")}
					>
						<div className={cn("space-y-1.5", isCompact && "space-y-0.5")}>
							<Label htmlFor={personaSelectId}>Persona</Label>
							<InspectorSelect
								id={personaSelectId}
								value={snapshot.shareState.personaId}
								onValueChange={(value) =>
									props.onPersonaChange(value as "guest" | "member" | "admin")
								}
								options={PERSONA_OPTIONS}
								compact={isCompact}
							/>
						</div>
						<div className={cn("space-y-1.5", isCompact && "space-y-0.5")}>
							<Label htmlFor={networkSelectId}>Network</Label>
							<InspectorSelect
								id={networkSelectId}
								value={snapshot.shareState.networkMode}
								onValueChange={(value) =>
									props.onNetworkChange(value as "normal" | "slow" | "faulty")
								}
								options={NETWORK_OPTIONS}
								compact={isCompact}
							/>
						</div>
					</section>
				</CardContent>
			</Card>

			<Card>
				<CardHeader className={cn("p-3 pb-2", isCompact && "p-2 pb-1")}>
					<CardTitle className={cn("text-base", isCompact && "text-sm")}>
						Data
					</CardTitle>
				</CardHeader>
				<CardContent
					className={cn(
						"space-y-2.5 px-3 pb-3",
						isCompact && "space-y-1.5 px-2 pb-2",
					)}
				>
					<div className="flex items-center justify-between gap-3">
						<div className="space-y-0.5">
							<Label
								htmlFor={includeOwnReleasesSwitchId}
								className="justify-start gap-0 text-sm"
							>
								Include My Releases
							</Label>
							{isCompact ? null : (
								<p className="text-muted-foreground text-xs leading-4">
									影响 Settings 回显和 Dashboard owner-only release 露出。
								</p>
							)}
						</div>
						<Switch
							id={includeOwnReleasesSwitchId}
							checked={snapshot.shareState.includeOwnReleases}
							onCheckedChange={props.onIncludeOwnReleasesChange}
						/>
					</div>
					<div className={cn("space-y-1.5", isCompact && "space-y-0.5")}>
						<Label htmlFor={publicationStateSelectId}>
							Repo Public Release State
						</Label>
						<InspectorSelect
							id={publicationStateSelectId}
							value={snapshot.shareState.publicationState}
							onValueChange={(value) =>
								props.onPublicationStateChange(
									value as "published" | "unpublished",
								)
							}
							options={PUBLICATION_OPTIONS}
							compact={isCompact}
						/>
					</div>
				</CardContent>
			</Card>

			<Card>
				<CardHeader className={cn("p-3 pb-2", isCompact && "p-2 pb-1")}>
					<CardTitle className={cn("text-base", isCompact && "text-sm")}>
						Actions & Share
					</CardTitle>
				</CardHeader>
				<CardContent
					className={cn(
						"space-y-2.5 px-3 pb-3",
						isCompact && "space-y-1.5 px-2 pb-2",
					)}
				>
					<div className="flex flex-wrap gap-1.5">
						<Button
							type="button"
							variant="outline"
							size={isCompact ? "sm" : "default"}
							className={cn(isCompact && "h-7 gap-1.5 px-2.5 text-xs")}
							onClick={props.onReset}
						>
							<RefreshCcw className="size-4" />
							Reset Scene
						</Button>
						<Button
							type="button"
							variant="outline"
							size={isCompact ? "sm" : "default"}
							className={cn(isCompact && "h-7 gap-1.5 px-2.5 text-xs")}
							onClick={props.onCopyShareLink}
						>
							<Copy className="size-4" />
							Copy Share URL
						</Button>
					</div>
					<div className={cn("space-y-1.5", isCompact && "space-y-0.5")}>
						{isCompact ? null : <Label htmlFor={shareInputId}>Share</Label>}
						<Input
							id={shareInputId}
							readOnly
							value={props.shareHref}
							spellCheck={false}
							className={cn(
								"h-10 rounded-xl bg-muted/30 font-mono text-[10px] leading-4 md:text-[10px]",
								isCompact && "h-8 px-2 py-1 text-[9px] leading-3 md:text-[9px]",
							)}
						/>
					</div>
					{isCompact ? null : (
						<p className="text-muted-foreground text-xs">
							{snapshot.mutations.length === 0
								? "No simulated writes recorded yet."
								: `${snapshot.mutations.length} simulated write${snapshot.mutations.length > 1 ? "s" : ""} recorded.`}
						</p>
					)}
				</CardContent>
			</Card>

			<details
				className={cn(
					"rounded-2xl border bg-muted/20 p-2.5",
					isCompact && "p-1.5",
				)}
			>
				<summary className="cursor-pointer list-none">
					<div className="flex items-center justify-between gap-3">
						<p className="font-medium text-sm">Advanced</p>
						<div className="flex items-center gap-2">
							<Badge variant="outline">
								{snapshot.mutations.length === 0
									? "No writes yet"
									: `${snapshot.mutations.length} recorded`}
							</Badge>
							<Badge variant="outline">Raw JSON</Badge>
						</div>
					</div>
				</summary>
				<div className="mt-3 space-y-3">
					<section className="space-y-1.5">
						<p className="font-medium text-sm">Recent Mutations</p>
						{snapshot.mutations.length === 0 ? (
							<p className="text-muted-foreground text-sm">
								还没有 simulated
								write；直接在页面里保存、发布、取消或重试一次就会记录。
							</p>
						) : (
							<div className="space-y-1.5">
								{snapshot.mutations.map((mutation) => (
									<div
										key={mutation.id}
										className="rounded-xl border px-3 py-2"
									>
										<p className="font-medium text-sm">{mutation.label}</p>
										<p className="text-muted-foreground text-xs">
											{mutation.detail}
										</p>
									</div>
								))}
							</div>
						)}
					</section>
					<section className="space-y-1.5">
						<p className="font-medium text-sm">Raw JSON</p>
						<pre
							className={cn(
								"overflow-x-auto text-[11px] leading-5",
								"whitespace-pre-wrap break-all",
							)}
						>
							{JSON.stringify(snapshot.model, null, 2)}
						</pre>
					</section>
				</div>
			</details>
		</div>
	);
}
