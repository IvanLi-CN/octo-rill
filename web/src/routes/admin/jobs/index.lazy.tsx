import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { AdminJobsRoutePage } from "./-helpers";

const routeApi = getRouteApi("/admin/jobs/");

export const Route = createLazyFileRoute("/admin/jobs/")({
	component: AdminJobsRealtimeRouteComponent,
});

function AdminJobsRealtimeRouteComponent() {
	const search = routeApi.useSearch();

	return <AdminJobsRoutePage primaryTab="realtime" search={search} />;
}
