import {
	createContext,
	useCallback,
	useContext,
	useEffect,
	useMemo,
	useState,
	type ReactNode,
} from "react";

import { type MeResponse, ApiError, apiGet } from "@/api";
import {
	clearAllWarmStartupCaches,
	persistAuthenticatedStartup,
	readStartupPresentationSeed,
	type StartupPresentation,
} from "@/auth/startupCache";

export type AuthBootstrapStatus = "pending" | "anonymous" | "authenticated";

type AuthSnapshot = {
	status: AuthBootstrapStatus;
	me: MeResponse | null;
	bootError: string | null;
};

export type AuthBootstrapValue = AuthSnapshot & {
	isAuthenticated: boolean;
	isBootstrapping: boolean;
	bootPresentation: StartupPresentation;
	refreshAuth: () => Promise<MeResponse | null>;
};

const AuthBootstrapContext = createContext<AuthBootstrapValue | null>(null);

const startupSeed = readStartupPresentationSeed();

let cachedSnapshot: AuthSnapshot | null = startupSeed
	? {
			status: "authenticated",
			me: startupSeed.me,
			bootError: null,
		}
	: null;
let cachedSnapshotOrigin: "seed" | "network" | "none" = startupSeed
	? "seed"
	: "none";
let inflightSnapshotPromise: Promise<AuthSnapshot> | null = null;

async function requestAuthSnapshot(): Promise<AuthSnapshot> {
	try {
		const me = await apiGet<MeResponse>("/api/me");
		persistAuthenticatedStartup(me);
		return {
			status: "authenticated",
			me,
			bootError: null,
		};
	} catch (err) {
		if (err instanceof ApiError && err.status === 401) {
			clearAllWarmStartupCaches();
			return {
				status: "anonymous",
				me: null,
				bootError: null,
			};
		}

		if (cachedSnapshot?.status === "authenticated" && cachedSnapshot.me) {
			return {
				status: "authenticated",
				me: cachedSnapshot.me,
				bootError: err instanceof Error ? err.message : String(err),
			};
		}

		return {
			status: "anonymous",
			me: null,
			bootError: err instanceof Error ? err.message : String(err),
		};
	}
}

async function loadAuthSnapshot(force = false) {
	if (!force && cachedSnapshotOrigin === "network" && cachedSnapshot) {
		return cachedSnapshot;
	}
	if (!force && inflightSnapshotPromise) {
		return inflightSnapshotPromise;
	}

	inflightSnapshotPromise = requestAuthSnapshot()
		.then((snapshot) => {
			cachedSnapshot = snapshot;
			cachedSnapshotOrigin = "network";
			return snapshot;
		})
		.finally(() => {
			inflightSnapshotPromise = null;
		});

	return inflightSnapshotPromise;
}

export function AuthBootstrapProvider(props: { children: ReactNode }) {
	const { children } = props;
	const [snapshot, setSnapshot] = useState<AuthSnapshot>(() => {
		if (startupSeed) {
			return {
				status: "authenticated",
				me: startupSeed.me,
				bootError: null,
			};
		}
		return {
			status: "pending",
			me: null,
			bootError: null,
		};
	});
	const [bootPresentation, setBootPresentation] = useState<StartupPresentation>(
		startupSeed?.presentation ?? "cold-init",
	);
	const [isBootstrapping, setIsBootstrapping] = useState(true);

	useEffect(() => {
		let cancelled = false;
		setIsBootstrapping(true);
		void loadAuthSnapshot().then((nextSnapshot) => {
			if (cancelled) return;
			setSnapshot(nextSnapshot);
			setBootPresentation("live");
			setIsBootstrapping(false);
		});

		return () => {
			cancelled = true;
		};
	}, []);

	const refreshAuth = useCallback(async () => {
		setIsBootstrapping(true);
		const nextSnapshot = await loadAuthSnapshot(true);
		setSnapshot(nextSnapshot);
		setBootPresentation("live");
		setIsBootstrapping(false);
		return nextSnapshot.me;
	}, []);

	const value = useMemo<AuthBootstrapValue>(
		() => ({
			...snapshot,
			isAuthenticated:
				snapshot.status === "authenticated" && snapshot.me !== null,
			isBootstrapping,
			bootPresentation,
			refreshAuth,
		}),
		[snapshot, isBootstrapping, bootPresentation, refreshAuth],
	);

	return (
		<AuthBootstrapContext.Provider value={value}>
			{children}
		</AuthBootstrapContext.Provider>
	);
}

export function useAuthBootstrap() {
	const context = useContext(AuthBootstrapContext);
	if (!context) {
		throw new Error(
			"useAuthBootstrap must be used within AuthBootstrapProvider",
		);
	}
	return context;
}
