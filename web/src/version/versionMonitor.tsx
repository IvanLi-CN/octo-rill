import {
	createContext,
	useCallback,
	useContext,
	useEffect,
	useMemo,
	useRef,
	useState,
	type ReactNode,
} from "react";

import { registerPwaServiceWorker } from "@/pwa/serviceWorkerRegistration";

type HealthResponse = {
	ok: boolean;
	version: string;
};

type VersionResponse = {
	ok: boolean;
	version: string;
	source: string;
};

export type VersionMonitorValue = {
	loadedVersion: string;
	availableVersion: string | null;
	hasUpdate: boolean;
	hasServiceWorkerUpdate: boolean;
	canInstallPwa?: boolean;
	isPwaInstalled?: boolean;
	refreshPage: () => void;
	promptInstallPwa?: () => Promise<void>;
};

type VersionMonitorProviderProps = {
	children: ReactNode;
	pollIntervalMs?: number;
};

type VersionMonitorStateProviderProps = {
	children: ReactNode;
	value: VersionMonitorValue;
};

export const VERSION_UNKNOWN = "unknown";
const DEFAULT_POLL_INTERVAL_MS = 60_000;
const VERSION_REQUEST_HEADERS = {
	"cache-control": "no-cache",
	pragma: "no-cache",
};
const EMBEDDED_APP_VERSION = normalizeVersion(__APP_LOADED_VERSION__);

const defaultRefreshPage = () => {
	window.location.reload();
};

const defaultPromptInstallPwa = async () => {};

const defaultValue: VersionMonitorValue = {
	loadedVersion: EMBEDDED_APP_VERSION,
	availableVersion: null,
	hasUpdate: false,
	hasServiceWorkerUpdate: false,
	canInstallPwa: false,
	isPwaInstalled: false,
	refreshPage: defaultRefreshPage,
	promptInstallPwa: defaultPromptInstallPwa,
};

const VersionMonitorContext = createContext<VersionMonitorValue>(defaultValue);

export function normalizeVersion(raw: string): string {
	const trimmed = raw.trim();
	if (!trimmed) return VERSION_UNKNOWN;
	if (
		/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(trimmed)
	) {
		return `v${trimmed}`;
	}
	return trimmed;
}

async function fetchVersionFromEndpoint(
	endpoint: string,
	signal: AbortSignal,
): Promise<string> {
	const response = await fetch(endpoint, {
		credentials: "include",
		signal,
		cache: "no-store",
		headers: VERSION_REQUEST_HEADERS,
	});
	if (!response.ok) {
		throw new Error(`version request failed (${endpoint}): ${response.status}`);
	}

	const payload = (await response.json()) as
		| Partial<HealthResponse>
		| Partial<VersionResponse>;
	if (typeof payload.version !== "string") {
		throw new Error(`version payload missing version field (${endpoint})`);
	}
	return normalizeVersion(payload.version);
}

export async function fetchLatestVersion(signal: AbortSignal): Promise<string> {
	try {
		return await fetchVersionFromEndpoint("/api/version", signal);
	} catch {
		return fetchVersionFromEndpoint("/api/health", signal);
	}
}

type BeforeInstallPromptChoice = {
	outcome?: "accepted" | "dismissed";
	platform?: string;
};

type BeforeInstallPromptEvent = Event & {
	prompt: () => Promise<void>;
	userChoice: Promise<BeforeInstallPromptChoice>;
};

function isBeforeInstallPromptEvent(
	event: Event,
): event is BeforeInstallPromptEvent {
	const candidate = event as Partial<BeforeInstallPromptEvent>;
	return (
		typeof candidate.prompt === "function" &&
		typeof candidate.userChoice?.then === "function"
	);
}

function isStandalonePwaDisplayMode() {
	if (typeof window === "undefined") return false;
	const standaloneNavigator = navigator as Navigator & {
		standalone?: boolean;
	};
	return (
		standaloneNavigator.standalone === true ||
		window.matchMedia("(display-mode: standalone)").matches
	);
}

