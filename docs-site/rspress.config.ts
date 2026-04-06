import { defineConfig } from "rspress/config";

function normalizeBase(base: string | undefined): string {
	const raw = (base ?? "/").trim();
	if (!raw || raw === "/") return "/";
	const withLeading = raw.startsWith("/") ? raw : `/${raw}`;
	return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}


const docsBase = normalizeBase(process.env.DOCS_BASE);
const localStorybookDevOrigin =
	process.env.VITE_STORYBOOK_DEV_ORIGIN?.trim() ?? "";
const docsBrandMark = "/brand/mark.svg";
const docsFavicon = "/favicon.ico";

export default defineConfig({
	root: "docs",
	base: docsBase,
	logo: docsBrandMark,
	logoText: "OctoRill",
	icon: docsFavicon,
	builderConfig: {
		source: {
			define: {
				"process.env.RSPRESS_STORYBOOK_DEV_ORIGIN": JSON.stringify(
					localStorybookDevOrigin,
				),
			},
		},
	},
	title: "OctoRill 文档",
	description: "OctoRill 的快速开始、配置参考、产品说明与 Storybook 入口。",
	lang: "zh",
	themeConfig: {
		search: true,
		nav: [
			{ text: "首页", link: "/" },
			{ text: "快速开始", link: "/quick-start" },
			{ text: "配置参考", link: "/config" },
			{ text: "产品说明", link: "/product" },
			{ text: "Storybook", link: "/storybook.html" },
			{ text: "GitHub", link: "https://github.com/IvanLi-CN/octo-rill" },
		],
		sidebar: {
			"/": [
				{
					text: "入门",
					items: [
						{ text: "文档首页", link: "/" },
						{ text: "快速开始", link: "/quick-start" },
						{ text: "配置参考", link: "/config" },
						{ text: "产品说明", link: "/product" },
					],
				},
				{
					text: "预览与源码",
					items: [
						{ text: "Storybook 导览", link: "/storybook-guide.html" },
						{ text: "GitHub 仓库", link: "https://github.com/IvanLi-CN/octo-rill" },
					],
				},
			],
		},
	},
});
