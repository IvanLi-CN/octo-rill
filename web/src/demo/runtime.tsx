import { type ReactNode, useEffect, useSyncExternalStore } from "react";

import {
	applyDemoShareStateToSearchParams,
	buildDefaultDemoShareState,
	buildDemoHref,
	DEMO_PANEL_LAYOUT_STORAGE_KEY,
	getDemoScene,
	readDemoShareState,
	resolveDefaultSceneId,
} from "@/demo/registry";
import type {
	DemoModel,
	DemoPanelLayout,
	DemoShareStatePatch,
	DemoShareState,
	DemoSnapshot,
} from "@/demo/types";

type DemoEventSourceFactory = (
	url: string,
	options?: {
		withCredentials?: boolean;
	},
) => EventSource;

type DemoRuntimeDependencies = {
	buildDemoModel: (options: {
		personaId: DemoShareState["personaId"];
		includeOwnReleases: boolean;
		publicationState: DemoShareState["publicationState"];
	}) => DemoModel;
	demoHandlers: typeof import("@/demo/transport")["demoHandlers"];
	handleDemoUnhandledRequest: typeof import("@/demo/transport")["handleDemoUnhandledRequest"];
	openDemoEventSource: typeof import("@/demo/transport")["openDemoEventSource"];
	registerDemoRuntimeAccess: typeof import("@/demo/transport")["registerDemoRuntimeAccess"];
	setupWorker: typeof import("msw/browser")["setupWorker"];
	clearAllWarmStartupCaches: typeof import("@/auth/startupCache")["clearAllWarmStartupCaches"];
};

const DEFAULT_PANEL_LAYOUT: DemoPanelLayout = {
	edge: "right",
	x: 20,
	y: 88,
	collapsed: false,
};

const listeners = new Set<() => void>();

let demoDependencies: DemoRuntimeDependencies | null = null;
let demoDependenciesPromise: Promise<DemoRuntimeDependencies> | null = null;
let demoEventSourceFactory: DemoEventSourceFactory | null = null;
let demoWorker: ReturnType<DemoRuntimeDependencies["setupWorker"]> | null =
	null;
let workerStartPromise: Promise<void> | null = null;
const DEMO_WORKER_START_TIMEOUT_MS = 4_000;

const runtimeState: DemoSnapshot = {
	active: false,
	demoBuild: false,
	basepath: "/",
	revision: 0,
	shareState: {
		sceneId: "landing-welcome",
		personaId: "guest",
		networkMode: "normal",
		includeOwnReleases: false,
		publicationState: "unpublished",
	},
	model: null,
	mutations: [],
	panelLayout: DEFAULT_PANEL_LAYOUT,
	lastSyncedHref: null,
};
let runtimeSnapshot: DemoSnapshot = { ...runtimeState };

function emit() {
	runtimeSnapshot = { ...runtimeState };
	for (const listener of listeners) {
		listener();
	}
}

function patchRuntimeShareState(partial: DemoShareStatePatch) {
	runtimeState.shareState = {
		...runtimeState.shareState,
		...partial,
	};
}

function modelAffectingShareStateChanged(
	current: DemoShareState,
	next: DemoShareState,
) {
	return (
		current.sceneId !== next.sceneId ||
		current.personaId !== next.personaId ||
		current.includeOwnReleases !== next.includeOwnReleases ||
		current.publicationState !== next.publicationState
	);
}

function canUseStorage() {
	return (
		typeof window !== "undefined" && typeof window.localStorage !== "undefined"
	);
}

function readPanelLayout(): DemoPanelLayout {
	if (!canUseStorage()) return DEFAULT_PANEL_LAYOUT;
	try {
		const raw = window.localStorage.getItem(DEMO_PANEL_LAYOUT_STORAGE_KEY);
		if (!raw) return DEFAULT_PANEL_LAYOUT;
		const parsed = JSON.parse(raw) as Partial<DemoPanelLayout>;
		return {
			edge: parsed.edge === "left" ? "left" : "right",
			x: typeof parsed.x === "number" ? parsed.x : DEFAULT_PANEL_LAYOUT.x,
			y: typeof parsed.y === "number" ? parsed.y : DEFAULT_PANEL_LAYOUT.y,
			collapsed:
				typeof parsed.collapsed === "boolean"
					? parsed.collapsed
					: DEFAULT_PANEL_LAYOUT.collapsed,
		};
	} catch {
		return DEFAULT_PANEL_LAYOUT;
	}
}

