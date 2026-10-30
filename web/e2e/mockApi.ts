import type { Route } from "@playwright/test";

export const mockDashboardBootstrap = {
	daily_boundary_local: "00:00",
	daily_boundary_time_zone: "Asia/Shanghai",
	daily_boundary_utc_offset_minutes: 480,
};

export function json(route: Route, payload: unknown, status = 200) {
	const request = route.request();
	const errorPayload =
		typeof payload === "object" && payload !== null && "error" in payload
			? (payload as { error?: { code?: string; message?: string } }).error
			: undefined;
	const isUnhandledReadableFeed =
		request.method() === "GET" &&
		new URL(request.url()).pathname === "/api/dashboard/feed" &&
		status === 404 &&
		errorPayload?.code === "not_found" &&
		errorPayload.message === "unhandled GET /api/dashboard/feed";
	const responsePayload = isUnhandledReadableFeed
		? {
				error: {
					code: "release_reading_evidence_unavailable",
					message: "Release compare evidence is not ready",
				},
			}
		: payload;
	return route.fulfill({
		status: isUnhandledReadableFeed ? 503 : status,
		contentType: "application/json",
		body: JSON.stringify(responsePayload),
	});
}

export function mockUnavailableDashboardReadableFeed(route: Route) {
	return json(
		route,
		{
			error: {
				code: "release_reading_evidence_unavailable",
				message: "Release compare evidence is not ready",
			},
		},
		503,
	);
}

type MockUser = {
	id: string;
	github_user_id: number;
	login: string;
	name: string | null;
	avatar_url: string | null;
	email: string | null;
	is_admin: boolean;
	account_status?: "enabled" | "paused" | "disabled";
	paused_at?: string | null;
};

type MockAccessSync = {
	task_id: string | null;
	task_type: string | null;
	event_path: string | null;
	reason: "first_visit" | "inactive_over_1h" | "reused_inflight" | "none";
};

export function buildMockMeResponse(
	user: MockUser,
	options?: {
		access_sync?: MockAccessSync;
		include_own_releases?: boolean;
	},
) {
	return {
		user,
		dashboard: {
			...mockDashboardBootstrap,
			include_own_releases: options?.include_own_releases ?? false,
		},
		...(options?.access_sync ? { access_sync: options.access_sync } : {}),
	};
}
