import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/public/$owner/$repo/releases/tag/$tag")({
	pendingMs: 0,
	pendingMinMs: 200,
});
