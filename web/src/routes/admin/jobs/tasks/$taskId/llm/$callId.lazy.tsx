import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { AdminJobsRoutePage } from "../../../-helpers";

const routeApi = getRouteApi("/admin/jobs/tasks/$taskId/llm/$callId");

export const Route = createLazyFileRoute(
	"/admin/jobs/tasks/$taskId/llm/$callId",
)({
	component: AdminJobsTaskLlmDrawerRouteComponent,
});

function AdminJobsTaskLlmDrawerRouteComponent() {
	const search = routeApi.useSearch();
	const params = routeApi.useParams();

	return (
		<AdminJobsRoutePage
			primaryTab="realtime"
			search={search}
			taskId={params.taskId}
			llmCallId={params.callId}
		/>
	);
}
