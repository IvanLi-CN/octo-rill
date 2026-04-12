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

type AppShellScrollDirection = "idle" | "up" | "down";

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
	footer?: React.ReactNode;
	children: React.ReactNode;
	mobileChrome?: boolean;
};

export function AppShell({
	header,
	notice,
	subheader,
	subheaderClassName,
	footer,
	children,
	mobileChrome = false,
}: AppShellProps) {
	const headerRef = useRef<HTMLElement | null>(null);
	const frameRef = useRef<number | null>(null);
	const lastScrollTopRef = useRef(0);
	const [isMobileViewport, setIsMobileViewport] = useState(false);
	const [atTop, setAtTop] = useState(true);
	const [scrollDirection, setScrollDirection] =
		useState<AppShellScrollDirection>("idle");
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
		if (!mobileChrome || typeof window === "undefined") {
			setAtTop(true);
			setScrollDirection("idle");
			return;
		}

		const updateScrollState = () => {
			frameRef.current = null;
			const currentTop = window.scrollY;
			const delta = currentTop - lastScrollTopRef.current;
			lastScrollTopRef.current = currentTop;
			const nextAtTop = currentTop <= 4;
			setAtTop(nextAtTop);

			if (nextAtTop) {
				setScrollDirection("idle");
				return;
			}

			if (Math.abs(delta) < 6) {
				return;
			}

			setScrollDirection(delta > 0 ? "down" : "up");
		};

		lastScrollTopRef.current = window.scrollY;
		updateScrollState();

		const handleScroll = () => {
			if (frameRef.current !== null) return;
			frameRef.current = window.requestAnimationFrame(updateScrollState);
		};

		window.addEventListener("scroll", handleScroll, { passive: true });
		return () => {
			window.removeEventListener("scroll", handleScroll);
			if (frameRef.current !== null) {
				window.cancelAnimationFrame(frameRef.current);
				frameRef.current = null;
			}
		};
	}, [mobileChrome]);

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

	const compactHeader =
		mobileChrome && isMobileViewport && !atTop && scrollDirection === "up";
	const footerHidden = mobileChrome && isMobileViewport && !atTop;
	const chromeState = useMemo<AppShellChromeState>(
		() => ({
			mobileChromeEnabled: mobileChrome,
			isMobileViewport,
			compactHeader,
			footerHidden,
			atTop,
			headerHeight,
			scrollDirection,
		}),
		[
			atTop,
			compactHeader,
			footerHidden,
			headerHeight,
			isMobileViewport,
			mobileChrome,
			scrollDirection,
		],
	);

	return (
		<AppShellChromeContext.Provider value={chromeState}>
			<div
				className="min-h-screen"
				data-app-shell-mobile-chrome={mobileChrome ? "true" : "false"}
				data-app-shell-header-compact={compactHeader ? "true" : "false"}
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
						className="supports-[backdrop-filter]:bg-background/70 bg-background/90 sticky top-0 z-20 border-b backdrop-blur"
					>
						<div
							className={cn(
								"mx-auto max-w-6xl px-6 py-4",
								mobileChrome && "px-4 py-3 sm:px-6 sm:py-4",
							)}
						>
							{header}
						</div>
					</header>
				) : null}
				{notice}
				{subheader ? (
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
