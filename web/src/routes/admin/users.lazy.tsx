import { createLazyFileRoute } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { readAdminUsersWarmSnapshot } from "@/auth/startupCache";
import { AppBoot } from "@/pages/AppBoot";
import { AdminPanel } from "@/pages/AdminPanel";

import { useRequiredAdmin } from "../-adminGuard";

export const Route = createLazyFileRoute("/admin/users")({
	component: AdminUsersRouteComponent,
});

function AdminUsersRouteComponent() {
	const auth = useAuthBootstrap();
	const me = useRequiredAdmin();

	if (auth.isBootstrapping && auth.bootPresentation !== "live" && !me) {
		return <AppBoot />;
	}

	if (!me) {
		return null;
	}

	const warmStart = readAdminUsersWarmSnapshot({
		userId: me.user.id,
	});

	return <AdminPanel me={me} userManagementWarmStart={warmStart} />;
}
