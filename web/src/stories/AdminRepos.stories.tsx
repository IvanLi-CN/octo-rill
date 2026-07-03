import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useState } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import {
	expect,
	fireEvent,
	fn,
	userEvent,
	waitFor,
	within,
} from "storybook/test";

import type {
	AdminRepoGovernanceListResponse,
	AdminRepoGovernanceOverviewResponse,
	MeResponse,
} from "@/api";
import { AdminReposPage } from "@/pages/AdminReposPage";
import {
	type VersionMonitorValue,
	VersionMonitorStateProvider,
} from "@/version/versionMonitor";

const CURRENT_USER_ID = "admin-repos-story-user";
const STORY_REPO_TOTAL = 10_000;
const STORY_GRID_BUCKET_COUNTS = {
	fresh: 2_600,
	warm: 2_450,
	aging: 2_150,
	stale: 1_700,
	missing: 1_100,
} as const;
const STORY_GRID_TOTAL = Object.values(STORY_GRID_BUCKET_COUNTS).reduce(
	(sum, count) => sum + count,
	0,
);
const STORY_GRID_BUCKET_ORDER = [
	"fresh",
	"warm",
	"aging",
	"stale",
	"missing",
] as const;
const STORY_GRID_BUCKETS = STORY_GRID_BUCKET_ORDER.flatMap((bucket) =>
	Array.from({ length: STORY_GRID_BUCKET_COUNTS[bucket] }, () => bucket),
);

const STORY_VERSION_STATE: VersionMonitorValue = {
	loadedVersion: "v2.9.0",
	availableVersion: null,
	hasUpdate: false,
	hasServiceWorkerUpdate: false,
	canInstallPwa: false,
	isPwaInstalled: false,
	refreshPage: fn(),
	promptInstallPwa: async () => {},
};

const ADMIN_REPOS_LAST_LIST_REQUEST_KEY = "__adminReposLastListRequest";

const STORY_ME: MeResponse = {
	user: {
		id: CURRENT_USER_ID,
		github_user_id: 42,
		login: "storybook-admin",
		name: "Storybook Admin",
		avatar_url: null,
		email: "admin@example.com",
		is_admin: true,
	},
	access_sync: {
		task_id: null,
		task_type: null,
		event_path: null,
		reason: "none",
	},
	dashboard: {
		daily_boundary_local: "08:00",
		daily_boundary_time_zone: "Asia/Shanghai",
		daily_boundary_utc_offset_minutes: 480,
		include_own_releases: false,
	},
};

const governanceOverviewSeed: AdminRepoGovernanceOverviewResponse = {
	summary: {
		dedup_repo_count: STORY_REPO_TOTAL,
		pressure_windows: 8.74,
		last_full_cycle_completed_at: "2026-06-29T09:10:00Z",
	},
	cycle: {
		active_cycle_id: "cycle-2026-06-29T09:20",
		active_cycle_started_at: "2026-06-29T09:20:00Z",
		active_cycle_repo_count: STORY_REPO_TOTAL,
		active_cycle_completed_count: 3_860,
	},
	settings: {
		sync_auto_fetch_interval_minutes: 10,
		retry_recent_failures_interval_minutes: 10,
		repo_release_worker_concurrency: 8,
		repo_refresh_system_budget_per_window: 1000,
		recent_sync_tasks: [
			{
				id: "task-subscription-1010",
				status: "succeeded",
				source: "scheduler",
				duration_ms: 272000,
				created_at: "2026-06-29T10:10:00Z",
				started_at: "2026-06-29T10:10:02Z",
				finished_at: "2026-06-29T10:14:34Z",
			},
		],
	},
	grid_cells: Array.from({ length: STORY_GRID_TOTAL }, (_, index) => {
		const ageBucket = STORY_GRID_BUCKETS[(index * 97) % STORY_GRID_TOTAL];
		return {
			repo_id: index + 1,
			age_bucket: ageBucket,
			band_label: `W${Math.floor(index / 1000) + 1}`,
			urgency_score:
				ageBucket === "fresh"
					? 0.82
					: ageBucket === "warm"
						? 1.24
						: ageBucket === "aging"
							? 1.98
							: ageBucket === "stale"
								? 3.14
								: 4,
			system_attempt_status:
				index > 0 && index % 37 === 0
					? "failed"
					: index % 11 === 0
						? "succeeded"
						: null,
		};
	}),
};