function useVersionMonitorController(
	pollIntervalMs = DEFAULT_POLL_INTERVAL_MS,
): VersionMonitorValue {
	const [loadedVersion] = useState(EMBEDDED_APP_VERSION);
	const [availableVersion, setAvailableVersion] = useState<string | null>(null);
	const [hasUpdate, setHasUpdate] = useState(false);
	const [hasServiceWorkerUpdate, setHasServiceWorkerUpdate] = useState(false);
	const [canInstallPwa, setCanInstallPwa] = useState(false);
	const [isPwaInstalled, setIsPwaInstalled] = useState(() =>
		isStandalonePwaDisplayMode(),
	);
	const baselineVersionRef = useRef(EMBEDDED_APP_VERSION);
	const hasUpdateRef = useRef(false);
	const serviceWorkerRefreshRef = useRef<(() => void) | null>(null);
	const serviceWorkerUpdateCheckRef = useRef<(() => void) | null>(null);
	const pwaInstallPromptRef = useRef<BeforeInstallPromptEvent | null>(null);

	useEffect(() => {
		hasUpdateRef.current = hasUpdate || hasServiceWorkerUpdate;
	}, [hasServiceWorkerUpdate, hasUpdate]);

	useEffect(() => {
		registerPwaServiceWorker({
			onNeedRefresh(controller) {
				serviceWorkerRefreshRef.current = controller.applyUpdate;
				setHasServiceWorkerUpdate(true);
				setHasUpdate(true);
			},
			onRegistered(controller) {
				serviceWorkerUpdateCheckRef.current = controller.checkForUpdate;
			},
			onRegisterError() {
				// PWA installability is an enhancement; failed registration should not
				// block auth boot or normal dashboard usage.
			},
		});
	}, []);

	useEffect(() => {
		const handleBeforeInstallPrompt = (event: Event) => {
			if (!isBeforeInstallPromptEvent(event) || isStandalonePwaDisplayMode()) {
				return;
			}
			event.preventDefault();
			pwaInstallPromptRef.current = event;
			setIsPwaInstalled(false);
			setCanInstallPwa(true);
		};

		const handleAppInstalled = () => {
			pwaInstallPromptRef.current = null;
			setCanInstallPwa(false);
			setIsPwaInstalled(true);
		};

		window.addEventListener("beforeinstallprompt", handleBeforeInstallPrompt);
		window.addEventListener("appinstalled", handleAppInstalled);

		return () => {
			window.removeEventListener(
				"beforeinstallprompt",
				handleBeforeInstallPrompt,
			);
			window.removeEventListener("appinstalled", handleAppInstalled);
		};
	}, []);

	const promptInstallPwa = useCallback(async () => {
		const promptEvent = pwaInstallPromptRef.current;
		if (!promptEvent || isPwaInstalled) {
			return;
		}

		pwaInstallPromptRef.current = null;
		setCanInstallPwa(false);
		await promptEvent.prompt();
		const choice = await promptEvent.userChoice.catch(() => null);
		if (choice?.outcome === "accepted") {
			setIsPwaInstalled(true);
		}
	}, [isPwaInstalled]);

	const applyObservedVersion = useCallback((nextVersion: string) => {
		if (nextVersion !== baselineVersionRef.current) {
			setAvailableVersion(nextVersion);
			setHasUpdate(true);
			return;
		}

		setAvailableVersion(null);
		setHasUpdate(false);
	}, []);

	useEffect(() => {
		if (hasUpdate) return;

		let active = true;
		let inFlight = false;
		let currentAbortController: AbortController | null = null;

		const runCheck = async (respectVisibility: boolean) => {
			if (!active || inFlight || hasUpdateRef.current) {
				return;
			}
			if (respectVisibility && document.hidden) {
				return;
			}

			inFlight = true;
			const abortController = new AbortController();
			currentAbortController = abortController;
			try {
				const nextVersion = await fetchLatestVersion(abortController.signal);
				if (!active || abortController.signal.aborted) {
					return;
				}
				if (nextVersion !== baselineVersionRef.current) {
					serviceWorkerUpdateCheckRef.current?.();
				}
				applyObservedVersion(nextVersion);
			} catch {
				// Frontend loadedVersion is embedded at build time, so request failure
				// should not degrade footer text back to a loading/unknown placeholder.
			} finally {
				inFlight = false;
				if (currentAbortController === abortController) {
					currentAbortController = null;
				}
			}
		};

		void runCheck(false);
		const intervalId = window.setInterval(() => {
			void runCheck(true);
		}, pollIntervalMs);
		const handleVisibilityChange = () => {
			if (!document.hidden) {
				serviceWorkerUpdateCheckRef.current?.();
				void runCheck(true);
			}
		};

		document.addEventListener("visibilitychange", handleVisibilityChange);
		return () => {
			active = false;
			window.clearInterval(intervalId);
			document.removeEventListener("visibilitychange", handleVisibilityChange);
			currentAbortController?.abort();
		};
	}, [applyObservedVersion, hasUpdate, pollIntervalMs]);

	return useMemo(
		() => ({
			loadedVersion,
			availableVersion,
			hasUpdate,
			hasServiceWorkerUpdate,
			canInstallPwa,
			isPwaInstalled,
			refreshPage: () => {
				const applyServiceWorkerUpdate = serviceWorkerRefreshRef.current;
				if (applyServiceWorkerUpdate) {
					applyServiceWorkerUpdate();
					return;
				}
				defaultRefreshPage();
			},
			promptInstallPwa,
		}),
		[
			availableVersion,
			canInstallPwa,
			hasServiceWorkerUpdate,
			hasUpdate,
			isPwaInstalled,
			loadedVersion,
			promptInstallPwa,
		],
	);
}

export function VersionMonitorProvider(props: VersionMonitorProviderProps) {
	const { children, pollIntervalMs } = props;
	const value = useVersionMonitorController(pollIntervalMs);
	return (
		<VersionMonitorContext.Provider value={value}>
			{children}
		</VersionMonitorContext.Provider>
	);
}

export function VersionMonitorStateProvider(
	props: VersionMonitorStateProviderProps,
) {
	const { children, value } = props;
	return (
		<VersionMonitorContext.Provider value={value}>
			{children}
		</VersionMonitorContext.Provider>
	);
}

export function useVersionMonitor() {
	return useContext(VersionMonitorContext);
}
