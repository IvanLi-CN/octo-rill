import type * as React from "react";
import {
	createContext,
	useContext,
	useEffect,
	useLayoutEffect,
	useMemo,
	useRef,
	useState,
} from "react";

import { cn } from "@/lib/utils";

type AppShellScrollDirection = "idle" | "content-up" | "content-down";

type AppShellChromeState = {
	mobileChromeEnabled: boolean;
	isMobileViewport: boolean;
	compactHeader: boolean;
	headerProgress: number;
	headerInteracting: boolean;
	footerHidden: boolean;
	atTop: boolean;
	headerHeight: number;
	scrollDirection: AppShellScrollDirection;
};

const DEFAULT_CHROME_STATE: AppShellChromeState = {
	mobileChromeEnabled: false,
	isMobileViewport: false,
	compactHeader: false,
	headerProgress: 0,
	headerInteracting: false,
	footerHidden: false,
	atTop: true,
	headerHeight: 0,
	scrollDirection: "idle",
};

const AppShellChromeContext =
	createContext<AppShellChromeState>(DEFAULT_CHROME_STATE);

export function useAppShellChrome() {
	return useContext(AppShellChromeContext);
}

type AppShellProps = {
	header?: React.ReactNode;
	notice?: React.ReactNode;
	subheader?: React.ReactNode;
	subheaderClassName?: string;
	subheaderMode?: "always" | "mobile-compact";
	footer?: React.ReactNode;
	children: React.ReactNode;
	mobileChrome?: boolean;
};

