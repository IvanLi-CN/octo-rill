import { createFileRoute } from "@tanstack/react-router";

import { AdminJobsRoutePage, validateAdminJobsSearch } from "./-helpers";

export const Route = createFileRoute("/admin/jobs/scheduled")({
	component: AdminJobsScheduledRouteComponent,
	validateSearch: validateAdminJobsSearch,
});

function AdminJobsScheduledRouteComponent() {
	const search = Route.useSearch();

	return <AdminJobsRoutePage primaryTab="scheduled" search={search} />;
}
