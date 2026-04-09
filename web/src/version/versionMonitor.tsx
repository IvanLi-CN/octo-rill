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
	refreshPage: () => void;
};

type VersionMonitorProviderProps = {
	children: ReactNode;
	pollIntervalMs?: number;
};

type VersionMonitorStateProviderProps = {
	children: ReactNode;
	value: VersionMonitorValue;
};

export const VERSION_LOADING = "loading...";
export const VERSION_UNKNOWN = "unknown";
const DEFAULT_POLL_INTERVAL_MS = 60_000;
const VERSION_REQUEST_HEADERS = {
	"cache-control": "no-cache",
	pragma: "no-cache",
};

const defaultRefreshPage = () => {
	window.location.reload();
};

const defaultValue: VersionMonitorValue = {
	loadedVersion: VERSION_LOADING,
	availableVersion: null,
	hasUpdate: false,
	refreshPage: defaultRefreshPage,
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

function useVersionMonitorController(
	pollIntervalMs = DEFAULT_POLL_INTERVAL_MS,
): VersionMonitorValue {
	const [loadedVersion, setLoadedVersion] = useState(VERSION_LOADING);
	const [availableVersion, setAvailableVersion] = useState<string | null>(null);
	const [hasUpdate, setHasUpdate] = useState(false);
	const baselineVersionRef = useRef<string | null>(null);
	const hasUpdateRef = useRef(false);

	useEffect(() => {
		hasUpdateRef.current = hasUpdate;
	}, [hasUpdate]);

	const applyObservedVersion = useCallback((nextVersion: string) => {
		const baselineVersion = baselineVersionRef.current;
		if (!baselineVersion) {
			baselineVersionRef.current = nextVersion;
			setLoadedVersion(nextVersion);
			setAvailableVersion(null);
			setHasUpdate(false);
			return;
		}

		if (nextVersion !== baselineVersion) {
			setAvailableVersion(nextVersion);
			setHasUpdate(true);
			return;
		}

		setLoadedVersion(baselineVersion);
		setAvailableVersion(null);
		setHasUpdate(false);
	}, []);

	const handleVersionCheckFailure = useCallback(() => {
		if (baselineVersionRef.current === null) {
			setLoadedVersion(VERSION_UNKNOWN);
		}
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
				applyObservedVersion(nextVersion);
			} catch {
				if (active && !abortController.signal.aborted) {
					handleVersionCheckFailure();
				}
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
	}, [
		applyObservedVersion,
		handleVersionCheckFailure,
		hasUpdate,
		pollIntervalMs,
	]);

	return useMemo(
		() => ({
			loadedVersion,
			availableVersion,
			hasUpdate,
			refreshPage: defaultRefreshPage,
		}),
		[availableVersion, hasUpdate, loadedVersion],
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
