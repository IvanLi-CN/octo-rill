import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, fn, userEvent, waitFor, within } from "storybook/test";

import { ApiError, type SearchResponse } from "@/api";
import { Button } from "@/components/ui/button";
import { resolveNotificationHref } from "@/inbox/notificationLink";
import {
	CommandPalette,
	type CommandPaletteProps,
} from "@/search/CommandPalette";

const SEARCH_FIXTURE: SearchResponse = {
	query: "命令面板",
	remaining: 47,
	reset_at: "2026-09-13T11:30:00Z",
	items: [
		{
			id: "release-1",
			resource_type: "release",
			title: "v2.31.0 · 稳定的命令面板",
			snippet: "新增键盘导航、本地缓存搜索与可恢复的阅读状态。",
			repo_full_name: "octo-demo/release-lab",
			source_time: "2026-09-13T08:20:00Z",
			unread: true,
			matched_lane: "translated",
			target_path: "/octo-demo/release-lab/releases/tag/v2.31.0",
			target_url: null,
		},
		{
			id: "octo-demo/release-lab",
			resource_type: "repository",
			title: "octo-demo/release-lab",
			snippet: "包含本地缓存与同步能力的示例仓库。",
			repo_full_name: "octo-demo/release-lab",
			source_time: "2026-09-13T07:50:00Z",
			unread: false,
			is_following: false,
			matched_lane: "original",
			target_path: "/focus/repo/octo-demo/release-lab",
			target_url: "https://github.com/octo-demo/release-lab",
		},
	],
};

function mockSearchTransport(query: string, signal?: AbortSignal) {
	return new Promise<SearchResponse>((resolve, reject) => {
		const timer = window.setTimeout(() => {
			if (signal?.aborted) return;
			resolve(
				query.toLocaleLowerCase().includes("没有")
					? { ...SEARCH_FIXTURE, items: [] }
					: SEARCH_FIXTURE,
			);
		}, 30);
		signal?.addEventListener("abort", () => {
			window.clearTimeout(timer);
			reject(new DOMException("Aborted", "AbortError"));
		});
	});
}

function rateLimitedTransport(
	_query: string,
	_signal?: AbortSignal,
): Promise<SearchResponse> {
	return Promise.reject(
		new ApiError(429, "搜索额度已用完", "search_rate_limited", {
			error: {
				code: "search_rate_limited",
				retry_after_seconds: 214,
				reset_at: "2026-09-13T11:30:00Z",
			},
		}),
	);
}

function loadingTransport(
	_query: string,
	_signal?: AbortSignal,
): Promise<SearchResponse> {
	return new Promise(() => {});
}

function slowSearchTransport(query: string, signal?: AbortSignal) {
	return new Promise<SearchResponse>((resolve, reject) => {
		const timer = window.setTimeout(() => {
			if (signal?.aborted) return;
			resolve({ ...SEARCH_FIXTURE, query });
		}, 400);
		signal?.addEventListener("abort", () => {
			window.clearTimeout(timer);
			reject(new DOMException("Aborted", "AbortError"));
		});
	});
}

function errorTransport(
	_query: string,
	_signal?: AbortSignal,
): Promise<SearchResponse> {
	return Promise.reject(new Error("网络暂时不可用"));
}

type PalettePreviewProps = Omit<
	CommandPaletteProps,
	"open" | "onOpenChange"
> & { initialOpen?: boolean };

function PalettePreview(props: PalettePreviewProps) {
	const { initialOpen = false, ...paletteProps } = props;
	const [open, setOpen] = useState(initialOpen);
	const [portalContainer, setPortalContainer] = useState<HTMLDivElement | null>(
		null,
	);
	return (
		<div
			ref={setPortalContainer}
			className="min-h-screen bg-background p-8"
			data-visual-evidence-surface
		>
			{initialOpen ? null : (
				<Button type="button" onClick={() => setOpen(true)}>
					打开命令面板
				</Button>
			)}
			<CommandPalette
				{...paletteProps}
				open={open}
				onOpenChange={setOpen}
				portalContainer={portalContainer}
			/>
		</div>
	);
}

const COMMAND_PALETTE_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	commandPaletteMobile393: {
		name: "Command palette mobile 393x852",
		styles: { width: "393px", height: "852px" },
		type: "mobile",
	},
} as const;

const meta = {
	title: "Search/Command Palette",
	tags: ["autodocs", "command-palette-search"],
	parameters: {
		layout: "fullscreen",
		viewport: { options: COMMAND_PALETTE_VIEWPORTS },
		docs: {
			description: {
				component:
					"命令面板把本地缓存搜索和低频工作区动作放在一个可键盘操作的浮层中。普通查询只调用本地搜索接口，输入 > 后只显示动作。",
			},
		},
	},
} satisfies Meta<typeof CommandPalette>;

export default meta;
type Story = StoryObj<typeof meta>;
const generateBriefAction = fn();
const busySyncInboxAction = fn();

export const Empty: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			isAdmin={false}
			onSyncAll={() => {}}
			onSyncInbox={() => {}}
			onGenerateBrief={() => {}}
			searchTransport={mockSearchTransport}
		/>
	),
};

export const SearchResults: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			isAdmin={false}
			initialQuery="命令面板"
			searchTransport={mockSearchTransport}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		await expect(
			canvas.getByRole("dialog", { name: "命令面板" }),
		).toBeInTheDocument();
		await waitFor(() =>
			expect(canvas.getByText("v2.31.0 · 稳定的命令面板")).toBeVisible(),
		);
		await expect(canvas.getByText("翻译命中")).toBeVisible();
		await expect(canvas.getByText(/剩余 47\/50/)).toBeVisible();
	},
};

