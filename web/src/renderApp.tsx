import { StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";

import { AppToastProvider } from "@/components/feedback/AppToast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AppQueryProvider } from "@/query/queryClient";
import { ThemeProvider } from "@/theme/ThemeProvider";
import { VersionMonitorProvider } from "@/version/versionMonitor";
import App from "./App";

let root: Root | null = null;

export function renderApp(container: HTMLElement) {
	root ??= createRoot(container);
	root.render(
		<StrictMode>
			<AppQueryProvider>
				<ThemeProvider>
					<TooltipProvider>
						<AppToastProvider>
							<VersionMonitorProvider>
								<App />
							</VersionMonitorProvider>
						</AppToastProvider>
					</TooltipProvider>
				</ThemeProvider>
			</AppQueryProvider>
		</StrictMode>,
	);
}
