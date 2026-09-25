import { createLazyFileRoute } from "@tanstack/react-router";

export const Route = createLazyFileRoute(
	"/public/$owner/$repo/releases/tag/$tag",
)({
	component: () => null,
});