const governanceListSeed: AdminRepoGovernanceListResponse = {
	page: 1,
	page_size: 60,
	total: STORY_REPO_TOTAL,
	target_window_options: [
		{ target_window: 1, repo_count: 4_100 },
		{ target_window: 2, repo_count: 2_900 },
		{ target_window: 3, repo_count: 1_800 },
		{ target_window: 4, repo_count: 1_200 },
	],
	items: [
		{
			repo_id: 1,
			repo_full_name: "octo-rill/octo-rill",
			watcher_user_count: 88,
			watcher_repo_total_sum: 164,
			cached_stargazer_count: 932,
			priority_rank: 1,
			target_window: 1,
			target_interval_minutes: 10,
			urgency_score: 3.72,
			urgency_bucket: "critical",
			system_last_selected_at: "2026-06-29T10:10:01Z",
			system_last_success_at: "2026-06-29T08:30:00Z",
			system_last_attempt_at: "2026-06-29T10:11:12Z",
			system_last_attempt_status: "succeeded",
			system_last_attempt_error: null,
			actual_last_success_at: "2026-06-29T09:58:00Z",
			actual_last_success_source: "interactive",
		},
		{
			repo_id: 2,
			repo_full_name: "openai/openai-openapi",
			watcher_user_count: 74,
			watcher_repo_total_sum: 152,
			cached_stargazer_count: 476,
			priority_rank: 2,
			target_window: 1,
			target_interval_minutes: 10,
			urgency_score: 2.41,
			urgency_bucket: "due",
			system_last_selected_at: "2026-06-29T10:10:01Z",
			system_last_success_at: "2026-06-29T09:10:00Z",
			system_last_attempt_at: "2026-06-29T10:12:00Z",
			system_last_attempt_status: "failed",
			system_last_attempt_error: "github returned 404 Not Found",
			actual_last_success_at: "2026-06-29T09:10:00Z",
			actual_last_success_source: "system",
		},
		{
			repo_id: 3,
			repo_full_name: "microsoft/typescript",
			watcher_user_count: 63,
			watcher_repo_total_sum: 150,
			cached_stargazer_count: 1200,
			priority_rank: 3,
			target_window: 1,
			target_interval_minutes: 10,
			urgency_score: 1.12,
			urgency_bucket: "healthy",
			system_last_selected_at: "2026-06-29T10:00:00Z",
			system_last_success_at: "2026-06-29T09:56:00Z",
			system_last_attempt_at: "2026-06-29T10:00:22Z",
			system_last_attempt_status: "succeeded",
			system_last_attempt_error: null,
			actual_last_success_at: "2026-06-29T09:56:00Z",
			actual_last_success_source: "system",
		},
		{
			repo_id: 4,
			repo_full_name: "vercel/next.js",
			watcher_user_count: 58,
			watcher_repo_total_sum: 177,
			cached_stargazer_count: null,
			priority_rank: 41,
			target_window: 1,
			target_interval_minutes: 10,
			urgency_score: 4,
			urgency_bucket: "critical",
			system_last_selected_at: null,
			system_last_success_at: null,
			system_last_attempt_at: null,
			system_last_attempt_status: null,
			system_last_attempt_error: null,
			actual_last_success_at: null,
			actual_last_success_source: null,
		},
		{
			repo_id: 5,
			repo_full_name: "redis/redis",
			watcher_user_count: 49,
			watcher_repo_total_sum: 141,
			cached_stargazer_count: 990,
			priority_rank: 126,
			target_window: 2,
			target_interval_minutes: 20,
			urgency_score: 1.84,
			urgency_bucket: "due",
			system_last_selected_at: "2026-06-29T09:50:00Z",
			system_last_success_at: "2026-06-29T09:18:00Z",
			system_last_attempt_at: "2026-06-29T09:51:19Z",
			system_last_attempt_status: "succeeded",
			system_last_attempt_error: null,
			actual_last_success_at: "2026-06-29T09:18:00Z",
			actual_last_success_source: "system",
		},
	],
};

type AdminReposPreviewProps = {
	listMode?: "all" | "stale" | "missing";
};