export const ClearSearchResetsQuota: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			isAdmin={false}
			initialQuery="命令面板"
			searchTransport={mockSearchTransport}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		const input = canvas.getByRole("combobox", { name: "搜索内容或执行动作" });
		await waitFor(() => expect(canvas.getByText(/剩余 47\/50/)).toBeVisible());
		await userEvent.click(canvas.getByRole("button", { name: "清除搜索" }));
		await expect(input).toHaveValue("");
		await expect(canvas.queryByText(/剩余 47\/50/)).not.toBeInTheDocument();
	},
};

export const Actions: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			isAdmin={false}
			onSyncAll={() => {}}
			onSyncInbox={() => {}}
			onGenerateBrief={() => {}}
			searchTransport={mockSearchTransport}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		const input = canvas.getByRole("combobox", { name: "搜索内容或执行动作" });
		await userEvent.type(input, "> 同步");
		await expect(canvas.getByText("全量同步")).toBeVisible();
		await expect(
			canvas.queryByText("v2.31.0 · 稳定的命令面板"),
		).not.toBeInTheDocument();
		await expect(canvas.getByText("同步 Inbox")).toBeVisible();
	},
};

export const BusyActionsIgnoreKeyboardEnter: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			isAdmin={false}
			busy="Sync inbox"
			onSyncInbox={busySyncInboxAction}
			searchTransport={mockSearchTransport}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		const input = canvas.getByRole("combobox", { name: "搜索内容或执行动作" });
		await userEvent.type(input, "> 同步 Inbox");
		await userEvent.keyboard("{ArrowDown}{Enter}");
		await expect(canvas.getByText("同步 Inbox")).toBeVisible();
		await expect(busySyncInboxAction).not.toHaveBeenCalled();
	},
};

export const GenerateBriefConfirmation: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			isAdmin={false}
			onGenerateBrief={generateBriefAction}
			searchTransport={mockSearchTransport}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		const input = canvas.getByRole("combobox", { name: "搜索内容或执行动作" });
		await userEvent.type(input, "> 生成日报");
		await userEvent.click(canvas.getByRole("option", { name: /生成日报/ }));
		await expect(canvas.getByText("确认生成日报？")).toBeVisible();
		await expect(canvas.getByRole("button", { name: "返回" })).toHaveFocus();
		await userEvent.click(canvas.getByRole("button", { name: "返回" }));
		await expect(input).toHaveFocus();
		await userEvent.click(canvas.getByRole("option", { name: /生成日报/ }));
		await expect(
			canvas.getByRole("button", { name: "确认生成" }),
		).toBeVisible();
		await userEvent.click(canvas.getByRole("button", { name: "确认生成" }));
		await expect(generateBriefAction).toHaveBeenCalledTimes(1);
	},
};

export const NotificationApiUrlFallback: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			isAdmin={false}
			searchTransport={mockSearchTransport}
		/>
	),
	play: async () => {
		await expect(
			resolveNotificationHref({
				thread_id: "417",
				html_url: "https://api.github.com/notifications/threads/417",
				repo_full_name: "octo-demo/release-lab",
			}),
		).toBe("https://github.com/notifications/threads/417");
	},
};

export const RateLimited: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			initialQuery="release"
			isAdmin={false}
			searchTransport={rateLimitedTransport}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		await waitFor(() =>
			expect(canvas.getByRole("alert")).toHaveTextContent(/搜索额度已用完/),
		);
	},
};

export const Loading: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			initialQuery="本地缓存"
			isAdmin={false}
			searchTransport={loadingTransport}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		await waitFor(() =>
			expect(canvas.getByRole("status")).toHaveTextContent(/正在搜索本地缓存/),
		);
	},
};

export const LoadingClearsPreviousResults: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			initialQuery="命令面板"
			isAdmin={false}
			searchTransport={slowSearchTransport}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		const input = canvas.getByRole("combobox", { name: "搜索内容或执行动作" });
		await waitFor(() =>
			expect(canvas.getByText("v2.31.0 · 稳定的命令面板")).toBeVisible(),
		);
		await userEvent.type(input, " 新查询");
		await expect(
			canvas.queryByText("v2.31.0 · 稳定的命令面板"),
		).not.toBeInTheDocument();
		await expect(input).not.toHaveAttribute("aria-activedescendant");
	},
};

export const ErrorState: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			initialQuery="本地缓存"
			isAdmin={false}
			searchTransport={errorTransport}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		await waitFor(() =>
			expect(canvas.getByRole("alert")).toHaveTextContent(/网络暂时不可用/),
		);
	},
};

export const AdminNavigation: Story = {
	render: () => (
		<PalettePreview initialOpen isAdmin searchTransport={mockSearchTransport} />
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		await expect(
			canvas.getByRole("option", { name: /打开管理员面板/ }),
		).toBeInTheDocument();
		await expect(canvas.queryByText("全量同步")).not.toBeInTheDocument();
	},
};

export const Mobile393: Story = {
	render: () => (
		<PalettePreview
			initialOpen
			isAdmin={false}
			initialQuery="命令面板"
			searchTransport={mockSearchTransport}
		/>
	),
	parameters: {
		viewport: {
			defaultViewport: "commandPaletteMobile393",
		},
		docs: { description: { story: "393px mobile evidence surface." } },
	},
};
