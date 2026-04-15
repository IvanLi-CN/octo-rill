import { createFileRoute } from "@tanstack/react-router";

import { AdminJobsRoutePage, validateAdminJobsSearch } from "./-helpers";

export const Route = createFileRoute("/admin/jobs/llm")({
	component: AdminJobsLlmRouteComponent,
	validateSearch: validateAdminJobsSearch,
});

function AdminJobsLlmRouteComponent() {
	const search = Route.useSearch();

	return <AdminJobsRoutePage primaryTab="llm" search={search} />;
}
