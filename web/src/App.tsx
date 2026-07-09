import { RouterProvider } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { AuthBootstrapProvider } from "@/auth/AuthBootstrap";
import { DemoBootstrapBoundary, useDemoSnapshot } from "@/demo/runtime";
import { router } from "@/router";

function App() {
	return (
		<DemoBootstrapBoundary>
			<DemoAppTree>
				<RouterProvider router={router} />
			</DemoAppTree>
		</DemoBootstrapBoundary>
	);
}

function DemoAppTree(props: { children: ReactNode }) {
	const { children } = props;
	const snapshot = useDemoSnapshot();
	const appKey = snapshot.active ? `demo-${snapshot.revision}` : "live";

	return <AuthBootstrapProvider key={appKey}>{children}</AuthBootstrapProvider>;
}

export default App;
