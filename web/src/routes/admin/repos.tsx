import { createFileRoute } from "@tanstack/react-router";
import { AdminRoutePending } from "./-pending";

export const Route = createFileRoute("/admin/repos")({
	pendingMs: 0,
	pendingMinMs: 200,
	pendingComponent: AdminReposRoutePendingComponent,
});

function AdminReposRoutePendingComponent() {
	return <AdminRoutePending variant="dashboard" />;
}
