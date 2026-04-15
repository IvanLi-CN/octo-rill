import { RouterProvider } from "@tanstack/react-router";

import { AuthBootstrapProvider } from "@/auth/AuthBootstrap";
import { router } from "@/router";

function App() {
	return (
		<AuthBootstrapProvider>
			<RouterProvider router={router} />
		</AuthBootstrapProvider>
	);
}

export default App;
