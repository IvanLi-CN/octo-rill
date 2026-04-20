import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { AdminJobsRoutePage } from "./-helpers";

const routeApi = getRouteApi("/admin/jobs/llm");

export const Route = createLazyFileRoute("/admin/jobs/llm")({
	component: AdminJobsLlmRouteComponent,
});

function AdminJobsLlmRouteComponent() {
	const search = routeApi.useSearch();

	return <AdminJobsRoutePage primaryTab="llm" search={search} />;
}
