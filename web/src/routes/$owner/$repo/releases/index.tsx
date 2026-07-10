import { createFileRoute } from "@tanstack/react-router";

import { validatePublicReleaseSearch } from "@/publicRelease/routeState";

export const Route = createFileRoute("/$owner/$repo/releases/")({
	validateSearch: validatePublicReleaseSearch,
	pendingMs: 0,
	pendingMinMs: 200,
});
