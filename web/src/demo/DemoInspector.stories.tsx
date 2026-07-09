import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, within } from "storybook/test";

import { buildDemoModel } from "@/demo/fixtures";
import { DemoInspectorPanel } from "@/demo/DemoInspector";

const baseSnapshot = {
	active: true,
	demoBuild: true,
	basepath: "/demo",
	revision: 0,
	shareState: {
		sceneId: "dashboard-repo-publish" as const,
		personaId: "member" as const,
		networkMode: "normal" as const,
		includeOwnReleases: true,
		publicationState: "published" as const,
	},
	model: buildDemoModel({
		personaId: "member",
		includeOwnReleases: true,
		publicationState: "published",
	}),
	mutations: [
		{
			id: "mutation-1",
			label: "Publish public release page",
			detail: "octo-demo/release-lab is now published in demo memory only.",
			at: "2026-07-08T09:05:00+08:00",
		},
	],
	panelLayout: {
		edge: "right" as const,
		x: 16,
		y: 88,
		collapsed: false,
	},
	lastSyncedHref:
		"/demo/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish&d_persona=member&d_own=1&d_pub=published",
};

const meta = {
	title: "Demo/Inspector Panel",
	component: DemoInspectorPanel,
	render: (args) => (
		<div className="w-[400px]">
			<DemoInspectorPanel {...args} />
		</div>
	),
	args: {
		snapshot: baseSnapshot,
		sceneTitle: "Dashboard",
		shareHref:
			"/demo/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish&d_persona=member&d_own=1&d_pub=published",
		onSceneChange: fn(),
		onPersonaChange: fn(),
		onNetworkChange: fn(),
		onIncludeOwnReleasesChange: fn(),
		onPublicationStateChange: fn(),
		onReset: fn(),
		onCopyShareLink: fn(),
	},
	parameters: {
		layout: "centered",
	},
} satisfies Meta<typeof DemoInspectorPanel>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("Dashboard")).toBeInTheDocument();
		await expect(
			canvas.getByText("Publish public release page"),
		).toBeInTheDocument();
		await expect(canvas.getByText("Copy Share URL")).toBeInTheDocument();
	},
};

export const ShortDesktopSurface: Story = {
	render: (args) => (
		<div className="h-[560px] w-[400px] overflow-y-auto rounded-[28px] border bg-background p-3">
			<DemoInspectorPanel {...args} />
		</div>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("Actions & Share")).toBeInTheDocument();
		await expect(canvas.getByText("Share")).toBeInTheDocument();
	},
};

export const CompactDesktopSurface: Story = {
	args: {
		density: "compact",
	},
	render: (args) => (
		<div className="h-[560px] w-[400px] overflow-y-auto rounded-[28px] border bg-background p-3">
			<DemoInspectorPanel {...args} />
		</div>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("Actions & Share")).toBeInTheDocument();
		await expect(canvas.getByText("Advanced")).toBeInTheDocument();
	},
};
