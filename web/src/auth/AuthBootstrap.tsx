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
import { useDemoSnapshot } from "@/demo/runtime";
import {
	describeNetworkAwareError,
	type NetworkErrorKind,
} from "@/lib/errorPresentation";

export type AuthBootstrapStatus =
	| "pending"
	| "anonymous"
	| "paused"
	| "authenticated";

type AuthSnapshot = {
	status: AuthBootstrapStatus;
	me: MeResponse | null;
	bootError: string | null;
	bootErrorKind: NetworkErrorKind | null;
	bootErrorDetail: string | null;
};

export type AuthBootstrapValue = AuthSnapshot & {
	isAuthenticated: boolean;
	isBootstrapping: boolean;
	bootPresentation: StartupPresentation;
	refreshAuth: () => Promise<MeResponse | null>;
};

const AuthBootstrapContext = createContext<AuthBootstrapValue | null>(null);

const startupSeed = readStartupPresentationSeed();
const warmStartupSeed =
	startupSeed?.presentation === "warm-cache" ? startupSeed : null;
const canReuseStartupSeedOnTransientError = warmStartupSeed !== null;

let cachedSnapshot: AuthSnapshot | null = warmStartupSeed
	? {
			status: "authenticated",
			me: warmStartupSeed.me,
			bootError: null,
			bootErrorKind: null,
			bootErrorDetail: null,
		}
	: null;
let cachedSnapshotOrigin: "seed" | "network" | "none" = warmStartupSeed
	? "seed"
	: "none";
let inflightSnapshotPromise: Promise<AuthSnapshot> | null = null;

function buildSnapshotFromDemo(me: MeResponse | null): AuthSnapshot {
	if (me?.user.account_status === "paused") {
		return {
			status: "paused",
			me,
			bootError: null,
			bootErrorKind: null,
			bootErrorDetail: null,
		};
	}
	if (!me) {
		return {
			status: "anonymous",
			me: null,
			bootError: null,
			bootErrorKind: null,
			bootErrorDetail: null,
		};
	}
	return {
		status: "authenticated",
		me,
		bootError: null,
		bootErrorKind: null,
		bootErrorDetail: null,
	};
}

function resolveInitialAuthSnapshot(
	demoSnapshot: ReturnType<typeof useDemoSnapshot>,
): AuthSnapshot {
	if (demoSnapshot.active) {
		return buildSnapshotFromDemo(demoSnapshot.model?.me ?? null);
	}
	if (warmStartupSeed) {
		return {
			status: "authenticated",
			me: warmStartupSeed.me,
			bootError: null,
			bootErrorKind: null,
			bootErrorDetail: null,
		};
	}
	return {
		status: "pending",
		me: null,
		bootError: null,
		bootErrorKind: null,
		bootErrorDetail: null,
	};
}

async function requestAuthSnapshot(): Promise<AuthSnapshot> {
	try {
		const me = await apiGet<MeResponse>("/api/me");
		const previousUserId =
			cachedSnapshot?.me?.user.id ?? startupSeed?.me.user.id;
		if (previousUserId && previousUserId !== me.user.id) {
			clearAllWarmStartupCaches();
		}
		if (me.user.account_status === "paused") {
			clearAllWarmStartupCaches();
			return {
				status: "paused",
				me,
				bootError: null,
				bootErrorKind: null,
				bootErrorDetail: null,
			};
		}
		persistAuthenticatedStartup(me);
		return {
			status: "authenticated",
			me,
			bootError: null,
			bootErrorKind: null,
			bootErrorDetail: null,
		};
	} catch (err) {
		if (err instanceof ApiError && err.status === 401) {
			clearAllWarmStartupCaches();
			return {
				status: "anonymous",
				me: null,
				bootError: null,
				bootErrorKind: null,
				bootErrorDetail: null,
			};
		}
		const bootError = describeNetworkAwareError(
			err,
			"登录状态检查失败，请稍后重试。",
		);

		const canReuseCachedAuth =
			cachedSnapshot?.status === "authenticated" &&
			cachedSnapshot.me &&
			(cachedSnapshotOrigin === "network" ||
				(cachedSnapshotOrigin === "seed" &&
					canReuseStartupSeedOnTransientError));
		const cachedAuthenticatedMe =
			canReuseCachedAuth && cachedSnapshot?.me ? cachedSnapshot.me : null;

		if (cachedAuthenticatedMe) {
			return {
				status: "authenticated",
				me: cachedAuthenticatedMe,
				bootError: bootError.message,
				bootErrorKind: bootError.kind,
				bootErrorDetail: bootError.detail,
			};
		}

		return {
			status: "anonymous",
			me: null,
			bootError: bootError.message,
			bootErrorKind: bootError.kind,
			bootErrorDetail: bootError.detail,
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
	const demoSnapshot = useDemoSnapshot();
	const [snapshot, setSnapshot] = useState<AuthSnapshot>(() =>
		resolveInitialAuthSnapshot(demoSnapshot),
	);
	const [bootPresentation, setBootPresentation] = useState<StartupPresentation>(
		demoSnapshot.active ? "live" : (startupSeed?.presentation ?? "cold-init"),
	);
	const [isBootstrapping, setIsBootstrapping] = useState(!demoSnapshot.active);

	useEffect(() => {
		if (demoSnapshot.active) {
			const nextSnapshot = buildSnapshotFromDemo(
				demoSnapshot.model?.me ?? null,
			);
			inflightSnapshotPromise = null;
			setSnapshot(nextSnapshot);
			setBootPresentation("live");
			setIsBootstrapping(false);
			return;
		}

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
	}, [demoSnapshot.active, demoSnapshot.model?.me]);

	const refreshAuth = useCallback(async () => {
		if (demoSnapshot.active) {
			const nextSnapshot = buildSnapshotFromDemo(
				demoSnapshot.model?.me ?? null,
			);
			inflightSnapshotPromise = null;
			setSnapshot(nextSnapshot);
			setBootPresentation("live");
			setIsBootstrapping(false);
			return nextSnapshot.me;
		}

		setIsBootstrapping(true);
		const nextSnapshot = await loadAuthSnapshot(true);
		setSnapshot(nextSnapshot);
		setBootPresentation("live");
		setIsBootstrapping(false);
		return nextSnapshot.me;
	}, [demoSnapshot.active, demoSnapshot.model?.me]);

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
