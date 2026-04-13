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
	footerHidden: boolean;
	atTop: boolean;
	headerHeight: number;
	scrollDirection: AppShellScrollDirection;
};

const DEFAULT_CHROME_STATE: AppShellChromeState = {
	mobileChromeEnabled: false,
	isMobileViewport: false,
	compactHeader: false,
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
	const COMPACT_ENTER_DISTANCE = 16;
	const COMPACT_EXIT_DISTANCE = 24;
	const USER_INPUT_DIRECTION_TTL_MS = 220;
	const PROGRAMMATIC_COMPACT_DISTANCE = 120;
	const PROGRAMMATIC_EXPAND_DISTANCE = 120;
	const headerRef = useRef<HTMLElement | null>(null);
	const frameRef = useRef<number | null>(null);
	const lastScrollTopRef = useRef(0);
	const gestureAnchorTopRef = useRef(0);
	const gestureDirectionRef = useRef<AppShellScrollDirection>("idle");
	const compactHeaderRef = useRef(false);
	const compactPeakTopRef = useRef(0);
	const inputDirectionRef = useRef<AppShellScrollDirection>("idle");
	const inputDirectionAtRef = useRef(0);
	const touchLastYRef = useRef<number | null>(null);
	const [isMobileViewport, setIsMobileViewport] = useState(false);
	const [atTop, setAtTop] = useState(true);
	const [scrollDirection, setScrollDirection] =
		useState<AppShellScrollDirection>("idle");
	const [compactHeader, setCompactHeader] = useState(false);
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
		if (!mobileChrome || typeof window === "undefined") {
			setAtTop(true);
			setScrollDirection("idle");
			setCompactHeader(false);
			compactHeaderRef.current = false;
			gestureAnchorTopRef.current = 0;
			gestureDirectionRef.current = "idle";
			compactPeakTopRef.current = 0;
			inputDirectionRef.current = "idle";
			inputDirectionAtRef.current = 0;
			touchLastYRef.current = null;
			return;
		}

		const setRecentInputDirection = (direction: AppShellScrollDirection) => {
			if (direction === "idle") return;
			inputDirectionRef.current = direction;
			inputDirectionAtRef.current = window.performance.now();
		};

		const updateScrollState = () => {
			frameRef.current = null;
			const currentTop = window.scrollY;
			const previousTop = lastScrollTopRef.current;
			const delta = currentTop - previousTop;
			const now = window.performance.now();
			lastScrollTopRef.current = currentTop;
			const nextAtTop = currentTop <= 4;
			const currentCompactHeader = compactHeaderRef.current;
			const hasRecentInputDirection = (direction: AppShellScrollDirection) =>
				inputDirectionRef.current === direction &&
				now - inputDirectionAtRef.current <= USER_INPUT_DIRECTION_TTL_MS;
			setAtTop(nextAtTop);

			if (nextAtTop) {
				setScrollDirection("idle");
				setCompactHeader(false);
				compactHeaderRef.current = false;
				gestureAnchorTopRef.current = 0;
				gestureDirectionRef.current = "idle";
				compactPeakTopRef.current = 0;
				inputDirectionRef.current = "idle";
				inputDirectionAtRef.current = 0;
				return;
			}

			if (Math.abs(delta) < SCROLL_DIRECTION_EPSILON) {
				return;
			}

			const nextDirection: AppShellScrollDirection =
				delta > 0 ? "content-up" : "content-down";

			if (gestureDirectionRef.current !== nextDirection) {
				gestureDirectionRef.current = nextDirection;
				gestureAnchorTopRef.current = previousTop;
			}

			const directionDistance =
				nextDirection === "content-up"
					? currentTop - gestureAnchorTopRef.current
					: gestureAnchorTopRef.current - currentTop;

			setScrollDirection(nextDirection);

			if (!currentCompactHeader) {
				if (
					nextDirection === "content-up" &&
					currentTop >= COMPACT_ENTER_MIN_TOP &&
					directionDistance >= COMPACT_ENTER_DISTANCE &&
					(hasRecentInputDirection("content-up") ||
						directionDistance >= PROGRAMMATIC_COMPACT_DISTANCE)
				) {
					compactHeaderRef.current = true;
					compactPeakTopRef.current = currentTop;
					setCompactHeader(true);
					gestureAnchorTopRef.current = currentTop;
					gestureDirectionRef.current = nextDirection;
				}
				return;
			}

			compactPeakTopRef.current = Math.max(
				compactPeakTopRef.current,
				currentTop,
			);

			if (currentTop <= COMPACT_ENTER_MIN_TOP - 8) {
				compactHeaderRef.current = false;
				compactPeakTopRef.current = 0;
				setCompactHeader(false);
				gestureAnchorTopRef.current = currentTop;
				gestureDirectionRef.current = "content-down";
				return;
			}

			if (nextDirection === "content-up") {
				return;
			}

			if (
				nextDirection === "content-down" &&
				compactPeakTopRef.current - currentTop >= COMPACT_EXIT_DISTANCE &&
				(hasRecentInputDirection("content-down") ||
					compactPeakTopRef.current - currentTop >=
						PROGRAMMATIC_EXPAND_DISTANCE)
			) {
				compactHeaderRef.current = false;
				compactPeakTopRef.current = 0;
				setCompactHeader(false);
				gestureAnchorTopRef.current = currentTop;
				gestureDirectionRef.current = nextDirection;
			}
		};

		lastScrollTopRef.current = window.scrollY;
		updateScrollState();

		const handleWheel = (event: WheelEvent) => {
			if (Math.abs(event.deltaY) < SCROLL_DIRECTION_EPSILON) {
				return;
			}

			setRecentInputDirection(event.deltaY > 0 ? "content-up" : "content-down");
		};

		const handleTouchStart = (event: TouchEvent) => {
			touchLastYRef.current = event.touches[0]?.clientY ?? null;
		};

		const handleTouchMove = (event: TouchEvent) => {
			const touchY = event.touches[0]?.clientY;
			const previousTouchY = touchLastYRef.current;
			touchLastYRef.current = touchY ?? null;

			if (touchY == null || previousTouchY == null) {
				return;
			}

			const deltaY = touchY - previousTouchY;
			if (Math.abs(deltaY) < SCROLL_DIRECTION_EPSILON) {
				return;
			}

			setRecentInputDirection(deltaY < 0 ? "content-up" : "content-down");
		};

		const clearTouchGesture = () => {
			touchLastYRef.current = null;
		};

		const handleScroll = () => {
			if (frameRef.current !== null) return;
			frameRef.current = window.requestAnimationFrame(updateScrollState);
		};

		window.addEventListener("wheel", handleWheel, { passive: true });
		window.addEventListener("touchstart", handleTouchStart, { passive: true });
		window.addEventListener("touchmove", handleTouchMove, { passive: true });
		window.addEventListener("touchend", clearTouchGesture, { passive: true });
		window.addEventListener("touchcancel", clearTouchGesture, {
			passive: true,
		});
		window.addEventListener("scroll", handleScroll, { passive: true });
		return () => {
			window.removeEventListener("wheel", handleWheel);
			window.removeEventListener("touchstart", handleTouchStart);
			window.removeEventListener("touchmove", handleTouchMove);
			window.removeEventListener("touchend", clearTouchGesture);
			window.removeEventListener("touchcancel", clearTouchGesture);
			window.removeEventListener("scroll", handleScroll);
			if (frameRef.current !== null) {
				window.cancelAnimationFrame(frameRef.current);
				frameRef.current = null;
			}
		};
	}, [mobileChrome]);

	useEffect(() => {
		if (!mobileChrome || !isMobileViewport) {
			compactHeaderRef.current = false;
			compactPeakTopRef.current = 0;
			inputDirectionRef.current = "idle";
			inputDirectionAtRef.current = 0;
			touchLastYRef.current = null;
			setCompactHeader(false);
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
			footerHidden,
			atTop,
			headerHeight,
			scrollDirection,
		}),
		[
			atTop,
			footerHidden,
			headerHeight,
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
