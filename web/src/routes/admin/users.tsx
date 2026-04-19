import { createFileRoute } from "@tanstack/react-router";

import { readAdminUsersWarmSnapshot } from "@/auth/startupCache";
import { AdminPanel } from "@/pages/AdminPanel";

import { useRequiredAdmin } from "../-adminGuard";

export const Route = createFileRoute("/admin/users")({
	component: AdminUsersRouteComponent,
});

function AdminUsersRouteComponent() {
	const me = useRequiredAdmin();

	if (!me) {
		return null;
	}

	const warmStart = readAdminUsersWarmSnapshot({
		userId: me.user.id,
	});

	return <AdminPanel me={me} userManagementWarmStart={warmStart} />;
}
