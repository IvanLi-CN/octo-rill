import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";
import type { ReactNode } from "react";

import { Markdown } from "./Markdown";

const rawPullUrl = "https://github.com/CherryHQ/cherry-studio/pull/14247";
const rawIssueUrl = "https://github.com/CherryHQ/cherry-studio/issues/914";
const rawCommitUrl =
	"https://github.com/CherryHQ/cherry-studio/commit/4d8f459e7869d3e0b57fafe1b7a9034cb9b2d999";
const rawFileLineUrl =
	"https://github.com/CherryHQ/cherry-studio/blob/4d8f459e7869d3e0b57fafe1b7a9034cb9b2d999/src/main.ts?plain=1#L42";
const rawCompareUrl =
	"https://github.com/CherryHQ/cherry-studio/compare/v2.71.0...v2.71.1#files_bucket";
const encodedCompareUrl =
	"https://github.com/CherryHQ/cherry-studio/compare/release%2Fv2.71.0...v2.71.1#files_bucket";
const rawExternalUrl = "https://docs.example.com/releases/cherry-studio";

const meta = {
	title: "Components/Markdown",
	component: Markdown,
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"共享 Markdown 渲染器会压缩无专用标题的 GitHub URL，同时保留人工标题、外部链接和真实跳转地址。",
			},
		},
	},
} satisfies Meta<typeof Markdown>;

export default meta;
type Story = StoryObj<typeof meta>;

function EvidenceSurface(props: { children: ReactNode }) {
	return (
		<div
			data-visual-evidence-surface
			className="bg-background min-w-[360px] max-w-[720px] p-6"
		>
			<div data-visual-evidence-target>{props.children}</div>
		</div>
	);
}

export const GithubAutolinks: Story = {
	args: { content: "" },
	render: () => (
		<EvidenceSurface>
			<Markdown
				content={[
					"### GitHub 链接",
					"",
					`- PR：${rawPullUrl}`,
					`- Issue：${rawIssueUrl}`,
					`- Commit：${rawCommitUrl}`,
					`- 文件行号：${rawFileLineUrl}`,
					`- Compare：${rawCompareUrl}`,
					`- 编码 Compare：${encodedCompareUrl}`,
					`- 外部文档：${rawExternalUrl}`,
				].join("\n")}
			/>
		</EvidenceSurface>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole("link", { name: "#14247" })).toBeVisible();
		await expect(canvas.getByRole("link", { name: "#914" })).toBeVisible();
		await expect(canvas.getByRole("link", { name: "4d8f459" })).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: "src/main.ts#L42" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("link", {
				name: /^v2\.71\.0\.\.\.v2\.71\.1$/,
			}),
		).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: "release/v2.71.0...v2.71.1" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: rawExternalUrl }),
		).toBeVisible();
	},
};

export const CustomLabelsStayReadable: Story = {
	args: { content: "" },
	render: () => (
		<EvidenceSurface>
			<Markdown
				content={`- 自定义标签：[compare](https://github.com/CherryHQ/cherry-studio/compare/v2.71.0...v2.71.1)\n- 外部链接：[发布文档](${rawExternalUrl})`}
			/>
		</EvidenceSurface>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole("link", { name: "compare" })).toBeVisible();
		await expect(canvas.getByRole("link", { name: "发布文档" })).toBeVisible();
	},
};
