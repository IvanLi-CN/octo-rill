import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useState } from "react";

import type { AdminUserItem } from "@/admin/UserManagement";
import { AdminPanel } from "@/pages/AdminPanel";

const mockAdminUsers: AdminUserItem[] = [
	{
		id: 1,
		github_user_id: 10,
		login: "storybook-admin",
		name: "Storybook Admin",
		avatar_url: null,
		email: "admin@example.com",
		is_admin: true,
		is_disabled: false,
		created_at: "2026-02-25T08:00:00Z",
		updated_at: "2026-02-25T08:00:00Z",
	},
	{
		id: 2,
		github_user_id: 11,
		login: "octo-user",
		name: "Octo User",
		avatar_url: null,
		email: "user@example.com",
		is_admin: false,
		is_disabled: false,
		created_at: "2026-02-25T08:10:00Z",
		updated_at: "2026-02-25T08:10:00Z",
	},
	{
		id: 3,
		github_user_id: 12,
		login: "disabled-user",
		name: "Disabled User",
		avatar_url: null,
		email: "disabled@example.com",
		is_admin: false,
		is_disabled: true,
		created_at: "2026-02-25T08:20:00Z",
		updated_at: "2026-02-25T08:20:00Z",
	},
];

function AdminPanelPreview() {
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
				const id = Number(url.pathname.split("/").at(-1));
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
					id: 1,
					github_user_id: 10,
					login: "storybook-admin",
					name: "Storybook Admin",
					avatar_url: null,
					email: "admin@example.com",
					is_admin: true,
				},
			}}
		/>
	);
}

const meta = {
	title: "Pages/AdminPanel",
	component: AdminPanelPreview,
	parameters: {
		layout: "fullscreen",
	},
} satisfies Meta<typeof AdminPanelPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
