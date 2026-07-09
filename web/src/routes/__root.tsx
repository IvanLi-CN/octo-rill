import { lazy, Suspense } from "react";
import { createRootRoute, Outlet } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { isDemoMode } from "@/demo/runtime";
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
			<Outlet />
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
