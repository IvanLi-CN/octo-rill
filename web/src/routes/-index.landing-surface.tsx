import { applyDemoShareStateInPlace, useDemoSnapshot } from "@/demo/runtime";
import type { DemoShareState } from "@/demo/types";
import type { NetworkErrorKind } from "@/lib/errorPresentation";
import { Landing, type LandingAuthAction } from "@/pages/Landing";

function resolveLandingDemoState(
	shareState: Pick<
		DemoShareState,
		| "landingCase"
		| "landingAuthAction"
		| "landingPasskeySupport"
		| "landingBootState"
	>,
) {
	const authNetworkUnavailable =
		shareState.landingBootState === "network-unavailable";
	const passkeyUnsupported = shareState.landingPasskeySupport === "unsupported";
	const passkeyAction =
		shareState.landingAuthAction === "passkey-authenticate" ||
		shareState.landingAuthAction === "passkey-register";
	const previewAuthAction =
		authNetworkUnavailable || (passkeyUnsupported && passkeyAction)
			? null
			: shareState.landingAuthAction === "idle"
				? null
				: (shareState.landingAuthAction as LandingAuthAction);
	const previewAuthStateKey = [
		shareState.landingCase,
		shareState.landingAuthAction,
		shareState.landingPasskeySupport,
		shareState.landingBootState,
	].join(":");

	return {
		passkeySupportOverride: !passkeyUnsupported,
		previewAuthAction,
		previewAuthStateKey,
		...(authNetworkUnavailable
			? {
					bootError: "演示：网络连接不可用。请恢复网络后重试登录。",
					bootErrorKind: "network" as const,
					bootErrorDetail:
						"Demo 场景控制：认证入口已禁用，真实 OAuth 链路未被调用。",
				}
			: {}),
	};
}

export default function LandingRouteSurface(props: {
	bootError: string | null;
	bootErrorKind: NetworkErrorKind | null;
	bootErrorDetail: string | null;
	onRetryBoot?: () => void;
}) {
	const demoSnapshot = useDemoSnapshot();
	const demoLandingState =
		demoSnapshot.active && demoSnapshot.shareState.sceneId === "landing-welcome"
			? resolveLandingDemoState(demoSnapshot.shareState)
			: null;

	return (
		<Landing
			{...props}
			{...demoLandingState}
			onAuthNavigate={
				demoLandingState
					? (_href, action) =>
							applyDemoShareStateInPlace(
								{
									landingCase: "custom",
									landingAuthAction: action,
								},
								{ reseed: false },
							)
					: undefined
			}
		/>
	);
}
