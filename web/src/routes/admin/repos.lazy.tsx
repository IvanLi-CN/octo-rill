import { createLazyFileRoute } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { AppBoot } from "@/pages/AppBoot";
import { AdminReposPage } from "@/pages/AdminReposPage";

import { useRequiredAdmin } from "../-adminGuard";

export const Route = createLazyFileRoute("/admin/repos")({
	component: AdminReposRouteComponent,
});

function AdminReposRouteComponent() {
	const auth = useAuthBootstrap();
	const me = useRequiredAdmin();

	if (auth.isBootstrapping && auth.bootPresentation !== "live" && !me) {
		return <AppBoot />;
	}

	if (!me) {
		return null;
	}

	return <AdminReposPage me={me} />;
}