function buildListResponse(mode: AdminReposPreviewProps["listMode"]) {
	if (mode === "stale") {
		return {
			...governanceListSeed,
			total: 4,
			items: governanceListSeed.items.filter((item) =>
				["critical", "due"].includes(item.urgency_bucket),
			),
		};
	}
	if (mode === "missing") {
		return {
			...governanceListSeed,
			total: 1,
			items: governanceListSeed.items.filter(
				(item) => item.actual_last_success_at === null,
			),
		};
	}
	return governanceListSeed;
}

function rememberAdminReposListRequest(search: string) {
	window.sessionStorage.setItem(ADMIN_REPOS_LAST_LIST_REQUEST_KEY, search);
}

function readAdminReposListRequest() {
	return window.sessionStorage.getItem(ADMIN_REPOS_LAST_LIST_REQUEST_KEY) ?? "";
}

function AdminReposPreview(props: AdminReposPreviewProps) {
	const { listMode = "all" } = props;
	const [ready, setReady] = useState(false);

	useEffect(() => {
		const originalFetch = window.fetch.bind(window);
		let overview = {
			...governanceOverviewSeed,
			settings: { ...governanceOverviewSeed.settings },
		};

		window.fetch = async (input, init) => {
			const req =
				typeof input === "string" || input instanceof URL
					? new Request(input, init)
					: input;
			const url = new URL(req.url, window.location.origin);

			if (url.pathname === "/api/me" && req.method === "GET") {
				return new Response(JSON.stringify(STORY_ME), {
					status: 200,
					headers: { "content-type": "application/json" },
				});
			}

			if (
				url.pathname === "/api/admin/repos/overview" &&
				req.method === "GET"
			) {
				return new Response(JSON.stringify(overview), {
					status: 200,
					headers: { "content-type": "application/json" },
				});
			}

			if (url.pathname === "/api/admin/repos" && req.method === "GET") {
				rememberAdminReposListRequest(url.search);
				const aging = url.searchParams.get("aging");
				const query = (url.searchParams.get("query") ?? "").toLowerCase();
				const targetWindows = (url.searchParams.get("target_windows") ?? "")
					.split(",")
					.map((value) => Number(value))
					.filter((value) => Number.isFinite(value) && value > 0);
				const urgencyMin = Number(url.searchParams.get("urgency_min") ?? "0");
				const urgencyMax = Number(url.searchParams.get("urgency_max") ?? "4");
				const seeded = buildListResponse(
					aging === "stale" || aging === "missing" ? aging : listMode,
				);
				const items = seeded.items.filter((item) => {
					if (query && !item.repo_full_name.toLowerCase().includes(query)) {
						return false;
					}
					if (
						targetWindows.length > 0 &&
						!targetWindows.includes(item.target_window)
					) {
						return false;
					}
					return (
						item.urgency_score >= urgencyMin && item.urgency_score <= urgencyMax
					);
				});
				return new Response(
					JSON.stringify({
						...seeded,
						total: items.length,
						items,
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}

			if (
				url.pathname === "/api/admin/jobs/sync/runtime-config" &&
				req.method === "PATCH"
			) {
				const body = (await req.json()) as {
					sync_auto_fetch_interval_minutes?: number;
					repo_refresh_system_budget_per_window?: number;
				};
				overview = {
					...overview,
					settings: {
						...overview.settings,
						sync_auto_fetch_interval_minutes: Number(
							body.sync_auto_fetch_interval_minutes ??
								overview.settings.sync_auto_fetch_interval_minutes,
						),
						repo_refresh_system_budget_per_window: Number(
							body.repo_refresh_system_budget_per_window ??
								overview.settings.repo_refresh_system_budget_per_window,
						),
					},
				};
				return new Response(JSON.stringify(overview.settings), {
					status: 200,
					headers: { "content-type": "application/json" },
				});
			}

			if (url.pathname === "/api/version" || url.pathname === "/api/health") {
				return new Response(
					JSON.stringify({
						ok: true,
						version: "2.9.0",
						source: "storybook",
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}

			return originalFetch(input, init);
		};

		setReady(true);
		return () => {
			window.fetch = originalFetch;
		};
	}, [listMode]);

	if (!ready) return null;

	return (
		<VersionMonitorStateProvider value={STORY_VERSION_STATE}>
			<AdminReposPage me={STORY_ME} />
		</VersionMonitorStateProvider>
	);
}

const meta = {
	title: "Admin/Admin Repos",
	component: AdminReposPreview,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		viewport: {
			options: {
				...INITIAL_VIEWPORTS,
				adminReposNarrowTablet: {
					name: "Admin repos narrow tablet 768x1180",
					styles: {
						width: "768px",
						height: "1180px",
					},
					type: "tablet",
				},
			},
		},
		docs: {
			description: {
				component:
					"`/admin/repos` 的稳定治理场景，覆盖有效关注池 summary、预算入口提示、活动图和按迫切值排序的仓库明细。",
			},
		},
	},
	args: {
		listMode: "all",
	},
} satisfies Meta<typeof AdminReposPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const EvidenceDesktop: Story = {
	name: "Evidence / Desktop Governance",
	render: () => <AdminReposPreview listMode="all" />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("仓库刷新治理")).toBeVisible();
		await expect(canvas.getByText("去重仓库数")).toBeVisible();
		await expect(canvas.getByText("压力值")).toBeVisible();
		await expect(canvas.getByText("上次全量闭环")).toBeVisible();
		await expect(canvas.getByText("4 小时内 · 2600")).toBeVisible();
		await expect(canvas.getByText("10000")).toBeVisible();
		await expect(
			canvas.getByText(
				"颜色看实际刷新时间；W* 看系统软目标窗口；系统尝试看本轮 system 账本；迫切值大于 1 表示已超出软目标。",
			),
		).toBeVisible();
		await expect(canvas.getAllByText("系统尝试成功")[0]).toBeVisible();
		await expect(canvas.getByText("系统尝试失败")).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: /打开订阅同步设置，当前系统预算/i }),
		).toBeVisible();
		await expect(canvas.getByText("octo-rill/octo-rill")).toBeVisible();
		await expect(canvas.getByText("openai/openai-openapi")).toBeVisible();
		await expect(canvas.getByText("microsoft/typescript")).toBeVisible();
		const body = within(document.body);
		await userEvent.click(
			canvas.getByRole("combobox", { name: "刷新状态筛选，当前 全部" }),
		);
		await userEvent.click(
			await body.findByRole("option", { name: "仅未成功" }),
		);
		await waitFor(() => {
			const request = readAdminReposListRequest();
			expect(request).toContain("aging=missing");
		});
		await userEvent.click(
			canvas.getByRole("combobox", {
				name: "刷新状态筛选，当前 仅未成功",
			}),
		);
		await userEvent.click(await body.findByRole("option", { name: "全部" }));
		await userEvent.click(
			canvas.getByRole("button", { name: /目标窗口筛选/i }),
		);
		await userEvent.click(await body.findByLabelText(/W2/i));
		await expect(canvas.getByText("redis/redis")).toBeVisible();
		await userEvent.click(
			canvas.getByRole("button", { name: /迫切值范围筛选/i }),
		);
		const minSlider = await body.findByLabelText("迫切值下限");
		fireEvent.change(minSlider, { target: { value: "1.8" } });
		await waitFor(() => {
			const request = readAdminReposListRequest();
			expect(request).toContain("target_windows=2");
			expect(request).toContain("urgency_min=1.8");
		});
	},
	parameters: {
		docs: {
			description: {
				story:
					"桌面证据：以 10000 仓库治理池验证 `/admin/repos` 的 summary、预算入口、活动图和高优先级仓库明细在高负载场景下仍保持稳定。",
			},
		},
	},
};

export const EvidenceNarrowTablet: Story = {
	name: "Evidence / Narrow Tablet Governance",
	render: () => <AdminReposPreview listMode="stale" />,
	globals: {
		viewport: {
			value: "adminReposNarrowTablet",
			isRotated: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("仓库刷新治理")).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: /打开订阅同步设置，当前系统预算/i }),
		).toBeVisible();
		await expect(canvas.getByText("总计 10000 个仓库")).toBeVisible();
		await expect(canvas.getByText("仓库明细")).toBeVisible();
		await expect(canvas.getByPlaceholderText("搜索仓库全名")).toBeVisible();
		await expect(canvas.getByText("openai/openai-openapi")).toBeVisible();
		await expect(canvas.getByText("vercel/next.js")).toBeVisible();
	},
	parameters: {
		docs: {
			description: {
				story:
					"窄屏证据：以 10000 仓库治理池验证 summary、预算入口、活动图和明细列表在窄平板下仍保持单页稳定布局。",
			},
		},
	},
};
