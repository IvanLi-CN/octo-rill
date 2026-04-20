import { createFileRoute } from "@tanstack/react-router";
import { AdminRoutePending } from "./-pending";

export const Route = createFileRoute("/admin/users")({
	pendingMs: 0,
	pendingMinMs: 200,
	pendingComponent: AdminUsersRoutePendingComponent,
});

function AdminUsersRoutePendingComponent() {
	return <AdminRoutePending variant="users" />;
}
