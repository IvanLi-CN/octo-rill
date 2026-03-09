import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useState } from "react";

import type { UserManagementStoryState } from "@/admin/UserManagement";
import type { AdminUserItem } from "@/admin/UserManagement";
import { AdminPanel } from "@/pages/AdminPanel";

const CURRENT_USER_ID = "2f4k7m9p3x6c8v2a";
const STANDARD_USER_ID = "3g5n8q2r4y7d9w3b";
const DISABLED_USER_ID = "4h6p9s3t5z8e2x4c";

const mockAdminUsers: AdminUserItem[] = [
	{
		id: CURRENT_USER_ID,
		github_user_id: 10,
		login: "storybook-admin",
		name: "Storybook Admin",
		avatar_url: null,
		email: "admin@example.com",
		is_admin: true,
		is_disabled: false,
		last_active_at: "2026-02-26T08:00:00Z",
		created_at: "2026-02-25T08:00:00Z",
		updated_at: "2026-02-25T08:00:00Z",
	},
	{
		id: STANDARD_USER_ID,
		github_user_id: 11,
		login: "octo-user",
		name: "Octo User",
		avatar_url: null,
		email: "user@example.com",
		is_admin: false,
		is_disabled: false,
		last_active_at: "2026-02-26T07:30:00Z",
		created_at: "2026-02-25T08:10:00Z",
		updated_at: "2026-02-25T08:10:00Z",
	},
	{
		id: DISABLED_USER_ID,
		github_user_id: 12,
		login: "disabled-user",
		name: "Disabled User",
		avatar_url: null,
		email: "disabled@example.com",
		is_admin: false,
		is_disabled: true,
		last_active_at: null,
		created_at: "2026-02-25T08:20:00Z",
		updated_at: "2026-02-25T08:20:00Z",
	},
];

type AdminPanelPreviewProps = {
	storyState?: UserManagementStoryState;
};

function AdminPanelPreview({ storyState }: AdminPanelPreviewProps) {
	const [ready, setReady] = useState(false);

	useEffect(() => {
		const originalFetch = window.fetch.bind(window);
		let users = mockAdminUsers.map((item) => ({ ...item }));

		window.fetch = async (input, init) => {
			const req =
				typeof input === "string" || input instanceof URL
					? new Request(input, init)
					: input;
			const url = new URL(req.url, window.location.origin);

			if (url.pathname === "/api/admin/users" && req.method === "GET") {
				const query = (url.searchParams.get("query") ?? "").toLowerCase();
				const role = url.searchParams.get("role") ?? "all";
				const status = url.searchParams.get("status") ?? "all";
				const filtered = users.filter((user) => {
					if (role === "admin" && !user.is_admin) return false;
					if (role === "user" && user.is_admin) return false;
					if (status === "enabled" && user.is_disabled) return false;
					if (status === "disabled" && !user.is_disabled) return false;
					if (!query) return true;
					return (
						user.login.toLowerCase().includes(query) ||
						(user.name ?? "").toLowerCase().includes(query) ||
						(user.email ?? "").toLowerCase().includes(query)
					);
				});
				return new Response(
					JSON.stringify({
						items: filtered,
						page: 1,
						page_size: 20,
						total: filtered.length,
						guard: {
							admin_total: users.filter((item) => item.is_admin).length,
							active_admin_total: users.filter(
								(item) => item.is_admin && !item.is_disabled,
							).length,
						},
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}

			if (
				url.pathname.startsWith("/api/admin/users/") &&
				url.pathname.endsWith("/profile") &&
				req.method === "GET"
			) {
				const id = decodeURIComponent(url.pathname.split("/").at(-2) ?? "");
				const target = users.find((user) => user.id === id);
				if (!target) {
					return new Response(
						JSON.stringify({
							ok: false,
							error: { code: "not_found", message: "user not found" },
						}),
						{
							status: 404,
							headers: { "content-type": "application/json" },
						},
					);
				}
				return new Response(
					JSON.stringify({
						user_id: target.id,
						daily_brief_utc_time: "08:00",
						last_active_at: target.last_active_at,
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}

			if (
				url.pathname.startsWith("/api/admin/users/") &&
				req.method === "PATCH"
			) {
				const id = decodeURIComponent(url.pathname.split("/").at(-1) ?? "");
				const payload = (await req.json()) as {
					is_admin?: boolean;
					is_disabled?: boolean;
				};
				const target = users.find((user) => user.id === id);
				if (!target) {
					return new Response(
						JSON.stringify({
							ok: false,
							error: { code: "not_found", message: "user not found" },
						}),
						{
							status: 404,
							headers: { "content-type": "application/json" },
						},
					);
				}

				if (typeof payload.is_admin === "boolean") {
					target.is_admin = payload.is_admin;
				}
				if (typeof payload.is_disabled === "boolean") {
					target.is_disabled = payload.is_disabled;
				}
				target.updated_at = "2026-02-25T10:00:00Z";
				users = users.map((user) => (user.id === id ? { ...target } : user));

				return new Response(JSON.stringify(target), {
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
	}, []);

	if (!ready) {
		return null;
	}

	return (
		<AdminPanel
			me={{
				user: {
					id: CURRENT_USER_ID,
					github_user_id: 10,
					login: "storybook-admin",
					name: "Storybook Admin",
					avatar_url: null,
					email: "admin@example.com",
					is_admin: true,
				},
			}}
			userManagementStoryState={storyState}
		/>
	);
}

const meta = {
	title: "Admin/Admin Panel",
	component: AdminPanelPreview,
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"管理员用户面板，用来检查用户筛选、资料侧栏、管理员确认与禁用状态等管理操作。适合验证表格、筛选器与管理弹层在真实数据形态下的表现。\n\n相关公开文档：[产品说明](../product.html) · [快速开始](../quick-start.html)",
			},
		},
	},
} satisfies Meta<typeof AdminPanelPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	parameters: {
		docs: {
			description: {
				story:
					"默认用户管理列表，展示管理员自身与普通用户、禁用用户并存时的基线状态。",
			},
		},
	},
};

export const Filtered: Story = {
	args: {
		storyState: {
			queryInput: "octo",
			query: "octo",
			role: "user",
			status: "enabled",
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"模拟筛选条件生效后的结果列表，用来验证搜索与角色/状态筛选组合。",
			},
		},
	},
};

export const ProfileSheetOpen: Story = {
	args: {
		storyState: {
			profileUserId: STANDARD_USER_ID,
		},
	},
	parameters: {
		docs: {
			description: {
				story: "直接打开用户资料侧栏，便于检查详情区块与管理操作的布局。",
			},
		},
	},
};

export const AdminConfirmOpen: Story = {
	args: {
		storyState: {
			pendingAdminConfirmUserId: STANDARD_USER_ID,
		},
	},
	parameters: {
		docs: {
			description: {
				story: "聚焦管理员授权确认对话框，检查危险操作前的提示与确认路径。",
			},
		},
	},
};
