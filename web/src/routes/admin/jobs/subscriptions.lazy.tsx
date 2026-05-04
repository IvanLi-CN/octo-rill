import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { AdminJobsRoutePage } from "./-helpers";

const routeApi = getRouteApi("/admin/jobs/subscriptions");

export const Route = createLazyFileRoute("/admin/jobs/subscriptions")({
	component: AdminJobsSubscriptionsRouteComponent,
});

function AdminJobsSubscriptionsRouteComponent() {
	const search = routeApi.useSearch();
	const detailMatch = window.location.pathname.match(
		/^\/admin\/jobs\/subscriptions\/([^/]+?)$/,
	);

	return (
		<AdminJobsRoutePage
			primaryTab="subscriptions"
			search={search}
			subscriptionDetailTaskId={
				detailMatch ? decodeURIComponent(detailMatch[1] ?? "") : undefined
			}
		/>
	);
}
