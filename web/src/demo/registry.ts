import {
	buildDashboardScopeHref,
	type DashboardScope,
} from "@/dashboard/routeState";
import { buildSettingsHref } from "@/settings/routeState";
import type {
	DemoNetworkMode,
	DemoPersonaId,
	DemoPublicationState,
	DemoScene,
	DemoSceneId,
	DemoShareState,
} from "@/demo/types";

const repoScope: DashboardScope = {
	kind: "repo",
	owner: "octo-demo",
	repo: "release-lab",
};

export const DEMO_SCENES: DemoScene[] = [
	{
		id: "landing-welcome",
		title: "Landing",
		description: "匿名 landing 与登录入口。",
		path: "/",
		defaultPersona: "guest",
		personas: ["guest", "member", "admin"],
	},
	{
		id: "dashboard-repo-publish",
		title: "Dashboard",
		description: "repo scope 发布控制、Briefs / Inbox 与 simulated sync。",
		path: buildDashboardScopeHref(repoScope),
		defaultPersona: "member",
		personas: ["member", "admin", "guest"],
	},
	{
		id: "settings-my-releases",
		title: "Settings",
		description: "设置页深链到“我的发布”，带 simulated save。",
		path: buildSettingsHref("my-releases"),
		defaultPersona: "member",
		personas: ["member", "admin", "guest"],
	},
	{
		id: "public-release-ready",
		title: "Public Release",
		description: "公开 Release 详情页，验证匿名可读与 mock-only 数据。",
		path: "/public/octo-demo/release-lab/releases/tag/v2.31.0",
		defaultPersona: "guest",
		personas: ["guest", "member", "admin"],
	},
	{
		id: "admin-panel-users",
		title: "Admin Panel",
		description: "用户治理页，支持 simulated role / disabled / profile save。",
		path: "/admin/users",
		defaultPersona: "admin",
		personas: ["admin", "member", "guest"],
	},
	{
		id: "admin-jobs-running",
		title: "Admin Jobs",
		description: "任务中心运行态、设置保存与流式刷新入口。",
		path: "/admin/jobs",
		defaultPersona: "admin",
		personas: ["admin", "member", "guest"],
	},
];

const DEMO_SCENE_BY_ID = new Map<string, DemoScene>(
	DEMO_SCENES.map((scene) => [scene.id, scene]),
);

function normalizeBasepath(basepath: string) {
	if (!basepath || basepath === "/") return "";
	return basepath.endsWith("/") ? basepath.slice(0, -1) : basepath;
}

export function getDemoScene(sceneId: DemoSceneId) {
	return (
		DEMO_SCENE_BY_ID.get(sceneId) ?? DEMO_SCENE_BY_ID.get("landing-welcome")!
	);
}

export function resolveDefaultSceneId(
	pathname: string,
	basepath: string,
): DemoSceneId {
	const normalizedBasepath = normalizeBasepath(basepath);
	const normalizedPath =
		normalizedBasepath && pathname.startsWith(normalizedBasepath)
			? pathname.slice(normalizedBasepath.length) || "/"
			: pathname || "/";

	if (normalizedPath.startsWith("/admin/jobs")) return "admin-jobs-running";
	if (normalizedPath.startsWith("/admin")) return "admin-panel-users";
	if (normalizedPath.startsWith("/settings")) return "settings-my-releases";
	if (normalizedPath.startsWith("/public/")) return "public-release-ready";
	if (normalizedPath.startsWith("/focus/")) return "dashboard-repo-publish";
	return "landing-welcome";
}

export function normalizePersonaId(
	value: string | null | undefined,
	fallback: DemoPersonaId,
): DemoPersonaId {
	if (value === "guest" || value === "member" || value === "admin") {
		return value;
	}
	return fallback;
}

export function normalizeNetworkMode(
	value: string | null | undefined,
): DemoNetworkMode {
	if (value === "slow" || value === "faulty") return value;
	return "normal";
}

export function normalizePublicationState(
	value: string | null | undefined,
): DemoPublicationState {
	return value === "published" ? "published" : "unpublished";
}

