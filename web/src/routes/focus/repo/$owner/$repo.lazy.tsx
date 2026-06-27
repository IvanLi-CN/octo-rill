import { createLazyFileRoute, Outlet } from "@tanstack/react-router";

export const Route = createLazyFileRoute("/focus/repo/$owner/$repo")({
	component: Outlet,
});
