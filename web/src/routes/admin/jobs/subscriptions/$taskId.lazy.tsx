import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { AdminJobsRoutePage } from "../-helpers";

const routeApi = getRouteApi("/admin/jobs/subscriptions/$taskId");

export const Route = createLazyFileRoute("/admin/jobs/subscriptions/$taskId")({
	component: AdminJobsSubscriptionDetailRouteComponent,
});

function AdminJobsSubscriptionDetailRouteComponent() {
	const search = routeApi.useSearch();
	const params = routeApi.useParams();

	return (
		<AdminJobsRoutePage
			primaryTab="subscriptions"
			search={search}
			subscriptionDetailTaskId={params.taskId}
		/>
	);
}
