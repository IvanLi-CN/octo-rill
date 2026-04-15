import { createFileRoute } from "@tanstack/react-router";

import { AdminJobsRoutePage, validateAdminJobsSearch } from "./-helpers";

export const Route = createFileRoute("/admin/jobs/")({
	component: AdminJobsRealtimeRouteComponent,
	validateSearch: validateAdminJobsSearch,
});

function AdminJobsRealtimeRouteComponent() {
	const search = Route.useSearch();

	return <AdminJobsRoutePage primaryTab="realtime" search={search} />;
}
