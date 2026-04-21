import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { createFileRoute } from "@tanstack/react-router";
import { AppBoot, SettingsStartupSkeleton } from "@/pages/AppBoot";
import { normalizeSettingsSection } from "@/settings/routeState";

export const Route = createFileRoute("/settings")({
	validateSearch: (search: Record<string, unknown>) => ({
		section:
			typeof search.section === "string"
				? normalizeSettingsSection(search.section)
				: undefined,
		linuxdo: typeof search.linuxdo === "string" ? search.linuxdo : undefined,
		github: typeof search.github === "string" ? search.github : undefined,
	}),
	pendingMs: 0,
	pendingMinMs: 200,
	pendingComponent: SettingsRoutePendingComponent,
});

function SettingsRoutePendingComponent() {
	const auth = useAuthBootstrap();
	if (auth.isAuthenticated && auth.me) {
		return <SettingsStartupSkeleton me={auth.me} />;
	}

	return <AppBoot />;
}
