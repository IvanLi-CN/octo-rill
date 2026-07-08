import { prepareDemoRuntime, shouldPrepareDemoRuntime } from "@/demo/runtime";
import "./index.css";

async function renderApp() {
	const { renderApp } = await import("./renderApp");
	renderApp(document.getElementById("root")!);
}

if (shouldPrepareDemoRuntime()) {
	void prepareDemoRuntime().then(renderApp);
} else {
	void renderApp();
}
