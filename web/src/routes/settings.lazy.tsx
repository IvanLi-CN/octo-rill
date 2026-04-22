import { useEffect } from "react";
import {
	createLazyFileRoute,
	getRouteApi,
	useRouter,
} from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { Landing } from "@/pages/Landing";
import { AppBoot, SettingsStartupSkeleton } from "@/pages/AppBoot";
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
			github: search.github,
			passkey: search.passkey,
		});
		if (
			(search.section ?? undefined) === expectedSearch.section &&
			(search.linuxdo ?? undefined) === expectedSearch.linuxdo &&
			(search.github ?? undefined) === expectedSearch.github &&
			(search.passkey ?? undefined) === expectedSearch.passkey
		) {
			return;
		}
		void router.navigate({
			to: "/settings",
			search: expectedSearch as never,
			replace: true,
		});
	}, [
		router,
		search.github,
		search.linuxdo,
		search.passkey,
		search.section,
		section,
	]);

	if (auth.status === "pending") {
		return <AppBoot />;
	}

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
			githubStatus={search.github}
			linuxdoStatus={search.linuxdo}
			passkeyStatus={search.passkey}
			onSectionChange={(nextSection) => {
				void router.navigate({
					to: "/settings",
					search: buildSettingsSearch(nextSection, {
						passkey: search.passkey,
					}) as never,
				});
			}}
			onProfileSaved={async () => {
				await auth.refreshAuth();
			}}
		/>
	);
}
