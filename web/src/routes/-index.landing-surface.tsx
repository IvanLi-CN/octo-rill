import type { NetworkErrorKind } from "@/lib/errorPresentation";
import { Landing } from "@/pages/Landing";

export default function LandingRouteSurface(props: {
	bootError: string | null;
	bootErrorKind: NetworkErrorKind | null;
	bootErrorDetail: string | null;
	onRetryBoot?: () => void;
}) {
	return <Landing {...props} />;
}
