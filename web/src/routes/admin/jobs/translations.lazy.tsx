import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { AdminJobsRoutePage } from "./-helpers";

const routeApi = getRouteApi("/admin/jobs/translations");

export const Route = createLazyFileRoute("/admin/jobs/translations")({
	component: AdminJobsTranslationsRouteComponent,
});

function AdminJobsTranslationsRouteComponent() {
	const search = routeApi.useSearch();

	return <AdminJobsRoutePage primaryTab="translations" search={search} />;
}
