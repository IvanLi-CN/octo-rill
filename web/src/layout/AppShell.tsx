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

import { AppToastViewportHost } from "@/components/feedback/AppToast";
import { cn } from "@/lib/utils";

type AppShellScrollDirection = "idle" | "content-up" | "content-down";

type AppShellChromeState = {
	mobileChromeEnabled: boolean;
	isMobileViewport: boolean;
	compactHeader: boolean;
	headerProgress: number;
	headerInteracting: boolean;
	headerTransitionSuppressed: boolean;
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
	headerTransitionSuppressed: false,
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

const MOBILE_HEADER_GESTURE_GUARD_SELECTOR = "[data-app-shell-gesture-guard]";
const GUARDED_TOUCH_DRAG_THRESHOLD_PX = 12;
const GUARDED_TOUCH_CLICK_SUPPRESSION_WINDOW_MS = 750;

function resolveGestureTargetElement(
	target: EventTarget | null,
): Element | null {
	if (target instanceof Element) {
		return target;
	}
	if (target instanceof Node) {
		return target.parentElement;
	}
	return null;
}

function resolveGestureGuardElement(
	target: EventTarget | null,
): Element | null {
	return (
		resolveGestureTargetElement(target)?.closest(
			MOBILE_HEADER_GESTURE_GUARD_SELECTOR,
		) ?? null
	);
}

function isInteractiveGestureTarget(target: EventTarget | null): boolean {
	return resolveGestureGuardElement(target) !== null;
}

function shouldPromoteGuardedTouchSequence({
	clientX,
	clientY,
	startClientX,
	startClientY,
}: {
	clientX: number;
	clientY: number;
	startClientX: number;
	startClientY: number;
}) {
	const horizontalDelta = Math.abs(clientX - startClientX);
	const verticalDelta = Math.abs(clientY - startClientY);
	return (
		verticalDelta >= GUARDED_TOUCH_DRAG_THRESHOLD_PX &&
		verticalDelta >= horizontalDelta
	);
}

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
	const INTERACTION_RELEASE_THRESHOLD = 0.5;
	const WHEEL_TOGGLE_DELTA = 36;
	const SCROLL_SETTLE_DELAY_MS = 96;
	const TOUCH_FALLBACK_SETTLE_DELAY_MS = 160;
	const WHEEL_INTERACTION_WINDOW_MS = 140;
	const POST_WHEEL_DISCRETE_LOCK_MS = 420;
	const POST_TOUCH_DISCRETE_LOCK_MS = 260;
	const headerRef = useRef<HTMLElement | null>(null);
	const frameRef = useRef<number | null>(null);
	const settleTimeoutRef = useRef<number | null>(null);
	const deferredScrollTimeoutRef = useRef<number | null>(null);
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
	const ignoreInteractiveTouchSequenceRef = useRef(false);
	const guardedTouchStartClientXRef = useRef(0);
	const guardedTouchStartClientYRef = useRef(0);
	const guardedTouchTargetRef = useRef<Element | null>(null);
	const suppressedGuardedClickUntilRef = useRef(Number.NEGATIVE_INFINITY);
	const suppressedGuardedClickTargetRef = useRef<Element | null>(null);
	const touchFallbackTimeoutRef = useRef<number | null>(null);
	const transitionSuppressionTimeoutRef = useRef<number | null>(null);
	const lastWheelAtRef = useRef(Number.NEGATIVE_INFINITY);
	const wheelDeltaAccumulatorRef = useRef(0);
	const discreteLockUntilRef = useRef(Number.NEGATIVE_INFINITY);
	const [isMobileViewport, setIsMobileViewport] = useState(false);
	const [atTop, setAtTop] = useState(true);
	const [scrollDirection, setScrollDirection] =
		useState<AppShellScrollDirection>("idle");
	const [compactHeader, setCompactHeader] = useState(false);
	const [headerProgress, setHeaderProgress] = useState(0);
	const [headerInteracting, setHeaderInteracting] = useState(false);
	const [headerTransitionSuppressed, setHeaderTransitionSuppressed] =
		useState(false);
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
			setHeaderTransitionSuppressed(false);
			compactHeaderRef.current = false;
			headerProgressRef.current = 0;
			headerInteractingRef.current = false;
			progressAnchorTopRef.current = 0;
			progressAnchorValueRef.current = 0;
			progressDirectionRef.current = "idle";
			gestureStartTopRef.current = 0;
			gestureStartProgressRef.current = 0;
			gestureStartClientYRef.current = 0;
			ignoreInteractiveTouchSequenceRef.current = false;
			guardedTouchStartClientXRef.current = 0;
			guardedTouchStartClientYRef.current = 0;
			guardedTouchTargetRef.current = null;
			suppressedGuardedClickUntilRef.current = Number.NEGATIVE_INFINITY;
			suppressedGuardedClickTargetRef.current = null;
			lastWheelAtRef.current = Number.NEGATIVE_INFINITY;
			wheelDeltaAccumulatorRef.current = 0;
			discreteLockUntilRef.current = Number.NEGATIVE_INFINITY;
			activePointerTypeRef.current = null;
			activeTouchPointerIdRef.current = null;
			if (settleTimeoutRef.current !== null) {
				window.clearTimeout(settleTimeoutRef.current);
				settleTimeoutRef.current = null;
			}
			if (deferredScrollTimeoutRef.current !== null) {
				window.clearTimeout(deferredScrollTimeoutRef.current);
				deferredScrollTimeoutRef.current = null;
			}
			if (touchFallbackTimeoutRef.current !== null) {
				window.clearTimeout(touchFallbackTimeoutRef.current);
				touchFallbackTimeoutRef.current = null;
			}
			if (transitionSuppressionTimeoutRef.current !== null) {
				window.clearTimeout(transitionSuppressionTimeoutRef.current);
				transitionSuppressionTimeoutRef.current = null;
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

		const scheduleDeferredScrollAfterLock = (delayMs: number) => {
			if (deferredScrollTimeoutRef.current !== null) {
				window.clearTimeout(deferredScrollTimeoutRef.current);
			}
			deferredScrollTimeoutRef.current = window.setTimeout(
				() => {
					deferredScrollTimeoutRef.current = null;
					if (frameRef.current !== null) {
						return;
					}
					frameRef.current = window.requestAnimationFrame(updateScrollState);
				},
				Math.max(0, delayMs),
			);
		};

		const clearTransitionSuppression = () => {
			if (transitionSuppressionTimeoutRef.current !== null) {
				window.clearTimeout(transitionSuppressionTimeoutRef.current);
				transitionSuppressionTimeoutRef.current = null;
			}
			setHeaderTransitionSuppressed(false);
		};

		const suppressHeaderTransitionsFor = (durationMs: number) => {
			if (transitionSuppressionTimeoutRef.current !== null) {
				window.clearTimeout(transitionSuppressionTimeoutRef.current);
			}
			setHeaderTransitionSuppressed(true);
			transitionSuppressionTimeoutRef.current = window.setTimeout(() => {
				transitionSuppressionTimeoutRef.current = null;
				setHeaderTransitionSuppressed(false);
			}, durationMs);
		};

		const resetProgressAnchors = (currentTop: number) => {
			progressAnchorTopRef.current = currentTop;
			progressAnchorValueRef.current = headerProgressRef.current;
			progressDirectionRef.current = "idle";
		};

		const clearSuppressedGuardedClick = () => {
			suppressedGuardedClickUntilRef.current = Number.NEGATIVE_INFINITY;
			suppressedGuardedClickTargetRef.current = null;
		};

		const clearGuardedTouchSequence = () => {
			ignoreInteractiveTouchSequenceRef.current = false;
			guardedTouchStartClientXRef.current = 0;
			guardedTouchStartClientYRef.current = 0;
			guardedTouchTargetRef.current = null;
		};

		const suppressPendingGuardedClick = () => {
			if (!guardedTouchTargetRef.current) {
				return;
			}
			suppressedGuardedClickTargetRef.current = guardedTouchTargetRef.current;
			suppressedGuardedClickUntilRef.current =
				(typeof performance !== "undefined" ? performance.now() : Date.now()) +
				GUARDED_TOUCH_CLICK_SUPPRESSION_WINDOW_MS;
		};

		const shouldSuppressGuardedClickTarget = (target: EventTarget | null) => {
			const suppressedTarget = suppressedGuardedClickTargetRef.current;
			if (!suppressedTarget) {
				return false;
			}

			const now =
				typeof performance !== "undefined" ? performance.now() : Date.now();
			if (now > suppressedGuardedClickUntilRef.current) {
				clearSuppressedGuardedClick();
				return false;
			}

			return target instanceof Node && suppressedTarget.contains(target);
		};

		const promoteGuardedTouchSequence = ({
			clientX,
			clientY,
		}: {
			clientX: number;
			clientY: number;
		}) => {
			if (
				!shouldPromoteGuardedTouchSequence({
					clientX,
					clientY,
					startClientX: guardedTouchStartClientXRef.current,
					startClientY: guardedTouchStartClientYRef.current,
				})
			) {
				return false;
			}

			suppressPendingGuardedClick();
			ignoreInteractiveTouchSequenceRef.current = false;
			promoteToTouchInteraction(guardedTouchStartClientYRef.current);
			updateInteractiveGestureProgress(clientY);
			return true;
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
			wheelDeltaAccumulatorRef.current = 0;
			activePointerTypeRef.current = null;
			clearGuardedTouchSequence();
		};

		const settleHeaderState = (
			mode: "scroll" | "interaction" | "wheel" = "scroll",
		) => {
			const currentTop = window.scrollY;
			if (currentTop <= COMPACT_ENTER_MIN_TOP) {
				applySettledState(false, currentTop);
				return;
			}

			if (mode === "interaction") {
				applySettledState(
					headerProgressRef.current >= INTERACTION_RELEASE_THRESHOLD,
					currentTop,
				);
				return;
			}

			if (mode === "wheel") {
				if (Math.abs(wheelDeltaAccumulatorRef.current) < WHEEL_TOGGLE_DELTA) {
					applySettledState(compactHeaderRef.current, currentTop);
					return;
				}

				applySettledState(wheelDeltaAccumulatorRef.current > 0, currentTop);
				return;
			}

			const nextCompact =
				headerProgressRef.current >= SNAP_TO_COMPACT_THRESHOLD;
			const nextExpanded =
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
			clearTransitionSuppression();
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
			const wheelSequenceActive =
				wheelDriven || wheelDeltaAccumulatorRef.current !== 0;
			setScrollDirection(nextDirection);

			if (now < discreteLockUntilRef.current && !wheelDriven) {
				scheduleDeferredScrollAfterLock(discreteLockUntilRef.current - now + 4);
				return;
			}

			if (headerInteractingRef.current) {
				return;
			}

			if (wheelSequenceActive) {
				if (settleTimeoutRef.current !== null) {
					window.clearTimeout(settleTimeoutRef.current);
				}
				settleTimeoutRef.current = window.setTimeout(() => {
					settleTimeoutRef.current = null;
					if (wheelDeltaAccumulatorRef.current === 0) {
						return;
					}

					settleHeaderState("wheel");
					discreteLockUntilRef.current =
						(typeof performance !== "undefined"
							? performance.now()
							: Date.now()) + POST_WHEEL_DISCRETE_LOCK_MS;
					suppressHeaderTransitionsFor(POST_WHEEL_DISCRETE_LOCK_MS);
				}, SCROLL_SETTLE_DELAY_MS);
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
			if (deferredScrollTimeoutRef.current !== null) {
				window.clearTimeout(deferredScrollTimeoutRef.current);
				deferredScrollTimeoutRef.current = null;
			}
			clearTouchFallback();
			clearTransitionSuppression();
			setHeaderInteracting(true);
			headerInteractingRef.current = true;
			clearGuardedTouchSequence();
			discreteLockUntilRef.current = Number.NEGATIVE_INFINITY;
			activePointerTypeRef.current = options?.pointerType ?? "touch";
			gestureStartTopRef.current = window.scrollY;
			gestureStartProgressRef.current = headerProgressRef.current;
			gestureStartClientYRef.current = options?.clientY ?? 0;
			resetProgressAnchors(window.scrollY);
		};

		const endTouchInteraction = (releasedPointerType?: "touch" | "mouse") => {
			if (!headerInteractingRef.current) {
				return;
			}

			const settledPointerType =
				releasedPointerType ?? activePointerTypeRef.current;
			clearTouchFallback();
			setHeaderInteracting(false);
			headerInteractingRef.current = false;
			clearGuardedTouchSequence();
			activePointerTypeRef.current = null;
			activeTouchPointerIdRef.current = null;
			gestureStartClientYRef.current = 0;
			settleHeaderState(
				settledPointerType === "touch" ? "interaction" : "scroll",
			);
			discreteLockUntilRef.current =
				settledPointerType === "touch"
					? (typeof performance !== "undefined"
							? performance.now()
							: Date.now()) + POST_TOUCH_DISCRETE_LOCK_MS
					: Number.NEGATIVE_INFINITY;
		};

		const handlePointerDown = (event: PointerEvent) => {
			const isTouchPointer = event.pointerType === "touch";
			const isDirectPointer =
				isTouchPointer ||
				(isMobileViewport &&
					event.pointerType === "mouse" &&
					event.isPrimary &&
					event.button === 0);
			if (!isDirectPointer) {
				return;
			}
			clearSuppressedGuardedClick();
			if (isTouchPointer) {
				ignoreInteractiveTouchSequenceRef.current = isInteractiveGestureTarget(
					event.target,
				);
				guardedTouchTargetRef.current = resolveGestureGuardElement(
					event.target,
				);
				activeTouchPointerIdRef.current = event.pointerId;
				if (ignoreInteractiveTouchSequenceRef.current) {
					guardedTouchStartClientXRef.current = event.clientX;
					guardedTouchStartClientYRef.current = event.clientY;
					return;
				}
			} else {
				clearGuardedTouchSequence();
			}

			activeTouchPointerIdRef.current = event.pointerId;
			beginTouchInteraction({
				pointerType: event.pointerType === "mouse" ? "mouse" : "touch",
				clientY: event.clientY,
			});
		};

		const handlePointerUp = (event: PointerEvent) => {
			if (
				event.pointerType === "touch" &&
				ignoreInteractiveTouchSequenceRef.current
			) {
				if (activeTouchPointerIdRef.current === event.pointerId) {
					activeTouchPointerIdRef.current = null;
					clearGuardedTouchSequence();
				}
				return;
			}
			if (activeTouchPointerIdRef.current !== event.pointerId) {
				return;
			}

			const isDirectPointer =
				event.pointerType === "touch" ||
				(isMobileViewport && event.pointerType === "mouse" && event.isPrimary);
			if (!isDirectPointer) {
				return;
			}
			if (shouldSuppressGuardedClickTarget(event.target) && event.cancelable) {
				event.preventDefault();
			}

			endTouchInteraction(event.pointerType === "mouse" ? "mouse" : "touch");
		};

		const handlePointerCancel = (event: PointerEvent) => {
			if (
				event.pointerType === "touch" &&
				ignoreInteractiveTouchSequenceRef.current
			) {
				if (activeTouchPointerIdRef.current === event.pointerId) {
					activeTouchPointerIdRef.current = null;
					clearGuardedTouchSequence();
				}
				return;
			}
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
				event.pointerType === "touch" &&
				ignoreInteractiveTouchSequenceRef.current
			) {
				if (activeTouchPointerIdRef.current !== event.pointerId) {
					return;
				}
				promoteGuardedTouchSequence({
					clientX: event.clientX,
					clientY: event.clientY,
				});
				return;
			}
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
			clearSuppressedGuardedClick();
			ignoreInteractiveTouchSequenceRef.current = isInteractiveGestureTarget(
				event.target,
			);
			guardedTouchTargetRef.current = resolveGestureGuardElement(event.target);
			if (ignoreInteractiveTouchSequenceRef.current) {
				const primaryTouch = event.touches[0];
				guardedTouchStartClientXRef.current = primaryTouch?.clientX ?? 0;
				guardedTouchStartClientYRef.current = primaryTouch?.clientY ?? 0;
				return;
			}
			clearGuardedTouchSequence();
			promoteToTouchInteraction(event.touches[0]?.clientY ?? 0);
		};

		const handleTouchEnd = (event: TouchEvent) => {
			if (ignoreInteractiveTouchSequenceRef.current) {
				activeTouchPointerIdRef.current = null;
				clearGuardedTouchSequence();
				return;
			}
			if (event.touches.length > 0) {
				return;
			}
			if (shouldSuppressGuardedClickTarget(event.target) && event.cancelable) {
				event.preventDefault();
			}
			endTouchInteraction("touch");
		};

		const handleTouchMove = (event: TouchEvent) => {
			const primaryTouch = event.touches[0];
			if (!primaryTouch) {
				return;
			}

			if (ignoreInteractiveTouchSequenceRef.current) {
				if (
					!promoteGuardedTouchSequence({
						clientX: primaryTouch.clientX,
						clientY: primaryTouch.clientY,
					})
				) {
					return;
				}
				return;
			}
			if (!headerInteractingRef.current) {
				return;
			}

			if (activePointerTypeRef.current !== "touch") {
				promoteToTouchInteraction(primaryTouch.clientY);
			}

			updateInteractiveGestureProgress(primaryTouch.clientY);
		};

		const handleTouchCancel = () => {
			if (ignoreInteractiveTouchSequenceRef.current) {
				activeTouchPointerIdRef.current = null;
				clearGuardedTouchSequence();
				return;
			}
			activeTouchPointerIdRef.current = null;
			scheduleTouchFallbackSettle();
		};

		const handleClickCapture = (event: MouseEvent) => {
			if (!shouldSuppressGuardedClickTarget(event.target)) {
				return;
			}

			event.preventDefault();
			event.stopPropagation();
			clearSuppressedGuardedClick();
		};

		const handleScroll = () => {
			if (frameRef.current !== null) return;
			frameRef.current = window.requestAnimationFrame(updateScrollState);
		};

		const handleWheel = (event: WheelEvent) => {
			const now =
				typeof performance !== "undefined" ? performance.now() : Date.now();
			if (now - lastWheelAtRef.current > WHEEL_INTERACTION_WINDOW_MS * 2) {
				wheelDeltaAccumulatorRef.current = 0;
			}
			lastWheelAtRef.current = now;
			discreteLockUntilRef.current = Number.NEGATIVE_INFINITY;
			setHeaderTransitionSuppressed(true);
			wheelDeltaAccumulatorRef.current += event.deltaY;
		};

		window.addEventListener("pointerdown", handlePointerDown, {
			passive: true,
		});
		window.addEventListener("pointermove", handlePointerMove, {
			passive: false,
		});
		window.addEventListener("pointerup", handlePointerUp, { passive: false });
		window.addEventListener("pointercancel", handlePointerCancel, {
			passive: true,
		});
		window.addEventListener("touchstart", handleTouchStart, { passive: true });
		window.addEventListener("touchmove", handleTouchMove, { passive: true });
		window.addEventListener("touchend", handleTouchEnd, { passive: false });
		window.addEventListener("touchcancel", handleTouchCancel, {
			passive: true,
		});
		window.addEventListener("click", handleClickCapture, {
			capture: true,
			passive: false,
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
			window.removeEventListener("click", handleClickCapture, true);
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
			clearTransitionSuppression();
			clearSuppressedGuardedClick();
			clearGuardedTouchSequence();
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
			wheelDeltaAccumulatorRef.current = 0;
			discreteLockUntilRef.current = Number.NEGATIVE_INFINITY;
			activePointerTypeRef.current = null;
			activeTouchPointerIdRef.current = null;
			gestureStartClientYRef.current = 0;
			ignoreInteractiveTouchSequenceRef.current = false;
			guardedTouchStartClientXRef.current = 0;
			guardedTouchStartClientYRef.current = 0;
			guardedTouchTargetRef.current = null;
			suppressedGuardedClickUntilRef.current = Number.NEGATIVE_INFINITY;
			suppressedGuardedClickTargetRef.current = null;
			setCompactHeader(false);
			setHeaderProgress(0);
			setHeaderInteracting(false);
			setHeaderTransitionSuppressed(false);
			headerInteractingRef.current = false;
			if (
				touchFallbackTimeoutRef.current !== null &&
				typeof window !== "undefined"
			) {
				window.clearTimeout(touchFallbackTimeoutRef.current);
				touchFallbackTimeoutRef.current = null;
			}
			if (
				transitionSuppressionTimeoutRef.current !== null &&
				typeof window !== "undefined"
			) {
				window.clearTimeout(transitionSuppressionTimeoutRef.current);
				transitionSuppressionTimeoutRef.current = null;
			}
			if (
				deferredScrollTimeoutRef.current !== null &&
				typeof window !== "undefined"
			) {
				window.clearTimeout(deferredScrollTimeoutRef.current);
				deferredScrollTimeoutRef.current = null;
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
			headerTransitionSuppressed,
			footerHidden,
			atTop,
			headerHeight,
			scrollDirection,
		}),
		[
			atTop,
			footerHidden,
			headerInteracting,
			headerTransitionSuppressed,
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
				<AppToastViewportHost />
				{footer}
			</div>
		</AppShellChromeContext.Provider>
	);
}
