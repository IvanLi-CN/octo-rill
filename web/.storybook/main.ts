import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { StorybookConfig } from "@storybook/react-vite";
import tailwindcss from "@tailwindcss/vite";
import { mergeConfig } from "vite";

const __dirname = dirname(fileURLToPath(import.meta.url));

const config: StorybookConfig = {
	stories: ["../src/**/*.stories.@(ts|tsx)"],
	addons: ["@storybook/addon-docs"],
	framework: {
		name: "@storybook/react-vite",
		options: {},
	},
	async viteFinal(baseConfig) {
		return mergeConfig(baseConfig, {
			plugins: [tailwindcss()],
			resolve: {
				alias: {
					"@": resolve(__dirname, "../src"),
				},
			},
		});
	},
};

export default config;
