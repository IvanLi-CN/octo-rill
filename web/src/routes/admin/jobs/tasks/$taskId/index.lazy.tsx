import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { AdminJobsRoutePage } from "../../-helpers";

const routeApi = getRouteApi("/admin/jobs/tasks/$taskId/");

export const Route = createLazyFileRoute("/admin/jobs/tasks/$taskId/")({
	component: AdminJobsTaskDrawerRouteComponent,
});

function AdminJobsTaskDrawerRouteComponent() {
	const search = routeApi.useSearch();
	const params = routeApi.useParams();

	return (
		<AdminJobsRoutePage
			primaryTab="realtime"
			search={search}
			taskId={params.taskId}
		/>
	);
}
