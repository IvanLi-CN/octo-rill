import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { BindGitHubPage } from "@/pages/BindGitHub";

const routeApi = getRouteApi("/bind/github");

export const Route = createLazyFileRoute("/bind/github")({
	component: BindGitHubRoute,
});

function BindGitHubRoute() {
	const search = routeApi.useSearch();
	return (
		<BindGitHubPage
			linuxdoStatus={search.linuxdo}
			passkeyStatus={search.passkey}
		/>
	);
}
