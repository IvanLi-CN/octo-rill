import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ThemeProvider } from "@/theme/ThemeProvider";
import { VersionMonitorProvider } from "@/version/versionMonitor";
import "./index.css";
import App from "./App.tsx";

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<ThemeProvider>
			<TooltipProvider>
				<VersionMonitorProvider>
					<App />
				</VersionMonitorProvider>
			</TooltipProvider>
		</ThemeProvider>
	</StrictMode>,
);
