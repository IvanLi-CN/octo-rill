import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { TooltipProvider } from "@/components/ui/tooltip";
import { VersionMonitorProvider } from "@/version/versionMonitor";
import "./index.css";
import App from "./App.tsx";

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<TooltipProvider>
			<VersionMonitorProvider>
				<App />
			</VersionMonitorProvider>
		</TooltipProvider>
	</StrictMode>,
);
