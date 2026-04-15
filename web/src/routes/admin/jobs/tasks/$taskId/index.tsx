import { createFileRoute } from "@tanstack/react-router";

import { AdminJobsRoutePage, validateAdminJobsSearch } from "../../-helpers";

export const Route = createFileRoute("/admin/jobs/tasks/$taskId/")({
	component: AdminJobsTaskDrawerRouteComponent,
	validateSearch: validateAdminJobsSearch,
});

function AdminJobsTaskDrawerRouteComponent() {
	const search = Route.useSearch();
	const params = Route.useParams();

	return (
		<AdminJobsRoutePage
			primaryTab="realtime"
			search={search}
			taskId={params.taskId}
		/>
	);
}
