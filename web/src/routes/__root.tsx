import { createRootRoute, Outlet } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { AppBoot } from "@/pages/AppBoot";

export const Route = createRootRoute({
	component: RootRouteComponent,
});

function RootRouteComponent() {
	const auth = useAuthBootstrap();

	if (auth.isBootstrapping && auth.bootPresentation === "cold-init") {
		return <AppBoot />;
	}

	return <Outlet />;
}
