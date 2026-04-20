import { createRootRoute, Outlet } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { AppBoot } from "@/pages/AppBoot";
import { NotFoundPage } from "@/pages/NotFound";

export const Route = createRootRoute({
	component: RootRouteComponent,
	notFoundComponent: RootRouteNotFoundComponent,
});

function RootRouteComponent() {
	const auth = useAuthBootstrap();

	if (auth.isBootstrapping && auth.bootPresentation === "cold-init") {
		return <AppBoot />;
	}

	return <Outlet />;
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