export function readDemoShareState(url: URL, basepath: string): DemoShareState {
	const sceneIdRaw = url.searchParams.get("demo") as DemoSceneId | null;
	const sceneId = DEMO_SCENE_BY_ID.has(sceneIdRaw ?? "")
		? (sceneIdRaw as DemoSceneId)
		: resolveDefaultSceneId(url.pathname, basepath);
	const defaults = buildDefaultDemoShareState(sceneId);

	return {
		sceneId,
		personaId: normalizePersonaId(
			url.searchParams.get("d_persona"),
			defaults.personaId,
		),
		networkMode: normalizeNetworkMode(url.searchParams.get("d_net")),
		includeOwnReleases: url.searchParams.get("d_own") === "1",
		publicationState: normalizePublicationState(url.searchParams.get("d_pub")),
	};
}

export function buildDefaultDemoShareState(
	sceneId: DemoSceneId,
): DemoShareState {
	const scene = getDemoScene(sceneId);
	return {
		sceneId,
		personaId: scene.defaultPersona,
		networkMode: "normal",
		includeOwnReleases: false,
		publicationState: "unpublished",
	};
}

export function applyDemoShareStateToSearchParams(
	target: URLSearchParams,
	state: DemoShareState,
) {
	target.delete("demo");
	for (const key of Array.from(target.keys())) {
		if (key.startsWith("d_")) {
			target.delete(key);
		}
	}

	target.set("demo", state.sceneId);
	target.set("d_persona", state.personaId);
	if (state.networkMode !== "normal") {
		target.set("d_net", state.networkMode);
	}
	if (state.includeOwnReleases) {
		target.set("d_own", "1");
	}
	if (state.publicationState === "published") {
		target.set("d_pub", "published");
	}
	return target;
}

export function buildDemoHref(state: DemoShareState, basepath: string): string {
	const scene = getDemoScene(state.sceneId);
	const [scenePath, sceneSearch = ""] = scene.path.split("?", 2);
	const params = new URLSearchParams(sceneSearch);
	applyDemoShareStateToSearchParams(params, state);
	const path = `${normalizeBasepath(basepath)}${scenePath}` || "/";
	const query = params.toString();
	return query ? `${path}?${query}` : path;
}

export function copyDemoSearchParams(
	source: URLSearchParams,
	target: URLSearchParams,
) {
	for (const [key, value] of source.entries()) {
		if (key !== "demo" && !key.startsWith("d_")) continue;
		if (target.has(key)) continue;
		target.set(key, value);
	}
	return target;
}

export function readCurrentDemoSearchParams(search?: string) {
	if (typeof search === "string") {
		return new URLSearchParams(
			search.startsWith("?") ? search.slice(1) : search,
		);
	}
	if (typeof window === "undefined") {
		return new URLSearchParams();
	}
	return new URLSearchParams(window.location.search);
}

export function buildCurrentDemoSearchObject(search?: string) {
	const params = readCurrentDemoSearchParams(search);
	const demoOnly = new URLSearchParams();
	copyDemoSearchParams(params, demoOnly);
	const entries = Array.from(demoOnly.entries());
	return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

export function preserveCurrentDemoSearchInHref(href: string) {
	const next = new URL(href, window.location.origin);
	copyDemoSearchParams(readCurrentDemoSearchParams(), next.searchParams);
	return `${next.pathname}${next.search}${next.hash}`;
}

function resolveCurrentDemoBasepath() {
	return normalizeBasepath(__OCTO_RILL_ROUTER_BASEPATH__ || "/");
}

export function resolveDemoNativeHref(href: string) {
	if (typeof window === "undefined") {
		return href;
	}

	const next = new URL(href, window.location.origin);
	if (next.origin !== window.location.origin) {
		return href;
	}

	const basepath = resolveCurrentDemoBasepath();
	if (basepath) {
		if (next.pathname === "/") {
			next.pathname = `${basepath}/`;
		} else if (
			next.pathname !== basepath &&
			!next.pathname.startsWith(`${basepath}/`)
		) {
			next.pathname = `${basepath}${next.pathname}`;
		}
	}

	copyDemoSearchParams(readCurrentDemoSearchParams(), next.searchParams);
	return `${next.pathname}${next.search}${next.hash}`;
}

export const DEMO_PANEL_LAYOUT_STORAGE_KEY = "octo-rill.demo.panel-layout.v1";
