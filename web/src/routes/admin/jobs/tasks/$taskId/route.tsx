import { createFileRoute, Outlet } from "@tanstack/react-router";

export const Route = createFileRoute("/admin/jobs/tasks/$taskId")({
	component: Outlet,
});
