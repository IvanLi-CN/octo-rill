import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { AdminJobsRoutePage } from "./-helpers";

const routeApi = getRouteApi("/admin/jobs/user-sync");

export const Route = createLazyFileRoute("/admin/jobs/user-sync")({
	component: AdminJobsUserSyncRouteComponent,
});

function AdminJobsUserSyncRouteComponent() {
	const search = routeApi.useSearch();

	return <AdminJobsRoutePage primaryTab="user_sync" search={search} />;
}
