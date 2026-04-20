import { Landing } from "@/pages/Landing";

export default function LandingRouteSurface(props: {
	bootError: string | null;
}) {
	return <Landing {...props} />;
}
