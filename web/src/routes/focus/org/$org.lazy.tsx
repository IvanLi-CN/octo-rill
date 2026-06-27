import { createLazyFileRoute, Outlet } from "@tanstack/react-router";

export const Route = createLazyFileRoute("/focus/org/$org")({
	component: Outlet,
});
