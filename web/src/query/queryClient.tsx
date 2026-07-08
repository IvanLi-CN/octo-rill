import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { PersistQueryClientProvider } from "@tanstack/react-query-persist-client";
import { createSyncStoragePersister } from "@tanstack/query-sync-storage-persister";
import type { ReactNode } from "react";

import {
	DASHBOARD_QUERY_MAX_AGE_MS,
	DASHBOARD_QUERY_STALE_MS,
	shouldPersistDashboardQuery,
} from "@/query/dashboardQueryKeys";

const DASHBOARD_QUERY_PERSIST_KEY = "octo-rill.dashboard-query-cache.v1";

function createDashboardStoragePersister() {
	if (typeof window === "undefined") {
		return null;
	}
	return createSyncStoragePersister({
		key: DASHBOARD_QUERY_PERSIST_KEY,
		storage: window.localStorage,
	});
}

export const queryClient = new QueryClient({
	defaultOptions: {
		queries: {
			staleTime: DASHBOARD_QUERY_STALE_MS,
			gcTime: DASHBOARD_QUERY_MAX_AGE_MS,
			refetchOnWindowFocus: false,
			retry: 1,
		},
	},
});

const dashboardPersister = createDashboardStoragePersister();

export function clearDashboardQueryCache() {
	queryClient.removeQueries({ queryKey: ["dashboard"] });
	if (dashboardPersister) {
		void dashboardPersister.removeClient();
	}
	if (typeof window !== "undefined") {
		window.localStorage.removeItem(DASHBOARD_QUERY_PERSIST_KEY);
	}
}

export function AppQueryProvider(props: { children: ReactNode }) {
	const { children } = props;

	if (!dashboardPersister) {
		return (
			<QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
		);
	}

	return (
		<PersistQueryClientProvider
			client={queryClient}
			persistOptions={{
				persister: dashboardPersister,
				maxAge: DASHBOARD_QUERY_MAX_AGE_MS,
				buster: "dashboard-query-cache-v1",
				dehydrateOptions: {
					shouldDehydrateQuery: (query) =>
						query.state.status === "success" &&
						shouldPersistDashboardQuery(query.queryKey),
				},
			}}
		>
			{children}
		</PersistQueryClientProvider>
	);
}
