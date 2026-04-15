import { createFileRoute } from "@tanstack/react-router";

import { AdminJobsRoutePage, validateAdminJobsSearch } from "./-helpers";

export const Route = createFileRoute("/admin/jobs/translations")({
	component: AdminJobsTranslationsRouteComponent,
	validateSearch: validateAdminJobsSearch,
});

function AdminJobsTranslationsRouteComponent() {
	const search = Route.useSearch();

	return <AdminJobsRoutePage primaryTab="translations" search={search} />;
}