function persistPanelLayout(layout: DemoPanelLayout) {
	if (!canUseStorage()) return;
	try {
		window.localStorage.setItem(
			DEMO_PANEL_LAYOUT_STORAGE_KEY,
			JSON.stringify(layout),
		);
	} catch {
		// ignore storage failures
	}
}

function demoBuildEnabled() {
	return Boolean(__OCTO_RILL_DEMO_APP__);
}

function regularBuildDemoEnabled() {
	return Boolean(import.meta.env.DEV);
}

function demoBasepath() {
	return __OCTO_RILL_ROUTER_BASEPATH__ || "/";
}

function isDemoUrl(url: URL) {
	return (
		demoBuildEnabled() ||
		(regularBuildDemoEnabled() &&
			(url.searchParams.has("demo") || url.searchParams.has("d_restore")))
	);
}

async function loadDemoDependencies() {
	if (demoDependencies) {
		return demoDependencies;
	}
	if (demoDependenciesPromise) {
		return demoDependenciesPromise;
	}

	demoDependenciesPromise = Promise.all([
		import("msw/browser"),
		import("@/auth/startupCache"),
		import("@/demo/fixtures"),
		import("@/demo/transport"),
	]).then(([mswBrowser, startupCache, fixtures, transport]) => {
		demoDependencies = {
			buildDemoModel: fixtures.buildDemoModel,
			demoHandlers: transport.demoHandlers,
			handleDemoUnhandledRequest: transport.handleDemoUnhandledRequest,
			openDemoEventSource: transport.openDemoEventSource,
			registerDemoRuntimeAccess: transport.registerDemoRuntimeAccess,
			setupWorker: mswBrowser.setupWorker,
			clearAllWarmStartupCaches: startupCache.clearAllWarmStartupCaches,
		};
		return demoDependencies;
	});

	return demoDependenciesPromise;
}

function requireDemoDependencies() {
	if (!demoDependencies) {
		throw new Error("Demo runtime is not prepared");
	}
	return demoDependencies;
}

function seedModel(shareState: DemoShareState): DemoModel {
	return requireDemoDependencies().buildDemoModel({
		personaId: shareState.personaId,
		includeOwnReleases: shareState.includeOwnReleases,
		publicationState: shareState.publicationState,
	});
}

function stopDemoWorker() {
	if (!demoWorker) {
		return;
	}
	demoWorker.stop();
	demoWorker = null;
	workerStartPromise = null;
	demoEventSourceFactory = null;
}

function parseHref(href: string) {
	return new URL(href, window.location.origin);
}

function nextSnapshotFromUrl(url: URL) {
	const active = isDemoUrl(url);
	if (!active) {
		return {
			active: false,
			shareState: runtimeState.shareState,
			model: null,
		};
	}

	const shareState = readDemoShareState(url, demoBasepath());
	return {
		active: true,
		shareState,
		model: seedModel(shareState),
	};
}

async function ensureWorkerStarted() {
	if (workerStartPromise) {
		return workerStartPromise;
	}

	const dependencies = requireDemoDependencies();
	demoWorker ??= dependencies.setupWorker(...dependencies.demoHandlers);
	workerStartPromise = Promise.race([
		demoWorker.start({
			quiet: true,
			serviceWorker: {
				url: `${import.meta.env.BASE_URL}mockServiceWorker.js`,
				options: {
					scope: import.meta.env.BASE_URL,
				},
			},
			onUnhandledRequest(request, print) {
				dependencies.handleDemoUnhandledRequest(request, print);
			},
		}),
		new Promise<never>((_, reject) => {
			window.setTimeout(() => {
				reject(
					new Error(
						`Demo worker did not start within ${DEMO_WORKER_START_TIMEOUT_MS}ms.`,
					),
				);
			}, DEMO_WORKER_START_TIMEOUT_MS);
		}),
	])
		.then(() => undefined)
		.catch((error) => {
			stopDemoWorker();
			throw error;
		});
	return workerStartPromise;
}

