import { createFileRoute } from "@tanstack/react-router";

import { validatePublicReleaseSearch } from "@/publicRelease/routeState";

export const Route = createFileRoute("/public/$owner/$repo/releases/tag/$tag")({
	validateSearch: validatePublicReleaseSearch,
	pendingMs: 0,
	pendingMinMs: 200,
});
