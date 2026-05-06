import { createFileRoute } from "@tanstack/react-router";
import { AdminRoutePending } from "./-pending";

export const Route = createFileRoute("/admin/public-releases")({
	pendingMs: 0,
	pendingMinMs: 200,
	pendingComponent: AdminPublicReleasesRoutePendingComponent,
});

function AdminPublicReleasesRoutePendingComponent() {
	return <AdminRoutePending variant="dashboard" />;
}
