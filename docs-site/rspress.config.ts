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
const docsBrandMark = `${docsBase}brand/mark.svg`;
const docsFavicon = new URL("./docs/public/favicon.ico", import.meta.url);

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
	description: "OctoRill 的启动、配置、产品说明与常用入口。",
	lang: "zh",
	themeConfig: {
		search: true,
		nav: [
			{ text: "文档", link: "/" },
			{ text: "快速开始", link: "/quick-start" },
			{ text: "配置", link: "/config" },
			{ text: "产品", link: "/product" },
			{ text: "Storybook", link: "/storybook.html" },
			{ text: "GitHub", link: "https://github.com/IvanLi-CN/octo-rill" },
		],
		sidebar: {
			"/": [
				{
					text: "开始使用",
					items: [
						{ text: "文档首页", link: "/" },
						{ text: "快速开始", link: "/quick-start" },
						{ text: "配置参考", link: "/config" },
					],
				},
				{
					text: "理解产品",
					items: [{ text: "产品说明", link: "/product" }],
				},
				{
					text: "其他入口",
					items: [
						{ text: "Storybook", link: "/storybook.html" },
						{ text: "GitHub 仓库", link: "https://github.com/IvanLi-CN/octo-rill" },
					],
				},
			],
		},
	},
});
