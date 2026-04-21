import { useEffect, useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import type { AdminDashboardWindowValue } from "@/api";
import { AdminDashboardPage } from "@/pages/AdminDashboardPage";

type PreviewVariant = "default" | "busy" | "quiet" | "empty" | "error";

function buildTrendPoints(
	window: AdminDashboardWindowValue,
	buildValue: (index: number) => {
		activeUsers: number;
		totalUsers: number;
		translationsTotal: number;
		translationsFailed: number;
		summariesTotal: number;
		summariesFailed: number;
		briefsTotal: number;
		briefsFailed: number;
	},
) {
	const days = window === "30d" ? 30 : 7;
	const start = new Date(Date.UTC(2026, 3, 18));
	start.setUTCDate(start.getUTCDate() - (days - 1));

	return Array.from({ length: days }, (_, index) => {
		const date = new Date(start);
		date.setUTCDate(start.getUTCDate() + index);
		const dateText = date.toISOString().slice(0, 10);
		const value = buildValue(index);
		return {
			date: dateText,
			label: dateText.slice(5),
			total_users: value.totalUsers,
			active_users: value.activeUsers,
			translations_total: value.translationsTotal,
			translations_failed: value.translationsFailed,
			summaries_total: value.summariesTotal,
			summaries_failed: value.summariesFailed,
			briefs_total: value.briefsTotal,
			briefs_failed: value.briefsFailed,
		};
	});
}

function buildPayload(
	variant: PreviewVariant,
	window: AdminDashboardWindowValue,
) {
	if (variant === "error") {
		return null;
	}

	if (variant === "busy") {
		return {
			generated_at: "2026-04-18T09:12:00Z",
			time_zone: "Asia/Shanghai",
			summary: {
				total_users: 184,
				active_users_today: 61,
				ongoing_tasks_total: 14,
				queued_tasks: 8,
				running_tasks: 6,
				ongoing_by_task: { translations: 6, summaries: 5, briefs: 3 },
			},
			today_live: {
				date: "2026-04-18",
				total_users: 184,
				active_users: 61,
				ongoing_tasks_total: 14,
				queued_tasks: 8,
				running_tasks: 6,
			},
			status_breakdown: {
				queued_total: 8,
				running_total: 6,
				succeeded_total: 67,
				failed_total: 5,
				canceled_total: 1,
				total: 87,
				items: [
					{
						task_type: "translate.release.batch",
						label: "翻译",
						queued: 3,
						running: 3,
						succeeded: 28,
						failed: 3,
						canceled: 0,
						total: 37,
						success_rate: 0.9,
					},
					{
						task_type: "summarize.release.smart.batch",
						label: "润色",
						queued: 3,
						running: 2,
						succeeded: 24,
						failed: 1,
						canceled: 1,
						total: 31,
						success_rate: 0.923,
					},
					{
						task_type: "brief.daily_slot",
						label: "日报",
						queued: 2,
						running: 1,
						succeeded: 15,
						failed: 1,
						canceled: 0,
						total: 19,
						success_rate: 0.938,
					},
				],
			},
			task_share: [
				{
					task_type: "translate.release.batch",
					label: "翻译",
					total: 37,
					share_ratio: 37 / 87,
					success_rate: 0.9,
				},
				{
					task_type: "summarize.release.smart.batch",
					label: "润色",
					total: 31,
					share_ratio: 31 / 87,
					success_rate: 0.923,
				},
				{
					task_type: "brief.daily_slot",
					label: "日报",
					total: 19,
					share_ratio: 19 / 87,
					success_rate: 0.938,
				},
			],
			trend_points: buildTrendPoints(window, (index) => ({
				activeUsers: 22 + index * (window === "30d" ? 1 : 3),
				totalUsers: 150 + index,
				translationsTotal: 10 + Math.floor(index * 1.3),
				translationsFailed: index % 6 === 0 ? 2 : 1,
				summariesTotal: 8 + Math.floor(index * 1.1),
				summariesFailed: index % 9 === 0 ? 1 : 0,
				briefsTotal: 7 + (index % 8),
				briefsFailed: index % 13 === 0 ? 1 : 0,
			})),
			window_meta: {
				selected_window: window,
				available_windows: ["7d", "30d"],
				window_start: window === "30d" ? "2026-03-20" : "2026-04-12",
				window_end: "2026-04-18",
				point_count: window === "30d" ? 30 : 7,
			},
		};
	}

	if (variant === "quiet") {
		return {
			generated_at: "2026-04-18T09:12:00Z",
			time_zone: "Asia/Shanghai",
			summary: {
				total_users: 184,
				active_users_today: 17,
				ongoing_tasks_total: 2,
				queued_tasks: 1,
				running_tasks: 1,
				ongoing_by_task: { translations: 0, summaries: 1, briefs: 1 },
			},
			today_live: {
				date: "2026-04-18",
				total_users: 184,
				active_users: 17,
				ongoing_tasks_total: 2,
				queued_tasks: 1,
				running_tasks: 1,
			},
			status_breakdown: {
				queued_total: 1,
				running_total: 1,
				succeeded_total: 18,
				failed_total: 0,
				canceled_total: 0,
				total: 20,
				items: [
					{
						task_type: "translate.release.batch",
						label: "翻译",
						queued: 0,
						running: 0,
						succeeded: 6,
						failed: 0,
						canceled: 0,
						total: 6,
						success_rate: 1,
					},
					{
						task_type: "summarize.release.smart.batch",
						label: "润色",
						queued: 1,
						running: 1,
						succeeded: 5,
						failed: 0,
						canceled: 0,
						total: 7,
						success_rate: 1,
					},
					{
						task_type: "brief.daily_slot",
						label: "日报",
						queued: 0,
						running: 0,
						succeeded: 7,
						failed: 0,
						canceled: 0,
						total: 7,
						success_rate: 1,
					},
				],
			},
			task_share: [
				{
					task_type: "translate.release.batch",
					label: "翻译",
					total: 6,
					share_ratio: 0.3,
					success_rate: 1,
				},
				{
					task_type: "summarize.release.smart.batch",
					label: "润色",
					total: 7,
					share_ratio: 0.35,
					success_rate: 1,
				},
				{
					task_type: "brief.daily_slot",
					label: "日报",
					total: 7,
					share_ratio: 0.35,
					success_rate: 1,
				},
			],
			trend_points: buildTrendPoints(window, (index) => ({
				activeUsers: 9 + (index % 9),
				totalUsers: 184,
				translationsTotal: 2 + (index % 4),
				translationsFailed: 0,
				summariesTotal: 3 + (index % 4),
				summariesFailed: 0,
				briefsTotal: 4 + (index % 3),
				briefsFailed: 0,
			})),
			window_meta: {
				selected_window: window,
				available_windows: ["7d", "30d"],
				window_start: window === "30d" ? "2026-03-20" : "2026-04-12",
				window_end: "2026-04-18",
				point_count: window === "30d" ? 30 : 7,
			},
		};
	}

	if (variant === "empty") {
		return {
			generated_at: "2026-04-18T09:12:00Z",
			time_zone: "Asia/Shanghai",
			summary: {
				total_users: 12,
				active_users_today: 0,
				ongoing_tasks_total: 0,
				queued_tasks: 0,
				running_tasks: 0,
				ongoing_by_task: { translations: 0, summaries: 0, briefs: 0 },
			},
			today_live: {
				date: "2026-04-18",
				total_users: 12,
				active_users: 0,
				ongoing_tasks_total: 0,
				queued_tasks: 0,
				running_tasks: 0,
			},
			status_breakdown: {
				queued_total: 0,
				running_total: 0,
				succeeded_total: 0,
				failed_total: 0,
				canceled_total: 0,
				total: 0,
				items: [
					{
						task_type: "translate.release.batch",
						label: "翻译",
						queued: 0,
						running: 0,
						succeeded: 0,
						failed: 0,
						canceled: 0,
						total: 0,
						success_rate: 0,
					},
					{
						task_type: "summarize.release.smart.batch",
						label: "润色",
						queued: 0,
						running: 0,
						succeeded: 0,
						failed: 0,
						canceled: 0,
						total: 0,
						success_rate: 0,
					},
					{
						task_type: "brief.daily_slot",
						label: "日报",
						queued: 0,
						running: 0,
						succeeded: 0,
						failed: 0,
						canceled: 0,
						total: 0,
						success_rate: 0,
					},
				],
			},
			task_share: [],
			trend_points: buildTrendPoints(window, () => ({
				activeUsers: 0,
				totalUsers: 12,
				translationsTotal: 0,
				translationsFailed: 0,
				summariesTotal: 0,
				summariesFailed: 0,
				briefsTotal: 0,
				briefsFailed: 0,
			})),
			window_meta: {
				selected_window: window,
				available_windows: ["7d", "30d"],
				window_start: window === "30d" ? "2026-03-20" : "2026-04-12",
				window_end: "2026-04-18",
				point_count: window === "30d" ? 30 : 7,
			},
		};
	}

	return {
		generated_at: "2026-04-18T09:12:00Z",
		time_zone: "Asia/Shanghai",
		summary: {
			total_users: 184,
			active_users_today: 44,
			ongoing_tasks_total: 7,
			queued_tasks: 4,
			running_tasks: 3,
			ongoing_by_task: { translations: 3, summaries: 2, briefs: 2 },
		},
		today_live: {
			date: "2026-04-18",
			total_users: 184,
			active_users: 44,
			ongoing_tasks_total: 7,
			queued_tasks: 4,
			running_tasks: 3,
		},
		status_breakdown: {
			queued_total: 4,
			running_total: 3,
			succeeded_total: 42,
			failed_total: 3,
			canceled_total: 0,
			total: 52,
			items: [
				{
					task_type: "translate.release.batch",
					label: "翻译",
					queued: 2,
					running: 1,
					succeeded: 17,
					failed: 2,
					canceled: 0,
					total: 22,
					success_rate: 0.895,
				},
				{
					task_type: "summarize.release.smart.batch",
					label: "润色",
					queued: 1,
					running: 1,
					succeeded: 13,
					failed: 1,
					canceled: 0,
					total: 16,
					success_rate: 0.929,
				},
				{
					task_type: "brief.daily_slot",
					label: "日报",
					queued: 1,
					running: 1,
					succeeded: 12,
					failed: 0,
					canceled: 0,
					total: 14,
					success_rate: 1,
				},
			],
		},
		task_share: [
			{
				task_type: "translate.release.batch",
				label: "翻译",
				total: 22,
				share_ratio: 22 / 52,
				success_rate: 0.895,
			},
			{
				task_type: "summarize.release.smart.batch",
				label: "润色",
				total: 16,
				share_ratio: 16 / 52,
				success_rate: 0.929,
			},
			{
				task_type: "brief.daily_slot",
				label: "日报",
				total: 14,
				share_ratio: 14 / 52,
				success_rate: 1,
			},
		],
		trend_points: buildTrendPoints(window, (index) => ({
			activeUsers: 26 + Math.floor(index * (window === "30d" ? 0.7 : 3)),
			totalUsers: 150 + index,
			translationsTotal: 8 + Math.floor(index * 0.9),
			translationsFailed: index % 7 === 0 ? 2 : 1,
			summariesTotal: 6 + Math.floor(index * 0.7),
			summariesFailed: index % 11 === 0 ? 1 : 0,
			briefsTotal: 5 + (index % 5),
			briefsFailed: 0,
		})),
		window_meta: {
			selected_window: window,
			available_windows: ["7d", "30d"],
			window_start: window === "30d" ? "2026-03-20" : "2026-04-12",
			window_end: "2026-04-18",
			point_count: window === "30d" ? 30 : 7,
		},
	};
}

function AdminDashboardPreview(props: {
	variant: PreviewVariant;
	initialWindow?: AdminDashboardWindowValue;
}) {
	const { variant, initialWindow = "7d" } = props;
	const [ready, setReady] = useState(false);

	useEffect(() => {
		const originalFetch = window.fetch.bind(window);
		window.fetch = async (input, init) => {
			const req =
				typeof input === "string" || input instanceof URL
					? new Request(input, init)
					: input;
			const url = new URL(req.url, window.location.origin);

			if (url.pathname === "/api/admin/dashboard" && req.method === "GET") {
				const windowValue = (url.searchParams.get("window") ??
					initialWindow) as AdminDashboardWindowValue;
				if (variant === "error") {
					return new Response(
						JSON.stringify({ message: "mock dashboard error" }),
						{
							status: 500,
							headers: { "content-type": "application/json" },
						},
					);
				}
				return new Response(
					JSON.stringify(buildPayload(variant, windowValue)),
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
	}, [initialWindow, variant]);

	if (!ready) return null;

	return (
		<AdminDashboardPage
			me={{
				user: {
					id: "admin-dashboard-storybook-user",
					github_user_id: 10,
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
				},
			}}
		/>
	);
}

const meta = {
	title: "Admin/Admin Dashboard",
	component: AdminDashboardPreview,
	args: {
		variant: "default",
		initialWindow: "7d",
	},
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"管理后台仪表盘，集中展示用户规模、今日活跃、翻译 / 润色 / 日报任务的实时态势，并支持近 7 天 / 近 30 天预聚合趋势切换。",
			},
		},
	},
} satisfies Meta<typeof AdminDashboardPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		await expect(
			canvas.getByRole("heading", { name: "运营总览与任务态势一屏收口" }),
		).toBeInTheDocument();
		await expect(canvas.getByText("管理仪表盘")).toBeInTheDocument();
		await expect(canvas.getByRole("tab", { name: "近 7 天" })).toHaveAttribute(
			"data-state",
			"active",
		);
	},
};

