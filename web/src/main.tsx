import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppToastProvider } from "@/components/feedback/AppToast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ThemeProvider } from "@/theme/ThemeProvider";
import { VersionMonitorProvider } from "@/version/versionMonitor";
import "./index.css";
import App from "./App.tsx";

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<ThemeProvider>
			<TooltipProvider>
				<AppToastProvider>
					<VersionMonitorProvider>
						<App />
					</VersionMonitorProvider>
				</AppToastProvider>
			</TooltipProvider>
		</ThemeProvider>
	</StrictMode>,
);
