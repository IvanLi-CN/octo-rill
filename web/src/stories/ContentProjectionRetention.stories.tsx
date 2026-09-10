import { expect, fn, waitFor, within } from "storybook/test";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useRef } from "react";

import type { AnnouncementDetailResponse, ReleaseDetailResponse } from "@/api";
import { AnnouncementDetailPage } from "@/dashboard/AnnouncementDetailPage";
import type { DashboardReleaseTarget } from "@/dashboard/routeState";
import { ReleaseDetailCard } from "@/sidebar/ReleaseDetailCard";

const retainedPolishDetail: ReleaseDetailResponse = {
	release_id: "383114065",
	repo_full_name: "octo-rill/example",
	repo_visual: null,
	tag_name: "v2.63.4",
	previous_tag_name: "v2.63.3",
	name: "Original release title",
	body: "Original release body",
	html_url: "https://github.com/octo-rill/example/releases/tag/v2.63.4",
	published_at: "2026-09-10T13:40:00Z",
	is_prerelease: 0,
	is_draft: 0,
	translated: {
		lang: "zh-CN",
		status: "ready",
		title: "已翻译标题",
		summary: "已翻译摘要",
	},
	smart: {
		lang: "zh-CN",
		status: "ready",
		title: "保留的润色标题",
		summary: "保留的润色摘要",
		request_id: "request-running-polish",
		auto_translate: true,
	},
};

const retainedAnnouncementDetail: AnnouncementDetailResponse = {
	repo_full_name: "octo-rill/example",
	discussion_number: 42,
	discussion_key: "octo-rill/example#42",
	repo_visual: null,
	title: "Original announcement title",
	body: "Original announcement body",
	html_url: "https://github.com/octo-rill/example/discussions/42",
	occurred_at: "2026-09-10T13:40:00Z",
	actor: { login: "octo-rill" },
	translated: {
		lang: "zh-CN",
		status: "ready",
		title: "已翻译公告标题",
		summary: "已翻译公告摘要",
	},
	smart: {
		lang: "zh-CN",
		status: "ready",
		title: "保留的公告润色标题",
		summary: "保留的公告润色摘要",
		request_id: "request-running-announcement-polish",
		auto_translate: true,
	},
};

const target: DashboardReleaseTarget = {
	releaseId: retainedPolishDetail.release_id,
	locator: null,
	fromTab: "all",
};

const meta = {
	title: "Content Projection Retention",
	component: ReleaseDetailCard,
	parameters: { layout: "fullscreen" },
	tags: ["content-projection-retention"],
} satisfies Meta<typeof ReleaseDetailCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const RetainedPolishProjectionRelease: Story = {
	args: {
		target,
		onClose: fn(),
		onResolvedDetail: fn(),
	},
	decorators: [
		(Story) => {
			const originalFetch = useRef(window.fetch);
			const restoreFetch = originalFetch.current;
			window.fetch = async (input, init) => {
				const url = new URL(
					typeof input === "string"
						? input
						: input instanceof Request
							? input.url
							: input.toString(),
					window.location.origin,
				);
				if (url.pathname.endsWith("/api/releases/383114065/detail")) {
					return new Response(JSON.stringify(retainedPolishDetail), {
						status: 200,
					});
				}
				return restoreFetch(input, init);
			};
			useEffect(
				() => () => {
					window.fetch = restoreFetch;
				},
				[restoreFetch],
			);
			return <Story />;
		},
	],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		await waitFor(() =>
			expect(canvas.getByText("保留的润色摘要")).toBeVisible(),
		);
		await expect(canvas.getByRole("tab", { name: "润色" })).toBeVisible();
	},
};

export const RetainedPolishProjectionAnnouncement: Story = {
	render: () => (
		<AnnouncementDetailPage
			owner="octo-rill"
			repo="example"
			number="42"
			onBack={fn()}
		/>
	),
	decorators: [
		(Story) => {
			const originalFetch = useRef(window.fetch);
			const restoreFetch = originalFetch.current;
			window.fetch = async (input, init) => {
				const url = new URL(
					typeof input === "string"
						? input
						: input instanceof Request
							? input.url
							: input.toString(),
					window.location.origin,
				);
				if (
					url.pathname.endsWith(
						"/api/repos/octo-rill/example/discussions/42/detail",
					)
				) {
					return new Response(JSON.stringify(retainedAnnouncementDetail), {
						status: 200,
					});
				}
				return restoreFetch(input, init);
			};
			useEffect(
				() => () => {
					window.fetch = restoreFetch;
				},
				[restoreFetch],
			);
			return <Story />;
		},
	],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement.ownerDocument.body);
		await waitFor(() =>
			expect(canvas.getByText("保留的公告润色摘要")).toBeVisible(),
		);
		await expect(canvas.getByRole("tab", { name: "润色" })).toBeVisible();
	},
};
