import { expect, fn, within } from "storybook/test";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useRef } from "react";

import type { ReleaseDetailResponse } from "@/api";
import type { DashboardReleaseTarget } from "@/dashboard/routeState";
import { ReleaseDetailCard } from "@/sidebar/ReleaseDetailCard";

const retainedPolishDetail: ReleaseDetailResponse = {
	release_id: "release-projection-retention",
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
		status: "running",
		title: "保留的润色标题",
		summary: "保留的润色摘要",
		request_id: "request-running-polish",
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
} satisfies Meta<typeof ReleaseDetailCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const RetainedPolishProjectionWhileRunning: Story = {
	args: {
		target,
		onClose: fn(),
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
				if (
					url.pathname.endsWith(
						"/api/releases/release-projection-retention/detail",
					)
				) {
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
		await expect(canvas.getByText("保留的润色摘要")).toBeVisible();
		await expect(canvas.getByText("润色正在后台处理中。")).toBeVisible();
	},
};