function apply404RestoreIfNeeded() {
	if (typeof window === "undefined") return;
	if (!demoBuildEnabled()) return;
	const url = new URL(window.location.href);
	const restore = url.searchParams.get("d_restore");
	if (!restore) return;
	try {
		const decoded = decodeURIComponent(restore);
		const target = new URL(decoded, window.location.origin);
		window.history.replaceState(
			{},
			"",
			`${target.pathname}${target.search}${target.hash}`,
		);
	} catch {
		url.searchParams.delete("d_restore");
		window.history.replaceState(
			{},
			"",
			`${url.pathname}${url.search}${url.hash}`,
		);
	}
}

function appendMutation(label: string, detail: string) {
	const next = {
		id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
		label,
		detail,
		at: new Date().toISOString(),
	};
	runtimeState.mutations = [next, ...runtimeState.mutations].slice(0, 8);
	emit();
}

function persistDemoShareStateToWindowUrl() {
	if (typeof window === "undefined" || !runtimeState.active) {
		return;
	}
	const nextHref = buildCurrentDemoShareHref();
	runtimeState.lastSyncedHref = nextHref;
	const nextUrl = new URL(nextHref, window.location.origin);
	const currentUrl = new URL(window.location.href);
	if (
		currentUrl.pathname === nextUrl.pathname &&
		currentUrl.search === nextUrl.search &&
		currentUrl.hash === nextUrl.hash
	) {
		return;
	}
	window.history.replaceState({}, "", nextUrl.toString());
}

export async function prepareDemoRuntime() {
	if (typeof window === "undefined") return;

	apply404RestoreIfNeeded();
	const url = new URL(window.location.href);
	const active = isDemoUrl(url);

	runtimeState.demoBuild = demoBuildEnabled();
	runtimeState.basepath = demoBasepath();
	runtimeState.panelLayout = readPanelLayout();
	runtimeState.lastSyncedHref = `${url.pathname}${url.search}${url.hash}`;
	runtimeState.active = active;

	if (!active) {
		stopDemoWorker();
		runtimeState.model = null;
		runtimeState.mutations = [];
		runtimeSnapshot = { ...runtimeState };
		return;
	}

	const dependencies = await loadDemoDependencies();
	const next = nextSnapshotFromUrl(url);
	runtimeState.shareState = next.shareState;
	runtimeState.model = next.model;

	dependencies.registerDemoRuntimeAccess({
		getSnapshot: () => runtimeSnapshot,
		updateModel: (updater) => {
			if (!runtimeState.model) return;
			runtimeState.model = updater(runtimeState.model);
			emit();
		},
		patchShareState: (partial) => {
			patchRuntimeShareState(partial);
			persistDemoShareStateToWindowUrl();
			emit();
		},
		recordMutation: appendMutation,
	});
	demoEventSourceFactory = dependencies.openDemoEventSource;

	dependencies.clearAllWarmStartupCaches();
	await ensureWorkerStarted();
	runtimeSnapshot = { ...runtimeState };
}

export function getDemoSnapshot() {
	return runtimeSnapshot;
}

export function shouldPrepareDemoRuntime() {
	if (typeof window === "undefined") return false;
	return isDemoUrl(new URL(window.location.href));
}

export function isDemoMode() {
	return runtimeState.active;
}

export function getDemoRouterBasepath() {
	return demoBasepath();
}

export function getDemoEventSourceFactory() {
	return demoEventSourceFactory;
}

export function useDemoSnapshot() {
	return useSyncExternalStore(
		(listener) => {
			listeners.add(listener);
			return () => listeners.delete(listener);
		},
		() => runtimeSnapshot,
		() => runtimeSnapshot,
	);
}

