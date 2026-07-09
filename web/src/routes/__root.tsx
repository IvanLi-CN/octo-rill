import { type CSSProperties, lazy, type ReactNode, Suspense } from "react";
import { createRootRoute, Outlet } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import {
	DEMO_INSPECTOR_DOCKED_BREAKPOINT_PX,
	DEMO_INSPECTOR_DOCKED_CONTENT_OFFSET_PX,
	DEMO_INSPECTOR_DOCKED_LAYOUT_WIDTH_PX,
} from "@/demo/layout";
import { isDemoMode, shouldPrepareDemoRuntime } from "@/demo/runtime";
import { useMediaQuery } from "@/lib/useMediaQuery";
import { AppBoot } from "@/pages/AppBoot";
import { NotFoundPage } from "@/pages/NotFound";

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

	if (auth.isBootstrapping && auth.bootPresentation === "cold-init") {
		return <AppBoot />;
	}

	return (
		<>
			<DemoRootFrame>
				<Outlet />
			</DemoRootFrame>
			<DemoInspectorMount />
		</>
	);
}

function RootRouteNotFoundComponent() {
	const auth = useAuthBootstrap();

	if (auth.isBootstrapping && auth.bootPresentation === "cold-init") {
		return <AppBoot />;
	}

	return (
		<NotFoundPage
			isAuthenticated={auth.isAuthenticated && Boolean(auth.me)}
			pathname={typeof window === "undefined" ? null : window.location.pathname}
		/>
	);
}

function DemoInspectorMount() {
	if (!isDemoMode()) {
		return null;
	}

	return (
		<Suspense fallback={null}>
			<LazyDemoInspector />
		</Suspense>
	);
}

function DemoRootFrame(props: { children: ReactNode }) {
	const demoWideDocked = useMediaQuery(
		`(min-width: ${DEMO_INSPECTOR_DOCKED_BREAKPOINT_PX}px)`,
	);
	const demoActive = isDemoMode() || shouldPrepareDemoRuntime();

	if (!demoActive || !demoWideDocked) {
		return <>{props.children}</>;
	}

	return (
		<div
			className="mx-auto w-full min-w-0"
			data-demo-root-frame="wide"
			style={
				{
					boxSizing: "border-box",
					maxWidth: `${DEMO_INSPECTOR_DOCKED_LAYOUT_WIDTH_PX}px`,
					paddingLeft: `${DEMO_INSPECTOR_DOCKED_CONTENT_OFFSET_PX}px`,
				} as CSSProperties
			}
		>
			{props.children}
		</div>
	);
}
