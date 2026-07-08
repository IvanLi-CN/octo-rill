import {
	buildCurrentDemoHref,
	getDemoSnapshot,
	isDemoMode,
} from "@/demo/runtime";

type DemoAuthIntent = "login" | "connect" | "logout";

export function resolveDemoSafeAuthHref(
	fallbackHref: string,
	intent: DemoAuthIntent,
) {
	if (!isDemoMode()) {
		return fallbackHref;
	}

	if (intent === "logout") {
		return buildCurrentDemoHref({
			sceneId: "landing-welcome",
			personaId: "guest",
			includeOwnReleases: false,
			publicationState: "unpublished",
		});
	}

	const snapshot = getDemoSnapshot();
	const personaId =
		snapshot.shareState.personaId === "guest"
			? "member"
			: snapshot.shareState.personaId;

	return buildCurrentDemoHref({ personaId });
}
