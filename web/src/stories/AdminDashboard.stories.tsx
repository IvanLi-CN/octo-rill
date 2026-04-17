import { useEffect, useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import { AdminDashboardPage } from "@/pages/AdminDashboardPage";

type PreviewVariant = "default" | "busy" | "quiet";

function buildPayload(variant: PreviewVariant) {
	if (variant === "busy") {
		return {
			generated_at: "2026-04-18T09:12:00Z",
			time_zone: "Asia/Shanghai",
			window_start: "2026-04-12",
			window_end: "2026-04-18",
			kpis: {
				total_users: 184,
				active_users_today: 61,
				ongoing_tasks_total: 14,
				queued_tasks: 8,
				running_tasks: 6,
				ongoing_by_task: { translations: 6, summaries: 5, briefs: 3 },
			},
			today: {
				queued_total: 8,
				running_total: 6,
				succeeded_total: 67,
				failed_total: 5,
				canceled_total: 1,
				total: 87,
				task_status: [
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
						label: "智能摘要",
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
			trends: [
				["2026-04-12", 22, 18, 11],
				["2026-04-13", 26, 22, 12],
				["2026-04-14", 29, 20, 13],
				["2026-04-15", 34, 24, 16],
				["2026-04-16", 45, 31, 18],
				["2026-04-17", 51, 34, 19],
				["2026-04-18", 61, 37, 31],
			].map(([date, activeUsers, translationsTotal, summariesTotal]) => {
				const dateValue = String(date);
				return {
					date: dateValue,
					label: dateValue.slice(5),
					total_users: Number(dateValue.slice(-2)) + 110,
					active_users: activeUsers,
					translations_total: translationsTotal,
					translations_failed: Math.max(
						0,
						Math.floor(Number(translationsTotal) / 10) - 1,
					),
					summaries_total: summariesTotal,
					summaries_failed: Math.max(
						0,
						Math.floor(Number(summariesTotal) / 12) - 1,
					),
					briefs_total: 10 + (Number(dateValue.slice(-2)) % 6),
					briefs_failed: 0,
				};
			}),
		};
	}

	if (variant === "quiet") {
		return {
			generated_at: "2026-04-18T09:12:00Z",
			time_zone: "Asia/Shanghai",
			window_start: "2026-04-12",
			window_end: "2026-04-18",
			kpis: {
				total_users: 184,
				active_users_today: 17,
				ongoing_tasks_total: 2,
				queued_tasks: 1,
				running_tasks: 1,
				ongoing_by_task: { translations: 0, summaries: 1, briefs: 1 },
			},
			today: {
				queued_total: 1,
				running_total: 1,
				succeeded_total: 18,
				failed_total: 0,
				canceled_total: 0,
				total: 20,
				task_status: [
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
						label: "智能摘要",
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
			trends: [
				["2026-04-12", 12, 4, 3, 5],
				["2026-04-13", 14, 4, 5, 5],
				["2026-04-14", 13, 5, 4, 6],
				["2026-04-15", 15, 5, 5, 6],
				["2026-04-16", 14, 6, 5, 6],
				["2026-04-17", 16, 5, 4, 6],
				["2026-04-18", 17, 6, 7, 7],
			].map(
				([
					date,
					active_users,
					translations_total,
					summaries_total,
					briefs_total,
				]) => {
					const dateValue = String(date);
					return {
						date: dateValue,
						label: dateValue.slice(5),
						total_users: 184,
						active_users,
						translations_total,
						translations_failed: 0,
						summaries_total,
						summaries_failed: 0,
						briefs_total,
						briefs_failed: 0,
					};
				},
			),
		};
	}

	return {
		generated_at: "2026-04-18T09:12:00Z",
		time_zone: "Asia/Shanghai",
		window_start: "2026-04-12",
		window_end: "2026-04-18",
		kpis: {
			total_users: 184,
			active_users_today: 44,
			ongoing_tasks_total: 7,
			queued_tasks: 4,
			running_tasks: 3,
			ongoing_by_task: { translations: 3, summaries: 2, briefs: 2 },
		},
		today: {
			queued_total: 4,
			running_total: 3,
			succeeded_total: 42,
			failed_total: 3,
			canceled_total: 0,
			total: 52,
			task_status: [
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
					label: "智能摘要",
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
		trends: [
			["2026-04-12", 26, 11, 9, 8],
			["2026-04-13", 28, 12, 8, 9],
			["2026-04-14", 31, 14, 10, 10],
			["2026-04-15", 36, 15, 11, 10],
			["2026-04-16", 39, 17, 12, 11],
			["2026-04-17", 41, 18, 13, 12],
			["2026-04-18", 44, 22, 16, 14],
		].map(
			([
				date,
				active_users,
				translations_total,
				summaries_total,
				briefs_total,
			]) => {
				const dateValue = String(date);
				return {
					date: dateValue,
					label: dateValue.slice(5),
					total_users: Number(dateValue.slice(-2)) + 110,
					active_users,
					translations_total,
					translations_failed: Number(translations_total) > 14 ? 2 : 1,
					summaries_total,
					summaries_failed: Number(summaries_total) > 12 ? 1 : 0,
					briefs_total,
					briefs_failed: 0,
				};
			},
		),
	};
}

function AdminDashboardPreview(props: { variant: PreviewVariant }) {
	const { variant } = props;
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
				return new Response(JSON.stringify(buildPayload(variant)), {
					status: 200,
					headers: { "content-type": "application/json" },
				});
			}

			return originalFetch(input, init);
		};
		setReady(true);

		return () => {
			window.fetch = originalFetch;
		};
	}, [variant]);

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
	},
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"管理后台仪表盘，集中展示用户规模、今日活跃、翻译 / 智能摘要 / 日报任务的实时态势与近 7 日 rollup 趋势，适合做运营总览与异常巡检。",
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

export const EvidenceOverview: Story = {
	name: "Evidence / Overview",
	parameters: {
		docs: {
			disable: true,
		},
	},
};
