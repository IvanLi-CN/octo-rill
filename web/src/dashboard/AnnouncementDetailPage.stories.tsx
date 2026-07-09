import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";
import { useLayoutEffect } from "react";

import type { AnnouncementDetailResponse } from "@/api";
import { AnnouncementDetailPage } from "@/dashboard/AnnouncementDetailPage";

const STORY_OWNER = "acme";
const STORY_REPO = "rocket";
const STORY_NUMBER = "64";

const announcementDetailReady: AnnouncementDetailResponse = {
	repo_full_name: `${STORY_OWNER}/${STORY_REPO}`,
	discussion_number: 64,
	discussion_key: `${STORY_OWNER}/${STORY_REPO}#64`,
	repo_visual: {
		owner_avatar_url: "https://github.com/acme.png?size=96",
		open_graph_image_url: null,
		uses_custom_open_graph_image: false,
	},
	title: "公告：支持站内 discussion 阅读页",
	body: [
		"## What's changed",
		"",
		"- 标题进入站内详情页，而不是继续留在 GitHub 外链",
		"- 公告和 release 共享原文 / 翻译 / 润色三 lane",
	].join("\n"),
	html_url: "https://github.com/acme/rocket/discussions/64",
	occurred_at: "2026-07-09T12:00:00Z",
	actor: {
		login: "maintainer",
		avatar_url: "https://github.com/maintainer.png?size=96",
		html_url: "https://github.com/maintainer",
	},
	translated: {
		lang: "zh-CN",
		status: "ready",
		title: "公告：支持站内 discussion 详情页",
		summary: "- 译文已就绪\n- 返回工作台时恢复原 tab / scope",
	},
	smart: {
		lang: "zh-CN",
		status: "ready",
		title: "公告：站内阅读流已对齐",
		summary: [
			"## 润色摘要",
			"",
			"- 默认进入润色 lane，但仍保留原文回退。",
			"- 右上角 `GitHub` 继续作为 escape hatch。",
		].join("\n"),
	},
};

function useAnnouncementDetailStoryMock(input: {
	response: AnnouncementDetailResponse | null;
	status?: number;
}) {
	const { response, status = 200 } = input;

	useLayoutEffect(() => {
		const previousFetch = globalThis.fetch.bind(globalThis);
		const detailPath = `/api/repos/${encodeURIComponent(STORY_OWNER)}/${encodeURIComponent(STORY_REPO)}/discussions/${encodeURIComponent(STORY_NUMBER)}/detail`;

		globalThis.fetch = async (request, init) => {
			const rawUrl =
				typeof request === "string"
					? request
					: request instanceof URL
						? request.toString()
						: request.url;
			const resolvedUrl = new URL(rawUrl, window.location.origin);

			if (resolvedUrl.pathname === detailPath) {
				if (status >= 400 || !response) {
					return new Response(
						JSON.stringify({
							error: {
								code: "detail_failed",
								message: "detail failed",
							},
						}),
						{
							headers: { "Content-Type": "application/json" },
							status,
						},
					);
				}
				return new Response(JSON.stringify(response), {
					headers: { "Content-Type": "application/json" },
					status: 200,
				});
			}

			return previousFetch(request, init);
		};

		return () => {
			globalThis.fetch = previousFetch;
		};
	}, [response, status]);
}

function AnnouncementDetailPreview(props: {
	response: AnnouncementDetailResponse | null;
	status?: number;
}) {
	useAnnouncementDetailStoryMock(props);

	return (
		<div className="bg-background min-h-screen px-4 py-8">
			<div className="mx-auto max-w-4xl">
				<AnnouncementDetailPage
					owner={STORY_OWNER}
					repo={STORY_REPO}
					number={STORY_NUMBER}
					onBack={() => {}}
				/>
			</div>
		</div>
	);
}

const meta = {
	title: "Dashboard/AnnouncementDetailPage",
	component: AnnouncementDetailPreview,
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"登录态公告详情页固定走 Dashboard 壳层内的 discussion 阅读面；默认进入润色 lane，并沿用 release detail 的错误与 fallback 语汇。",
			},
		},
	},
} satisfies Meta<typeof AnnouncementDetailPreview>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Ready: Story = {
	args: {
		response: announcementDetailReady,
	},
	parameters: {
		docs: {
			description: {
				story:
					"详情页 ready 态：默认进入润色 lane，保留返回工作台按钮与 GitHub escape hatch。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const smartTab = canvas.getByRole("tab", { name: "润色" });
		await expect(smartTab).toHaveAttribute("aria-selected", "true");
		await expect(
			canvas.getByRole("heading", { name: "公告：站内阅读流已对齐" }),
		).toBeVisible();
		await expect(canvas.getByText("润色摘要", { exact: true })).toBeVisible();
		await expect(canvas.getByRole("link", { name: "GitHub" })).toHaveAttribute(
			"href",
			announcementDetailReady.html_url,
		);
	},
};

export const LoadError: Story = {
	args: {
		response: null,
		status: 500,
	},
	parameters: {
		docs: {
			description: {
				story: "详情页 load error 态：保留统一错误面与单次重试入口。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("公告详情加载失败", { exact: true }),
		).toBeVisible();
		await expect(canvas.getByRole("button", { name: "重试" })).toBeVisible();
	},
};

export const SmartError: Story = {
	args: {
		response: {
			...announcementDetailReady,
			smart: {
				lang: "zh-CN",
				status: "error",
				title: null,
				summary: null,
				error_code: "upstream_timeout",
				error_summary: "润色暂时失败",
				error_detail: "upstream gateway timeout",
				auto_translate: false,
			},
		},
	},
	parameters: {
		docs: {
			description: {
				story:
					"详情页润色错误态：默认仍停在润色 lane，并给出重试润色 / 查看原文两条恢复路径。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("润色失败", { exact: true })).toBeVisible();
		await expect(
			canvas.getByText("润色暂时失败", { exact: true }),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "重试润色" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "查看原文" }),
		).toBeVisible();
	},
};
