import path from "node:path";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defineConfig } from "vite";
import {
	readCargoPackageVersion,
	resolveEmbeddedAppVersion,
} from "./config/embeddedVersion";

const repoRoot = path.resolve(__dirname, "..");
const embeddedAppVersion = resolveEmbeddedAppVersion(
	process.env.APP_EFFECTIVE_VERSION,
	readCargoPackageVersion(repoRoot),
);

function normalizeBase(base: string | undefined): string {
	const raw = (base ?? "/").trim();
	if (!raw || raw === "/") return "/";
	const withLeading = raw.startsWith("/") ? raw : `/${raw}`;
	return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}

function buildDemoBase(base: string | undefined): string {
	const normalized = normalizeBase(base);
	return normalized === "/" ? "/demo/" : `${normalized}demo/`;
}

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
	const isDemoBuild = mode === "demo";
	const demoBase = buildDemoBase(process.env.DOCS_BASE);
	const demoRouterBasepath = demoBase.endsWith("/")
		? demoBase.slice(0, -1)
		: demoBase;

	return {
		base: isDemoBuild ? demoBase : "/",
		build: {
			outDir: isDemoBuild ? "dist-demo" : "dist",
		},
		define: {
			__APP_LOADED_VERSION__: JSON.stringify(embeddedAppVersion),
			__OCTO_RILL_DEMO_APP__: JSON.stringify(isDemoBuild),
			__OCTO_RILL_ROUTER_BASEPATH__: JSON.stringify(
				isDemoBuild ? demoRouterBasepath : "/",
			),
		},
		plugins: [
			tanstackRouter({
				target: "react",
				autoCodeSplitting: true,
			}),
			react(),
			tailwindcss(),
		],
		resolve: {
			alias: {
				"@": path.resolve(__dirname, "./src"),
			},
		},
		server: {
			host: "127.0.0.1",
			port: 55174,
			strictPort: true,
			proxy: {
				"/api": "http://127.0.0.1:58090",
				"/auth": "http://127.0.0.1:58090",
			},
		},
	};
});
