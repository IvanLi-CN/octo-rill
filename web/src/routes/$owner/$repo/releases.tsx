import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/$owner/$repo/releases")({
	pendingMs: 0,
	pendingMinMs: 200,
});
