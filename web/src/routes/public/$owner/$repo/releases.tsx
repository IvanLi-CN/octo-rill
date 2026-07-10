import { createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/public/$owner/$repo/releases")({
	beforeLoad: ({ location, params }) => {
		const requestedPath = location.pathname.replace(/\/+$/, "");
		const legacyListPath = `/public/${params.owner}/${params.repo}/releases`;
		if (requestedPath !== legacyListPath) {
			return;
		}
		throw redirect({
			to: "/$owner/$repo/releases",
			params: {
				owner: params.owner,
				repo: params.repo,
			},
			search: Object.fromEntries(new URLSearchParams(location.search)),
			replace: true,
		});
	},
});
