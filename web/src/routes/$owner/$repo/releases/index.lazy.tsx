import { createLazyFileRoute } from "@tanstack/react-router";

export const Route = createLazyFileRoute("/$owner/$repo/releases/")({
	component: () => null,
});
