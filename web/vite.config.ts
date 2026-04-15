import fs from "node:fs";
import path from "node:path";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defineConfig } from "vite";

const repoRoot = path.resolve(__dirname, "..");
const cargoTomlPath = path.resolve(repoRoot, "Cargo.toml");
const cargoToml = fs.readFileSync(cargoTomlPath, "utf8");
const cargoVersionMatch = cargoToml.match(
	/^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m,
);
const embeddedAppVersion =
	process.env.APP_EFFECTIVE_VERSION?.trim() ||
	cargoVersionMatch?.[1] ||
	"unknown";

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
