import {
	lazy,
	type CSSProperties,
	type ReactNode,
	Suspense,
	useEffect,
} from "react";
import {
	createRootRoute,
	Outlet,
	useLocation,
	useRouter,
} from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import {
	DEMO_APP_MAX_FRAME_WIDTH_PX,
	DEMO_INSPECTOR_DOCKED_BREAKPOINT_PX,
	DEMO_INSPECTOR_DOCKED_CONTENT_OFFSET_PX,
	DEMO_INSPECTOR_DOCKED_VIEWPORT_GUTTER_PX,
} from "@/demo/layout";
import {
	isDemoMode,
	shouldPrepareDemoRuntime,
	useDemoSnapshot,
} from "@/demo/runtime";
import { useMediaQuery } from "@/lib/useMediaQuery";
import { AppBoot } from "@/pages/AppBoot";
import { NotFoundPage } from "@/pages/NotFound";
import { PausedAccountPage } from "@/pages/PausedAccount";

const LazyDemoInspector = lazy(async () => {
	const module = await import("@/demo/DemoInspector");
	return {
		default: module.DemoInspector,
	};
});

export const Route = createRootRoute({
	component: RootRouteComponent,
	notFoundComponent: RootRouteNotFoundComponent,
});

function RootRouteComponent() {
	const auth = useAuthBootstrap();
	const location = useLocation();
	const router = useRouter();
	const demoSnapshot = useDemoSnapshot();

	useEffect(() => {
		if (auth.status !== "paused" || location.pathname === "/account/paused") {
			return;
		}
		void router.navigate({ to: "/account/paused", replace: true });
	}, [auth.status, location.pathname, router]);
	const demoWideDocked = useMediaQuery(
		`(min-width: ${DEMO_INSPECTOR_DOCKED_BREAKPOINT_PX}px)`,
	);
	const demoActive = isDemoMode() || shouldPrepareDemoRuntime();
	const demoControlsHidden = demoSnapshot.shareState.controlsHidden;
	const showWideDockedLayout =
		demoActive &&
		!demoControlsHidden &&
		demoWideDocked &&
		!demoSnapshot.panelLayout.collapsed;

	if (auth.isBootstrapping) {
		return <AppBoot />;
	}
	if (auth.status === "paused" && location.pathname !== "/account/paused") {
		return <PausedAccountPage />;
	}

	return (
		<>
			<DemoRootFrame
				showWideDockedLayout={showWideDockedLayout}
				dockedInspector={
					showWideDockedLayout ? (
						<DemoInspectorMount desktopMode="docked-sidebar" />
					) : null
				}
			>
				<Outlet />
			</DemoRootFrame>
			{showWideDockedLayout || demoControlsHidden ? null : (
				<DemoInspectorMount />
			)}
		</>
	);
}

function RootRouteNotFoundComponent() {
	const auth = useAuthBootstrap();

	if (auth.isBootstrapping) {
		return <AppBoot />;
	}

	return (
		<NotFoundPage
			isAuthenticated={auth.isAuthenticated && Boolean(auth.me)}
			pathname={typeof window === "undefined" ? null : window.location.pathname}
		/>
	);
}

function DemoInspectorMount(props: {
	desktopMode?: "floating" | "docked-sidebar";
}) {
	if (!isDemoMode()) {
		return null;
	}

	return (
		<Suspense fallback={null}>
			<LazyDemoInspector desktopMode={props.desktopMode} />
		</Suspense>
	);
}

function DemoRootFrame(props: {
	children: ReactNode;
	showWideDockedLayout: boolean;
	dockedInspector: ReactNode;
}) {
	if (!props.showWideDockedLayout) {
		return <>{props.children}</>;
	}

	return (
		<div className="w-full min-h-dvh" data-demo-root-frame="wide">
			<div data-demo-root-sidebar="wide">{props.dockedInspector}</div>
			<div
				className="min-h-dvh min-w-0"
				data-demo-root-content="wide"
				style={
					{
						"--app-meta-footer-left": `${DEMO_INSPECTOR_DOCKED_CONTENT_OFFSET_PX}px`,
						"--app-meta-footer-right": `${DEMO_INSPECTOR_DOCKED_VIEWPORT_GUTTER_PX}px`,
						boxSizing: "border-box",
						paddingLeft: `${DEMO_INSPECTOR_DOCKED_CONTENT_OFFSET_PX}px`,
						paddingRight: `${DEMO_INSPECTOR_DOCKED_VIEWPORT_GUTTER_PX}px`,
					} as CSSProperties
				}
			>
				<div
					className="mx-auto w-full min-w-0"
					style={{
						maxWidth: `${DEMO_APP_MAX_FRAME_WIDTH_PX}px`,
					}}
				>
					{props.children}
				</div>
			</div>
		</div>
	);
}
