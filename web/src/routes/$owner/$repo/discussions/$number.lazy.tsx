import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "../../../-dashboardRoute";

const routeApi = getRouteApi("/$owner/$repo/discussions/$number");

export const Route = createLazyFileRoute("/$owner/$repo/discussions/$number")({
	component: DashboardDiscussionRouteComponent,
});

function DashboardDiscussionRouteComponent() {
	const params = routeApi.useParams();
	const search = routeApi.useSearch() as {
		from?: unknown;
		scope?: unknown;
		items?: unknown;
		org?: unknown;
	};
	const from = typeof search.from === "string" ? search.from : null;
	const scope = typeof search.scope === "string" ? search.scope : null;
	const items = typeof search.items === "string" ? search.items : null;
	const org = typeof search.org === "string" ? search.org : null;

	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({
				search: { from, scope, items, org },
				owner: params.owner,
				repo: params.repo,
				number: params.number,
			})}
		/>
	);
}
