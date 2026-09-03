import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { AdminJobsRoutePage } from "../../-helpers";

const routeApi = getRouteApi("/admin/jobs/ai-records/$kind/$recordId");

export const Route = createLazyFileRoute(
	"/admin/jobs/ai-records/$kind/$recordId",
)({
	component: AdminJobsAiRecordDetailRouteComponent,
});

function AdminJobsAiRecordDetailRouteComponent() {
	const search = routeApi.useSearch();
	const params = routeApi.useParams();
	const kind =
		params.kind === "announcement" || params.kind === "brief"
			? params.kind
			: "release";

	return (
		<AdminJobsRoutePage
			primaryTab="ai_records"
			search={search}
			aiRecordKind={kind}
			aiRecordId={params.recordId}
			aiRecordAttemptId={search.ai_attempt}
			aiRecordLlmCallId={search.ai_llm}
		/>
	);
}
