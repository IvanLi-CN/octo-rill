import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/$owner/$repo/discussions/$number")({
	pendingMs: 0,
	pendingMinMs: 200,
});
