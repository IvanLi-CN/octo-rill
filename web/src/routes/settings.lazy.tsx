import { useEffect } from "react";
import {
	createLazyFileRoute,
	getRouteApi,
	useRouter,
} from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { Landing } from "@/pages/Landing";
import { SettingsStartupSkeleton } from "@/pages/AppBoot";
import { SettingsPage } from "@/pages/Settings";
import {
	buildSettingsSearch,
	normalizeSettingsSection,
} from "@/settings/routeState";

const routeApi = getRouteApi("/settings");

export const Route = createLazyFileRoute("/settings")({
	component: SettingsRouteComponent,
});

function SettingsRouteComponent() {
	const auth = useAuthBootstrap();
	const router = useRouter();
	const search = routeApi.useSearch();
	const section = normalizeSettingsSection(search.section);

	useEffect(() => {
		const expectedSearch = buildSettingsSearch(section, {
			linuxdo: search.linuxdo,
		});
		if (
			(search.section ?? undefined) === expectedSearch.section &&
			(search.linuxdo ?? undefined) === expectedSearch.linuxdo
		) {
			return;
		}
		void router.navigate({
			to: "/settings",
			search: expectedSearch as never,
			replace: true,
		});
	}, [router, search.linuxdo, search.section, section]);

	if (!auth.isAuthenticated || !auth.me) {
		return <Landing bootError={auth.bootError} />;
	}

	if (auth.isBootstrapping && auth.bootPresentation !== "live") {
		return <SettingsStartupSkeleton me={auth.me} />;
	}

	return (
		<SettingsPage
			me={auth.me}
			section={section}
			linuxdoStatus={search.linuxdo}
			onSectionChange={(nextSection) => {
				void router.navigate({
					to: "/settings",
					search: buildSettingsSearch(nextSection) as never,
				});
			}}
			onProfileSaved={async () => {
				await auth.refreshAuth();
			}}
		/>
	);
}
