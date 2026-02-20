import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defineConfig } from "vite";

// https://vite.dev/config/
export default defineConfig({
	plugins: [react(), tailwindcss()],
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
