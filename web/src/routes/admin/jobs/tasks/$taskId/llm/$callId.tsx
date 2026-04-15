import { createFileRoute } from "@tanstack/react-router";

import { AdminJobsRoutePage, validateAdminJobsSearch } from "../../../-helpers";

export const Route = createFileRoute("/admin/jobs/tasks/$taskId/llm/$callId")({
	component: AdminJobsTaskLlmDrawerRouteComponent,
	validateSearch: validateAdminJobsSearch,
});

function AdminJobsTaskLlmDrawerRouteComponent() {
	const search = Route.useSearch();
	const params = Route.useParams();

	return (
		<AdminJobsRoutePage
			primaryTab="realtime"
			search={search}
			taskId={params.taskId}
			llmCallId={params.callId}
		/>
	);
}
