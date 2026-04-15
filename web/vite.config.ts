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

// https://vite.dev/config/
export default defineConfig({
	define: {
		__APP_LOADED_VERSION__: JSON.stringify(embeddedAppVersion),
	},
	plugins: [
		tanstackRouter({
			target: "react",
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
});
