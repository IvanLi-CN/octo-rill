import { prepareDemoRuntime, shouldPrepareDemoRuntime } from "@/demo/runtime";
import "./index.css";

async function renderApp() {
	const { renderApp } = await import("./renderApp");
	renderApp(document.getElementById("root")!);
}

function resolveDemoDocsHomeHref() {
	const baseUrl = import.meta.env.BASE_URL || "/";
	if (baseUrl === "/") {
		return "/";
	}

	const normalizedBase = baseUrl.endsWith("/") ? baseUrl.slice(0, -1) : baseUrl;
	if (normalizedBase.endsWith("/demo")) {
		const docsBase = normalizedBase.slice(0, -"/demo".length);
		return docsBase ? `${docsBase}/` : "/";
	}

	return baseUrl.startsWith("/") ? baseUrl : `/${baseUrl}`;
}

function appendTextBlock(
	parent: HTMLElement,
	tagName: string,
	className: string,
	text: string,
) {
	const element = document.createElement(tagName);
	element.className = className;
	element.textContent = text;
	parent.append(element);
	return element;
}

function renderDemoBootFailure(error: unknown) {
	const root = document.getElementById("root");
	if (!root) return;

	const detail =
		error instanceof Error && error.message.trim().length > 0
			? error.message.trim()
			: "Unknown demo bootstrap error.";

	root.replaceChildren();

	const main = document.createElement("main");
	main.className = "min-h-dvh bg-background px-4 py-10 text-foreground sm:px-6";

	const shell = document.createElement("div");
	shell.className =
		"mx-auto flex min-h-[calc(100dvh-5rem)] max-w-3xl items-center";

	const panel = document.createElement("section");
	panel.className =
		"w-full rounded-[28px] border border-border/70 bg-card/95 p-6 shadow-2xl sm:p-8";

	appendTextBlock(
		panel,
		"p",
		"font-mono text-xs text-muted-foreground",
		"Web Demo Bootstrap Error",
	);
	appendTextBlock(
		panel,
		"h1",
		"mt-3 text-2xl font-semibold sm:text-3xl",
		"Web Demo 启动失败",
	);
	appendTextBlock(
		panel,
		"p",
		"mt-3 max-w-[62ch] text-sm leading-6 text-muted-foreground sm:text-base",
		"Demo runtime 没有完成 mock-only 启动，所以应用没有继续渲染，以避免误触真实接口或真实认证链路。",
	);

	const detailWrap = document.createElement("div");
	detailWrap.className =
		"mt-5 rounded-2xl border border-dashed bg-muted/25 p-4";
	appendTextBlock(detailWrap, "p", "text-sm font-medium", "启动错误");
	appendTextBlock(
		detailWrap,
		"p",
		"mt-2 font-mono text-xs leading-6 text-muted-foreground break-all",
		detail,
	);
	panel.append(detailWrap);

	const actions = document.createElement("div");
	actions.className = "mt-6 flex flex-wrap gap-3";

	const retryButton = document.createElement("button");
	retryButton.type = "button";
	retryButton.className =
		"inline-flex h-10 items-center justify-center rounded-2xl bg-foreground px-4 text-sm font-medium text-background";
	retryButton.textContent = "重新尝试";
	retryButton.addEventListener("click", () => {
		window.location.reload();
	});
	actions.append(retryButton);

	const homeLink = document.createElement("a");
	homeLink.className =
		"inline-flex h-10 items-center justify-center rounded-2xl border border-border/80 px-4 text-sm font-medium text-foreground";
	homeLink.href = resolveDemoDocsHomeHref();
	homeLink.textContent = "回到文档首页";
	actions.append(homeLink);

	panel.append(actions);
	shell.append(panel);
	main.append(shell);
	root.append(main);
}

if (shouldPrepareDemoRuntime()) {
	void prepareDemoRuntime().then(renderApp).catch(renderDemoBootFailure);
} else {
	void renderApp();
}
