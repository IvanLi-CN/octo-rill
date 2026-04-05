export const mockDashboardBootstrap = {
	daily_boundary_local: "08:00",
	daily_boundary_time_zone: "Asia/Shanghai",
	daily_boundary_utc_offset_minutes: 480,
};

type MockUser = {
	id: string;
	github_user_id: number;
	login: string;
	name: string | null;
	avatar_url: string | null;
	email: string | null;
	is_admin: boolean;
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
	},
) {
	return {
		user,
		dashboard: mockDashboardBootstrap,
		...(options?.access_sync ? { access_sync: options.access_sync } : {}),
	};
}
