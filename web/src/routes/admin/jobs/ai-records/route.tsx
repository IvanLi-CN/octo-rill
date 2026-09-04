import { createFileRoute, Outlet } from "@tanstack/react-router";

export const Route = createFileRoute("/admin/jobs/ai-records")({
	component: Outlet,
});
