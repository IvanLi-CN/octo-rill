import { createFileRoute } from "@tanstack/react-router";
import { validateAdminJobsSearch } from "@/admin/jobsRouteState";

import { AdminRoutePending } from "../../-pending";

export const Route = createFileRoute("/admin/jobs/subscriptions/$taskId")({
	validateSearch: validateAdminJobsSearch,
	pendingMs: 0,
	pendingMinMs: 200,
	pendingComponent: AdminJobsSubscriptionDetailPendingComponent,
});

function AdminJobsSubscriptionDetailPendingComponent() {
	return <AdminRoutePending variant="jobs" />;
}
