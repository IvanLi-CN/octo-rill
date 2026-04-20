import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { AdminJobsRoutePage } from "./-helpers";

const routeApi = getRouteApi("/admin/jobs/scheduled");

export const Route = createLazyFileRoute("/admin/jobs/scheduled")({
	component: AdminJobsScheduledRouteComponent,
});

function AdminJobsScheduledRouteComponent() {
	const search = routeApi.useSearch();

	return <AdminJobsRoutePage primaryTab="scheduled" search={search} />;
}
