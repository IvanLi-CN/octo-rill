import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect } from "react";

import { PublicReleaseRepoManagement } from "@/admin/PublicReleaseRepoManagement";

type StoryWindow = Window & {
	__publicReleaseAdminOriginalFetch?: typeof window.fetch;
};

function installPublicReleaseAdminMock() {
	const storyWindow = window as StoryWindow;
	if (!storyWindow.__publicReleaseAdminOriginalFetch) {
		storyWindow.__publicReleaseAdminOriginalFetch = window.fetch.bind(window);
	}
	window.fetch = async (input, init) => {
		const req =
			typeof input === "string" || input instanceof URL
				? new Request(input, init)
				: input;
		const url = new URL(req.url, window.location.origin);
		if (url.pathname.startsWith("/api/admin/public-release-repos")) {
			const deleted = req.method === "DELETE";
			return new Response(
				JSON.stringify({
					items: deleted ? [] : publicReleaseAdminItems,
					page: 1,
					page_size: 100,
					total: deleted ? 0 : publicReleaseAdminItems.length,
					cache_cleanup: deleted
						? {
								repo_id: 14957082,
								full_name: "octo-rill/example",
								deleted_release_count: 24,
								deleted_ai_cache_count: 33,
								skipped_reason: null,
							}
						: null,
				}),
				{
					status: 200,
					headers: { "content-type": "application/json" },
				},
			);
		}
		return storyWindow.__publicReleaseAdminOriginalFetch?.(req) ?? fetch(req);
	};
}

const publicReleaseAdminItems = [
	{
		id: "pubrepo1234567890",
		repo_id: 14957082,
		full_name: "octo-rill/example",
		first_registered_at: "2026-05-04T08:00:00Z",
		last_requested_at: "2026-05-04T08:12:00Z",
		last_list_requested_at: "2026-05-04T08:12:00Z",
		last_detail_requested_at: "2026-05-04T08:10:00Z",
		api_list_requests: 18,
		api_detail_requests: 9,
		page_list_requests: 4,
		page_detail_requests: 6,
		last_sync_status: "ready",
		last_sync_error: null,
		release_count: 24,
		translated_ready_count: 18,
		translated_missing_count: 6,
		polished_ready_count: 15,
		polished_missing_count: 9,
		created_at: "2026-05-04T08:00:00Z",
		updated_at: "2026-05-04T08:12:00Z",
	},
	{
		id: "pubrepo0987654321",
		repo_id: null,
		full_name: "third-party/waiting",
		first_registered_at: "2026-05-04T08:11:00Z",
		last_requested_at: "2026-05-04T08:11:00Z",
		last_list_requested_at: "2026-05-04T08:11:00Z",
		last_detail_requested_at: null,
		api_list_requests: 1,
		api_detail_requests: 0,
		page_list_requests: 2,
		page_detail_requests: 0,
		last_sync_status: "pending",
		last_sync_error: null,
		release_count: 0,
		translated_ready_count: 0,
		translated_missing_count: 0,
		polished_ready_count: 0,
		polished_missing_count: 0,
		created_at: "2026-05-04T08:11:00Z",
		updated_at: "2026-05-04T08:11:00Z",
	},
];

function PublicReleaseRepoManagementStory() {
	installPublicReleaseAdminMock();

	useEffect(() => {
		return () => {
			const storyWindow = window as StoryWindow;
			if (storyWindow.__publicReleaseAdminOriginalFetch) {
				window.fetch = storyWindow.__publicReleaseAdminOriginalFetch;
				delete storyWindow.__publicReleaseAdminOriginalFetch;
			}
		};
	}, []);

	return (
		<div className="min-h-dvh bg-background p-6">
			<PublicReleaseRepoManagement />
		</div>
	);
}

const meta = {
	title: "Admin/PublicReleaseRepoManagement",
	component: PublicReleaseRepoManagementStory,
	parameters: {
		layout: "fullscreen",
	},
} satisfies Meta<typeof PublicReleaseRepoManagementStory>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};
