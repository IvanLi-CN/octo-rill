import { createLazyFileRoute } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { AdminPublicReleasesPage } from "@/pages/AdminPublicReleasesPage";
import { AdminDashboardStartupSkeleton, AppBoot } from "@/pages/AppBoot";

import { useRequiredAdmin } from "../-adminGuard";

export const Route = createLazyFileRoute("/admin/public-releases")({
	component: AdminPublicReleasesRouteComponent,
});

function AdminPublicReleasesRouteComponent() {
	const auth = useAuthBootstrap();
	const me = useRequiredAdmin();

	if (auth.isBootstrapping && auth.bootPresentation !== "live" && !me) {
		return <AppBoot />;
	}

	if (!me) {
		return null;
	}

	if (auth.isBootstrapping && auth.bootPresentation !== "live") {
		return <AdminDashboardStartupSkeleton me={me} />;
	}

	return <AdminPublicReleasesPage me={me} />;
}
