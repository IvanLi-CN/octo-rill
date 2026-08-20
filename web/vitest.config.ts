import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { storybookTest } from "@storybook/addon-vitest/vitest-plugin";
import { playwright } from "@vitest/browser-playwright";
import { defineConfig, mergeConfig } from "vitest/config";
import viteConfig from "./vite.config";

const __dirname = dirname(fileURLToPath(import.meta.url));

export default defineConfig((env) =>
	mergeConfig(viteConfig(env), {
		test: {
			projects: [
				{
					extends: true,
					plugins: [
						storybookTest({
							configDir: resolve(__dirname, ".storybook"),
							tags: {
								include: ["landing-auth-feedback"],
								exclude: [],
								skip: [],
							},
						}),
					],
					test: {
						name: "storybook",
						browser: {
							enabled: true,
							provider: playwright({}),
							headless: true,
							instances: [{ browser: "chromium" }],
						},
						setupFiles: ["./.storybook/vitest.setup.ts"],
					},
				},
			],
		},
	}),
);
