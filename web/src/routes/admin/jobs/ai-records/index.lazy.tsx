import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { AdminJobsRoutePage } from "../-helpers";

const routeApi = getRouteApi("/admin/jobs/ai-records/");

export const Route = createLazyFileRoute("/admin/jobs/ai-records/")({
	component: AdminJobsAiRecordsRouteComponent,
});

function AdminJobsAiRecordsRouteComponent() {
	const search = routeApi.useSearch();

	return <AdminJobsRoutePage primaryTab="ai_records" search={search} />;
}
