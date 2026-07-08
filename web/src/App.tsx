import { RouterProvider } from "@tanstack/react-router";

import { AuthBootstrapProvider } from "@/auth/AuthBootstrap";
import { DemoBootstrapBoundary } from "@/demo/runtime";
import { router } from "@/router";

function App() {
	return (
		<AuthBootstrapProvider>
			<DemoBootstrapBoundary>
				<RouterProvider router={router} />
			</DemoBootstrapBoundary>
		</AuthBootstrapProvider>
	);
}

export default App;
