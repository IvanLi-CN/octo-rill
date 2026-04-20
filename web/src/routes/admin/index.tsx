import { createFileRoute } from "@tanstack/react-router";
import { AdminRoutePending } from "./-pending";

export const Route = createFileRoute("/admin/")({
	pendingMs: 0,
	pendingMinMs: 200,
	pendingComponent: AdminDashboardRoutePendingComponent,
});

function AdminDashboardRoutePendingComponent() {
	return <AdminRoutePending variant="dashboard" />;
}
