import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useState } from "react";
import { expect, waitFor, within } from "storybook/test";

import { PublicReleaseRepoManagement } from "@/admin/PublicReleaseRepoManagement";

type StoryMode =
	| "ready"
	| "initial-loading"
	| "refreshing"
	| "empty"
	| "blocking-error";

type StoryWindow = Window & {
	__publicReleaseAdminOriginalFetch?: typeof window.fetch;
};

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

async function sleep(ms: number) {
	await new Promise((resolve) => window.setTimeout(resolve, ms));
}

function installPublicReleaseAdminMock(mode: StoryMode) {
	const storyWindow = window as StoryWindow;
	if (!storyWindow.__publicReleaseAdminOriginalFetch) {
		storyWindow.__publicReleaseAdminOriginalFetch = window.fetch.bind(window);
	}
	let requestCount = 0;

	window.fetch = async (input, init) => {
		const req =
			typeof input === "string" || input instanceof URL
				? new Request(input, init)
				: input;
		const url = new URL(req.url, window.location.origin);
		if (url.pathname.startsWith("/api/admin/public-release-repos")) {
			requestCount += 1;
			if (mode === "initial-loading") {
				await sleep(60_000);
			}
			if (mode === "blocking-error") {
				return new Response(
					JSON.stringify({
						ok: false,
						error: {
							code: "admin_public_release_repos_failed",
							message: "公开仓库登记列表加载失败",
						},
					}),
					{
						status: 500,
						headers: { "content-type": "application/json" },
					},
				);
			}
			if (mode === "refreshing" && requestCount >= 2) {
				await sleep(700);
			}
			const items = mode === "empty" ? [] : publicReleaseAdminItems;
			return new Response(
				JSON.stringify({
					items,
					page: 1,
					page_size: 100,
					total: items.length,
					cache_cleanup: null,
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

function PublicReleaseRepoManagementStory(props: { mode: StoryMode }) {
	const { mode } = props;
	const [ready, setReady] = useState(false);
	const [refreshKey, setRefreshKey] = useState(0);

	useEffect(() => {
		installPublicReleaseAdminMock(mode);
		setReady(true);

		if (mode === "refreshing") {
			const timer = window.setTimeout(() => {
				const input = document.querySelector<HTMLInputElement>(
					'input[placeholder="搜索 owner/repo"]',
				);
				input?.focus();
				input?.setSelectionRange(0, 0);
				setRefreshKey((current) => current + 1);
				const refreshButton = Array.from(
					document.querySelectorAll<HTMLButtonElement>("button"),
				).find((button) => button.textContent?.includes("刷新"));
				refreshButton?.click();
			}, 150);
			return () => {
				window.clearTimeout(timer);
				const storyWindow = window as StoryWindow;
				if (storyWindow.__publicReleaseAdminOriginalFetch) {
					window.fetch = storyWindow.__publicReleaseAdminOriginalFetch;
					delete storyWindow.__publicReleaseAdminOriginalFetch;
				}
			};
		}

		return () => {
			const storyWindow = window as StoryWindow;
			if (storyWindow.__publicReleaseAdminOriginalFetch) {
				window.fetch = storyWindow.__publicReleaseAdminOriginalFetch;
				delete storyWindow.__publicReleaseAdminOriginalFetch;
			}
		};
	}, [mode]);

	if (!ready) {
		return null;
	}

	return (
		<div className="min-h-dvh bg-background p-6">
			<div data-story-refresh-key={refreshKey} className="sr-only" />
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
	args: {
		mode: "ready",
	},
} satisfies Meta<typeof PublicReleaseRepoManagementStory>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Ready: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("octo-rill/example")).toBeVisible();
		await waitFor(() =>
			expect(
				canvasElement.querySelector('[data-list-state="ready"]'),
			).not.toBeNull(),
		);
	},
};

export const InitialLoading: Story = {
	args: {
		mode: "initial-loading",
	},
	play: async ({ canvasElement }) => {
		await waitFor(() =>
			expect(
				canvasElement.querySelector('[data-list-state="initial-loading"]'),
			).not.toBeNull(),
		);
	},
};

export const Refreshing: Story = {
	args: {
		mode: "refreshing",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("octo-rill/example")).toBeVisible();
		await waitFor(() =>
			expect(
				canvasElement.querySelector('[data-list-refreshing="true"]'),
			).not.toBeNull(),
		);
		await expect(
			canvas.getByText("登记仓库更新中...", { exact: true }),
		).toBeVisible();
	},
};

export const Empty: Story = {
	args: {
		mode: "empty",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("还没有公开端点登记仓库")).toBeVisible();
		await waitFor(() =>
			expect(
				canvasElement.querySelector('[data-list-empty-state="true"]'),
			).not.toBeNull(),
		);
	},
};

export const BlockingError: Story = {
	args: {
		mode: "blocking-error",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("公开仓库登记列表加载失败", { exact: true }),
		).toBeVisible();
		await waitFor(() =>
			expect(
				canvasElement.querySelector('[data-list-blocking-error="true"]'),
			).not.toBeNull(),
		);
	},
};

export const EvidenceFiveStates: Story = {
	name: "Evidence / Ready",
	args: {
		mode: "ready",
	},
	parameters: {
		docs: {
			description: {
				story:
					"后台公开仓库列表统一状态 contract 的基线证据：ready 态保留真实表格排版，配合 sibling stories 覆盖 skeleton / refreshing / empty / blocking-error。",
			},
		},
	},
	play: Ready.play,
};