export function AppShell({
	header,
	notice,
	subheader,
	subheaderClassName,
	subheaderMode = "always",
	footer,
	children,
	mobileChrome = false,
}: AppShellProps) {
	const SCROLL_DIRECTION_EPSILON = 2;
	const COMPACT_ENTER_MIN_TOP = 40;
	const HEADER_PROGRESS_DISTANCE = 96;
	const SNAP_TO_COMPACT_THRESHOLD = 0.65;
	const SNAP_TO_EXPANDED_THRESHOLD = 0.35;
	const SCROLL_SETTLE_DELAY_MS = 96;
	const TOUCH_FALLBACK_SETTLE_DELAY_MS = 160;
	const WHEEL_INTERACTION_WINDOW_MS = 140;
	const POST_TOUCH_DISCRETE_LOCK_MS = 260;
	const headerRef = useRef<HTMLElement | null>(null);
	const frameRef = useRef<number | null>(null);
	const settleTimeoutRef = useRef<number | null>(null);
	const lastScrollTopRef = useRef(0);
	const compactHeaderRef = useRef(false);
	const headerInteractingRef = useRef(false);
	const headerProgressRef = useRef(0);
	const progressAnchorTopRef = useRef(0);
	const progressAnchorValueRef = useRef(0);
	const progressDirectionRef = useRef<AppShellScrollDirection>("idle");
	const activeTouchPointerIdRef = useRef<number | null>(null);
	const activePointerTypeRef = useRef<"touch" | "mouse" | null>(null);
	const gestureStartTopRef = useRef(0);
	const gestureStartProgressRef = useRef(0);
	const gestureStartClientYRef = useRef(0);
	const touchFallbackTimeoutRef = useRef<number | null>(null);
	const lastWheelAtRef = useRef(Number.NEGATIVE_INFINITY);
	const discreteLockUntilRef = useRef(Number.NEGATIVE_INFINITY);
	const [isMobileViewport, setIsMobileViewport] = useState(false);
	const [atTop, setAtTop] = useState(true);
	const [scrollDirection, setScrollDirection] =
		useState<AppShellScrollDirection>("idle");
	const [compactHeader, setCompactHeader] = useState(false);
	const [headerProgress, setHeaderProgress] = useState(0);
	const [headerInteracting, setHeaderInteracting] = useState(false);
	const [headerHeight, setHeaderHeight] = useState(0);

	useEffect(() => {
		if (!mobileChrome || typeof window === "undefined") {
			setIsMobileViewport(false);
			return;
		}

		const mediaQuery = window.matchMedia("(max-width: 639px)");
		const updateViewport = () => {
			setIsMobileViewport(mediaQuery.matches);
		};

		updateViewport();
		mediaQuery.addEventListener("change", updateViewport);
		return () => {
			mediaQuery.removeEventListener("change", updateViewport);
		};
	}, [mobileChrome]);

	useEffect(() => {
		compactHeaderRef.current = compactHeader;
	}, [compactHeader]);

	useEffect(() => {
		headerProgressRef.current = headerProgress;
	}, [headerProgress]);

	useEffect(() => {
		headerInteractingRef.current = headerInteracting;
	}, [headerInteracting]);

	useEffect(() => {
		if (!mobileChrome || typeof window === "undefined") {
			setAtTop(true);
			setScrollDirection("idle");
			setCompactHeader(false);
			setHeaderProgress(0);
			setHeaderInteracting(false);
			compactHeaderRef.current = false;
			headerProgressRef.current = 0;
			headerInteractingRef.current = false;
			progressAnchorTopRef.current = 0;
			progressAnchorValueRef.current = 0;
			progressDirectionRef.current = "idle";
			gestureStartTopRef.current = 0;
			gestureStartProgressRef.current = 0;
			gestureStartClientYRef.current = 0;
			lastWheelAtRef.current = Number.NEGATIVE_INFINITY;
			discreteLockUntilRef.current = Number.NEGATIVE_INFINITY;
			activePointerTypeRef.current = null;
			activeTouchPointerIdRef.current = null;
			if (settleTimeoutRef.current !== null) {
				window.clearTimeout(settleTimeoutRef.current);
				settleTimeoutRef.current = null;
			}
			if (touchFallbackTimeoutRef.current !== null) {
				window.clearTimeout(touchFallbackTimeoutRef.current);
				touchFallbackTimeoutRef.current = null;
			}
			return;
		}

		const clampUnit = (value: number) => Math.max(0, Math.min(1, value));

		const clearTouchFallback = () => {
			if (touchFallbackTimeoutRef.current !== null) {
				window.clearTimeout(touchFallbackTimeoutRef.current);
				touchFallbackTimeoutRef.current = null;
			}
		};

		const resetProgressAnchors = (currentTop: number) => {
			progressAnchorTopRef.current = currentTop;
			progressAnchorValueRef.current = headerProgressRef.current;
			progressDirectionRef.current = "idle";
		};

		const applySettledState = (nextCompact: boolean, currentTop: number) => {
			if (settleTimeoutRef.current !== null) {
				window.clearTimeout(settleTimeoutRef.current);
				settleTimeoutRef.current = null;
			}
			clearTouchFallback();

			compactHeaderRef.current = nextCompact;
			headerProgressRef.current = nextCompact ? 1 : 0;
			setCompactHeader(nextCompact);
			setHeaderProgress(nextCompact ? 1 : 0);
			progressAnchorTopRef.current = currentTop;
			progressAnchorValueRef.current = nextCompact ? 1 : 0;
			progressDirectionRef.current = "idle";
			gestureStartTopRef.current = currentTop;
			gestureStartProgressRef.current = nextCompact ? 1 : 0;
			gestureStartClientYRef.current = 0;
			discreteLockUntilRef.current = Number.NEGATIVE_INFINITY;
			activePointerTypeRef.current = null;
		};

		const settleHeaderState = () => {
			const currentTop = window.scrollY;
			const nextCompact =
				currentTop > COMPACT_ENTER_MIN_TOP &&
				headerProgressRef.current >= SNAP_TO_COMPACT_THRESHOLD;
			const nextExpanded =
				currentTop <= COMPACT_ENTER_MIN_TOP ||
				headerProgressRef.current <= SNAP_TO_EXPANDED_THRESHOLD;

			if (compactHeaderRef.current) {
				applySettledState(!nextExpanded, currentTop);
				return;
			}

			applySettledState(nextCompact, currentTop);
		};

		const scheduleTouchFallbackSettle = () => {
			clearTouchFallback();
			touchFallbackTimeoutRef.current = window.setTimeout(() => {
				touchFallbackTimeoutRef.current = null;
				if (!headerInteractingRef.current) {
					return;
				}
				setHeaderInteracting(false);
				headerInteractingRef.current = false;
				settleHeaderState();
			}, TOUCH_FALLBACK_SETTLE_DELAY_MS);
		};

		const updateInteractiveGestureProgress = (clientY: number) => {
			const nextHeaderProgress = clampUnit(
				gestureStartProgressRef.current +
					(gestureStartClientYRef.current - clientY) / HEADER_PROGRESS_DISTANCE,
			);
			headerProgressRef.current = nextHeaderProgress;
			setHeaderProgress(nextHeaderProgress);
			clearTouchFallback();
			return nextHeaderProgress;
		};

		const promoteToTouchInteraction = (clientY?: number) => {
			const nextClientY = clientY ?? gestureStartClientYRef.current ?? 0;
			clearTouchFallback();
			discreteLockUntilRef.current = Number.NEGATIVE_INFINITY;

			if (!headerInteractingRef.current) {
				beginTouchInteraction({
					pointerType: "touch",
					clientY: nextClientY,
				});
				return;
			}

			activePointerTypeRef.current = "touch";
			activeTouchPointerIdRef.current = null;
			gestureStartTopRef.current = window.scrollY;
			gestureStartProgressRef.current = headerProgressRef.current;
			gestureStartClientYRef.current = nextClientY;
			resetProgressAnchors(window.scrollY);
		};

		const updateScrollState = () => {
			frameRef.current = null;
			const currentTop = window.scrollY;
			const previousTop = lastScrollTopRef.current;
			const delta = currentTop - previousTop;
			lastScrollTopRef.current = currentTop;
			const nextAtTop = currentTop <= 4;
			setAtTop(nextAtTop);

			if (nextAtTop && !headerInteractingRef.current) {
				setScrollDirection("idle");
				applySettledState(false, currentTop);
				activeTouchPointerIdRef.current = null;
				return;
			}

			if (Math.abs(delta) < SCROLL_DIRECTION_EPSILON) {
				return;
			}

			const nextDirection: AppShellScrollDirection =
				delta > 0 ? "content-up" : "content-down";
			const now =
				typeof performance !== "undefined" ? performance.now() : Date.now();
			const wheelDriven =
				now - lastWheelAtRef.current <= WHEEL_INTERACTION_WINDOW_MS;
			setScrollDirection(nextDirection);

			if (now < discreteLockUntilRef.current) {
				return;
			}

			if (headerInteractingRef.current) {
				return;
			}

			const currentProgress = headerProgressRef.current;

			if (progressDirectionRef.current !== nextDirection) {
				progressDirectionRef.current = nextDirection;
				progressAnchorTopRef.current = previousTop;
				progressAnchorValueRef.current = currentProgress;
			}

			const directionDistance =
				nextDirection === "content-up"
					? currentTop - progressAnchorTopRef.current
					: progressAnchorTopRef.current - currentTop;
			const nextHeaderProgress =
				nextDirection === "content-up"
					? clampUnit(
							progressAnchorValueRef.current +
								directionDistance / HEADER_PROGRESS_DISTANCE,
						)
					: clampUnit(
							progressAnchorValueRef.current -
								directionDistance / HEADER_PROGRESS_DISTANCE,
						);

			headerProgressRef.current = nextHeaderProgress;
			if (!wheelDriven) {
				setHeaderProgress(nextHeaderProgress);
			}

			if (settleTimeoutRef.current !== null) {
				window.clearTimeout(settleTimeoutRef.current);
			}
			settleTimeoutRef.current = window.setTimeout(() => {
				settleTimeoutRef.current = null;
				settleHeaderState();
			}, SCROLL_SETTLE_DELAY_MS);
		};

		lastScrollTopRef.current = window.scrollY;
		updateScrollState();

		const beginTouchInteraction = (options?: {
			pointerType?: "touch" | "mouse";
			clientY?: number;
		}) => {
			if (headerInteractingRef.current) {
				return;
			}

			if (settleTimeoutRef.current !== null) {
				window.clearTimeout(settleTimeoutRef.current);
				settleTimeoutRef.current = null;
			}
			clearTouchFallback();
			setHeaderInteracting(true);
			headerInteractingRef.current = true;
			discreteLockUntilRef.current = Number.NEGATIVE_INFINITY;
			activePointerTypeRef.current = options?.pointerType ?? "touch";
			gestureStartTopRef.current = window.scrollY;
			gestureStartProgressRef.current = headerProgressRef.current;
			gestureStartClientYRef.current = options?.clientY ?? 0;
			resetProgressAnchors(window.scrollY);
		};

		const endTouchInteraction = () => {
			if (!headerInteractingRef.current) {
				return;
			}

			const releasedPointerType = activePointerTypeRef.current;
			clearTouchFallback();
			setHeaderInteracting(false);
			headerInteractingRef.current = false;
			activePointerTypeRef.current = null;
			activeTouchPointerIdRef.current = null;
			gestureStartClientYRef.current = 0;
			discreteLockUntilRef.current =
				releasedPointerType === "touch"
					? (typeof performance !== "undefined"
							? performance.now()
							: Date.now()) + POST_TOUCH_DISCRETE_LOCK_MS
					: Number.NEGATIVE_INFINITY;
			settleHeaderState();
		};

		const handlePointerDown = (event: PointerEvent) => {
			const isDirectPointer =
				event.pointerType === "touch" ||
				(isMobileViewport &&
					event.pointerType === "mouse" &&
					event.isPrimary &&
					event.button === 0);
			if (!isDirectPointer) {
				return;
			}

			activeTouchPointerIdRef.current = event.pointerId;
			beginTouchInteraction({
				pointerType: event.pointerType === "mouse" ? "mouse" : "touch",
				clientY: event.clientY,
			});
		};

		const handlePointerUp = (event: PointerEvent) => {
			if (activeTouchPointerIdRef.current !== event.pointerId) {
				return;
			}

			const isDirectPointer =
				event.pointerType === "touch" ||
				(isMobileViewport && event.pointerType === "mouse" && event.isPrimary);
			if (!isDirectPointer) {
				return;
			}

			endTouchInteraction();
		};

		const handlePointerCancel = (event: PointerEvent) => {
			if (activeTouchPointerIdRef.current !== event.pointerId) {
				return;
			}

			activePointerTypeRef.current = null;
			activeTouchPointerIdRef.current = null;
			gestureStartClientYRef.current = 0;
			scheduleTouchFallbackSettle();
		};

		const handlePointerMove = (event: PointerEvent) => {
			if (
				activeTouchPointerIdRef.current !== event.pointerId ||
				!headerInteractingRef.current
			) {
				return;
			}

			if (
				activePointerTypeRef.current !== "mouse" &&
				activePointerTypeRef.current !== "touch"
			) {
				return;
			}

			updateInteractiveGestureProgress(event.clientY);

			if (activePointerTypeRef.current !== "mouse") {
				return;
			}

			const nextScrollTop = Math.max(
				0,
				gestureStartTopRef.current +
					(gestureStartClientYRef.current - event.clientY),
			);
			event.preventDefault();
			window.scrollTo({ top: nextScrollTop, behavior: "auto" });
		};

		const handleTouchStart = (event: TouchEvent) => {
			promoteToTouchInteraction(event.touches[0]?.clientY ?? 0);
		};

		const handleTouchEnd = (event: TouchEvent) => {
			if (event.touches.length > 0) {
				return;
			}
			endTouchInteraction();
		};

		const handleTouchMove = (event: TouchEvent) => {
			if (!headerInteractingRef.current) {
				return;
			}

			const primaryTouch = event.touches[0];
			if (!primaryTouch) {
				return;
			}

			if (activePointerTypeRef.current !== "touch") {
				promoteToTouchInteraction(primaryTouch.clientY);
			}

			updateInteractiveGestureProgress(primaryTouch.clientY);
		};

		const handleTouchCancel = () => {
			activeTouchPointerIdRef.current = null;
			scheduleTouchFallbackSettle();
		};

		const handleScroll = () => {
			if (frameRef.current !== null) return;
			frameRef.current = window.requestAnimationFrame(updateScrollState);
		};

		const handleWheel = () => {
			lastWheelAtRef.current =
				typeof performance !== "undefined" ? performance.now() : Date.now();
		};

		window.addEventListener("pointerdown", handlePointerDown, {
			passive: true,
		});
		window.addEventListener("pointermove", handlePointerMove, {
			passive: false,
		});
		window.addEventListener("pointerup", handlePointerUp, { passive: true });
		window.addEventListener("pointercancel", handlePointerCancel, {
			passive: true,
		});
		window.addEventListener("touchstart", handleTouchStart, { passive: true });
		window.addEventListener("touchmove", handleTouchMove, { passive: true });
		window.addEventListener("touchend", handleTouchEnd, { passive: true });
		window.addEventListener("touchcancel", handleTouchCancel, {
			passive: true,
		});
		window.addEventListener("wheel", handleWheel, { passive: true });
		window.addEventListener("scroll", handleScroll, { passive: true });
		return () => {
			window.removeEventListener("pointerdown", handlePointerDown);
			window.removeEventListener("pointermove", handlePointerMove);
			window.removeEventListener("pointerup", handlePointerUp);
			window.removeEventListener("pointercancel", handlePointerCancel);
			window.removeEventListener("touchstart", handleTouchStart);
			window.removeEventListener("touchmove", handleTouchMove);
			window.removeEventListener("touchend", handleTouchEnd);
			window.removeEventListener("touchcancel", handleTouchCancel);
			window.removeEventListener("wheel", handleWheel);
			window.removeEventListener("scroll", handleScroll);
			if (frameRef.current !== null) {
				window.cancelAnimationFrame(frameRef.current);
				frameRef.current = null;
			}
			if (settleTimeoutRef.current !== null) {
				window.clearTimeout(settleTimeoutRef.current);
				settleTimeoutRef.current = null;
			}
			clearTouchFallback();
		};
	}, [mobileChrome, isMobileViewport]);

	useEffect(() => {
		if (!mobileChrome || !isMobileViewport) {
			compactHeaderRef.current = false;
			headerProgressRef.current = 0;
			progressAnchorTopRef.current = 0;
			progressAnchorValueRef.current = 0;
			progressDirectionRef.current = "idle";
			gestureStartTopRef.current = 0;
			gestureStartProgressRef.current = 0;
			lastWheelAtRef.current = Number.NEGATIVE_INFINITY;
			discreteLockUntilRef.current = Number.NEGATIVE_INFINITY;
			activePointerTypeRef.current = null;
			activeTouchPointerIdRef.current = null;
			gestureStartClientYRef.current = 0;
			setCompactHeader(false);
			setHeaderProgress(0);
			setHeaderInteracting(false);
			headerInteractingRef.current = false;
			if (
				touchFallbackTimeoutRef.current !== null &&
				typeof window !== "undefined"
			) {
				window.clearTimeout(touchFallbackTimeoutRef.current);
				touchFallbackTimeoutRef.current = null;
			}
		}
	}, [isMobileViewport, mobileChrome]);

	useLayoutEffect(() => {
		const headerElement = headerRef.current;
		if (!headerElement) {
			setHeaderHeight(0);
			return;
		}

		const updateHeaderHeight = () => {
			setHeaderHeight(headerElement.offsetHeight);
		};

		updateHeaderHeight();
		const observer = new ResizeObserver(() => {
			updateHeaderHeight();
		});
		observer.observe(headerElement);
		return () => observer.disconnect();
	}, []);

	const mobileCompactHeader =
		mobileChrome && isMobileViewport && !atTop && compactHeader;
	const footerHidden = mobileChrome && isMobileViewport && !atTop;
	const showSubheader =
		Boolean(subheader) &&
		(subheaderMode === "always" ||
			(subheaderMode === "mobile-compact" && mobileCompactHeader));

	const chromeState = useMemo<AppShellChromeState>(
		() => ({
			mobileChromeEnabled: mobileChrome,
			isMobileViewport,
			compactHeader: mobileCompactHeader,
			headerProgress:
				mobileChrome && isMobileViewport && !atTop
					? Math.max(0, Math.min(1, headerProgress))
					: 0,
			headerInteracting,
			footerHidden,
			atTop,
			headerHeight,
			scrollDirection,
		}),
		[
			atTop,
			footerHidden,
			headerInteracting,
			headerHeight,
			headerProgress,
			isMobileViewport,
			mobileCompactHeader,
			mobileChrome,
			scrollDirection,
		],
	);

	return (
		<AppShellChromeContext.Provider value={chromeState}>
			<div
				className="min-h-screen"
				data-app-shell-mobile-chrome={mobileChrome ? "true" : "false"}
				data-app-shell-header-compact={mobileCompactHeader ? "true" : "false"}
				data-app-shell-header-progress={chromeState.headerProgress.toFixed(3)}
				data-app-shell-header-interacting={headerInteracting ? "true" : "false"}
				data-app-shell-footer-hidden={footerHidden ? "true" : "false"}
				style={
					{
						"--app-shell-header-height": `${headerHeight}px`,
					} as React.CSSProperties
				}
			>
				{header ? (
					<header
						ref={headerRef}
						className="supports-[backdrop-filter]:bg-background/70 bg-background/90 sticky top-0 z-20 border-b backdrop-blur motion-safe:transition-[background-color,border-color,box-shadow] motion-safe:duration-200 motion-safe:ease-out"
					>
						<div
							className={cn(
								"mx-auto max-w-6xl px-6 py-4 motion-safe:transition-[padding] motion-safe:duration-200 motion-safe:ease-out",
								mobileChrome && "px-4 py-3 sm:px-6 sm:py-4",
							)}
						>
							{header}
						</div>
					</header>
				) : null}
				{notice}
				{showSubheader ? (
					<div
						className={cn(
							"supports-[backdrop-filter]:bg-background/70 bg-background/95 sticky z-[15] border-b backdrop-blur",
							subheaderClassName,
						)}
						style={{
							top: mobileChrome
								? "var(--app-shell-header-height, 0px)"
								: undefined,
						}}
					>
						<div className="mx-auto max-w-6xl px-4 py-2 sm:px-6">
							{subheader}
						</div>
					</div>
				) : null}

				<main
					className={cn(
						"mx-auto max-w-6xl px-6 py-8",
						footer ? "pb-16" : null,
						mobileChrome && "px-4 py-4 pb-20 sm:px-6 sm:py-8 sm:pb-16",
					)}
				>
					{children}
				</main>
				{footer}
			</div>
		</AppShellChromeContext.Provider>
	);
}