export function syncDemoRuntimeWithHref(href: string) {
	if (typeof window === "undefined") return;
	const nextHref = href.startsWith("http")
		? new URL(href).pathname + new URL(href).search + new URL(href).hash
		: href;
	if (runtimeState.lastSyncedHref === nextHref) return;
	if (!demoDependencies) return;

	const currentShareState = runtimeState.shareState;
	const next = nextSnapshotFromUrl(parseHref(nextHref));
	runtimeState.lastSyncedHref = nextHref;
	if (!next.active && !runtimeState.demoBuild) {
		stopDemoWorker();
	}
	const shouldReseedModel =
		next.active &&
		(!runtimeState.active ||
			modelAffectingShareStateChanged(currentShareState, next.shareState));
	runtimeState.active = next.active;
	runtimeState.shareState = next.shareState;
	if (!next.active) {
		runtimeState.model = null;
	} else if (shouldReseedModel || !runtimeState.model) {
		runtimeState.model = next.model;
	}
	if (shouldReseedModel) {
		runtimeState.revision += 1;
	}
	emit();
}

export function patchDemoShareState(
	partial: Partial<DemoShareState>,
	options?: {
		reseed?: boolean;
	},
) {
	if (!demoDependencies) return;
	const nextShareState: DemoShareState = {
		...runtimeState.shareState,
		...partial,
	};
	const shouldBumpRevision =
		options?.reseed !== false &&
		modelAffectingShareStateChanged(runtimeState.shareState, nextShareState);
	runtimeState.shareState = nextShareState;
	if (options?.reseed !== false) {
		runtimeState.model = seedModel(nextShareState);
		runtimeState.mutations = [];
	}
	if (shouldBumpRevision) {
		runtimeState.revision += 1;
	}
	emit();
}

export function resetDemoScene() {
	if (!demoDependencies) return;
	const nextShareState = buildDefaultDemoShareState(
		runtimeState.shareState.sceneId,
	);
	runtimeState.shareState = nextShareState;
	runtimeState.model = seedModel(nextShareState);
	runtimeState.mutations = [];
	runtimeState.revision += 1;
	emit();
}

export function updateDemoPanelLayout(
	partial: Partial<DemoPanelLayout>,
	options?: {
		emit?: boolean;
	},
) {
	runtimeState.panelLayout = {
		...runtimeState.panelLayout,
		...partial,
	};
	persistPanelLayout(runtimeState.panelLayout);
	if (options?.emit !== false) {
		emit();
	}
}

export function buildCurrentDemoHref(nextShareState?: Partial<DemoShareState>) {
	return buildDemoHref(
		{
			...runtimeState.shareState,
			...nextShareState,
		},
		runtimeState.basepath,
	);
}

export function buildCurrentDemoShareHref(
	nextShareState?: Partial<DemoShareState>,
) {
	if (typeof window === "undefined") {
		return buildCurrentDemoHref(nextShareState);
	}

	const url = new URL(window.location.href);
	const params = new URLSearchParams(url.search);
	applyDemoShareStateToSearchParams(params, {
		...runtimeState.shareState,
		...nextShareState,
	});
	const query = params.toString();
	return `${url.pathname}${query ? `?${query}` : ""}${url.hash}`;
}

export function resolveCurrentDemoScene() {
	return getDemoScene(runtimeState.shareState.sceneId);
}

export function resolveSceneForPath(pathname: string) {
	return getDemoScene(resolveDefaultSceneId(pathname, runtimeState.basepath));
}

export function DemoBootstrapBoundary(props: { children: ReactNode }) {
	const { children } = props;
	const snapshot = useDemoSnapshot();

	useEffect(() => {
		if (!snapshot.active) return;
		const href = `${window.location.pathname}${window.location.search}${window.location.hash}`;
		if (snapshot.lastSyncedHref === href) return;
		syncDemoRuntimeWithHref(href);
	}, [snapshot.active, snapshot.lastSyncedHref]);

	return <>{children}</>;
}
