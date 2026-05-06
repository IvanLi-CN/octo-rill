import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { parseDashboardRouteState } from "@/dashboard/routeState";
import { AppBoot } from "@/pages/AppBoot";
import { PublicReleasePage } from "@/pages/PublicReleasePage";
import { DashboardRouteShell } from "../../../../-dashboardRoute";

const routeApi = getRouteApi("/$owner/$repo/releases/tag/$tag");

export const Route = createLazyFileRoute("/$owner/$repo/releases/tag/$tag")({
	component: DashboardReleaseRouteComponent,
});

function DashboardReleaseRouteComponent() {
	const auth = useAuthBootstrap();
	const params = routeApi.useParams();
	const search = routeApi.useSearch() as { from?: unknown };
	const from = typeof search.from === "string" ? search.from : null;
	if (auth.status === "pending") {
		return <AppBoot />;
	}
	if (auth.isAuthenticated && auth.me) {
		return (
			<DashboardRouteShell
				routeState={parseDashboardRouteState({
					owner: params.owner,
					repo: params.repo,
					tag: params.tag,
					from,
				})}
			/>
		);
	}
	return (
		<PublicReleasePage
			owner={params.owner}
			repo={params.repo}
			tag={params.tag}
		/>
	);
}
