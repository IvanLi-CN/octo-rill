import { createLazyFileRoute } from "@tanstack/react-router";

import { PausedAccountPage } from "@/pages/PausedAccount";

export const Route = createLazyFileRoute("/account/paused")({
	component: PausedAccountPage,
});
