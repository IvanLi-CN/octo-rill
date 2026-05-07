import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { PublicReleasePage } from "@/pages/PublicReleasePage";

const routeApi = getRouteApi("/public/$owner/$repo/releases/tag/$tag");

export const Route = createLazyFileRoute(
	"/public/$owner/$repo/releases/tag/$tag",
)({
	component: PublicReleaseDetailRouteComponent,
});

function PublicReleaseDetailRouteComponent() {
	const params = routeApi.useParams();
	return (
		<PublicReleasePage
			owner={params.owner}
			repo={params.repo}
			tag={params.tag}
		/>
	);
}
