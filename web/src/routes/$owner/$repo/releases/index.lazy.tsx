import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { PublicReleasePage } from "@/pages/PublicReleasePage";

const routeApi = getRouteApi("/$owner/$repo/releases/");

export const Route = createLazyFileRoute("/$owner/$repo/releases/")({
	component: PublicReleaseListRouteComponent,
});

function PublicReleaseListRouteComponent() {
	const params = routeApi.useParams();
	return <PublicReleasePage owner={params.owner} repo={params.repo} />;
}