export const BusyDay: Story = {
	args: {
		variant: "busy",
	},
	parameters: {
		docs: {
			description: {
				story: "高峰日场景，强调进行中任务与今日执行状态分布的压力感知。",
			},
		},
	},
};

export const QuietDay: Story = {
	args: {
		variant: "quiet",
	},
	parameters: {
		docs: {
			description: {
				story: "低峰日场景，用来检查低负载情况下的版面平衡与图表可读性。",
			},
		},
	},
};

export const EmptyState: Story = {
	args: {
		variant: "empty",
	},
	parameters: {
		docs: {
			description: {
				story: "空数据场景，用于验证图表与卡片在零值情况下依然稳定。",
			},
		},
	},
};

export const ErrorState: Story = {
	args: {
		variant: "error",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		await expect(canvas.getByRole("alert")).toBeInTheDocument();
	},
};

export const WindowSwitch: Story = {
	name: "Window switch / 7d to 30d",
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		await userEvent.click(canvas.getByRole("tab", { name: "近 30 天" }));
		await expect(canvas.getByRole("tab", { name: "近 30 天" })).toHaveAttribute(
			"data-state",
			"active",
		);
		await expect(canvas.getByText(/03-20/)).toBeInTheDocument();
	},
	parameters: {
		docs: {
			description: {
				story: "验证 7 天 / 30 天窗口切换不会破坏图表结构。",
			},
		},
	},
};

export const EvidenceOverview: Story = {
	name: "Evidence / Overview",
	args: {
		variant: "busy",
		initialWindow: "30d",
	},
	parameters: {
		docs: {
			disable: true,
		},
	},
};
