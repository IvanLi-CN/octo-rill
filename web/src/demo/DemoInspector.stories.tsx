import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, userEvent, within } from "storybook/test";
import { useState } from "react";

import { buildDemoModel } from "@/demo/fixtures";
import {
	DemoInspectorDockedRail,
	DemoInspectorPanel,
} from "@/demo/DemoInspector";

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
		webhookScenario: "healthy-registered" as const,
		publicationState: "published" as const,
		landingCase: "default" as const,
		landingAuthAction: "idle" as const,
		landingPasskeySupport: "supported" as const,
		landingBootState: "ready" as const,
		appShellState: "steady" as const,
		controlsHidden: false,
		contentDataCase: "loaded" as const,
		contentNetworkProfile: "normal" as const,
		llmDataCase: "loaded" as const,
		llmNetworkProfile: "normal" as const,
	},
	model: buildDemoModel({
		sceneId: "dashboard-repo-publish",
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
		onSurfaceChange: fn(),
		onContentDataCaseChange: fn(),
		onContentNetworkProfileChange: fn(),
		onLlmDataCaseChange: fn(),
		onLlmNetworkProfileChange: fn(),
		onLandingCaseChange: fn(),
		onLandingAuthActionChange: fn(),
		onLandingPasskeySupportChange: fn(),
		onLandingBootStateChange: fn(),
		onPersonaChange: fn(),
		onNetworkChange: fn(),
		onIncludeOwnReleasesChange: fn(),
		onWebhookScenarioChange: fn(),
		onPublicationStateChange: fn(),
		onReset: fn(),
		onCopyShareLink: fn(),
	},
	parameters: {
		layout: "centered",
	},
	tags: ["autodocs"],
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
		await expect(
			canvas.getByDisplayValue(
				"/demo/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish&d_persona=member&d_own=1&d_pub=published",
			),
		).toHaveAttribute("readonly");
	},
};

export const LandingCases: Story = {
	args: {
		snapshot: {
			...baseSnapshot,
			shareState: {
				...baseSnapshot.shareState,
				sceneId: "landing-welcome",
				personaId: "guest",
				landingCase: "github-redirect",
				landingAuthAction: "github",
			},
			model: buildDemoModel({
				sceneId: "landing-welcome",
				personaId: "guest",
				includeOwnReleases: true,
				publicationState: "published",
			}),
		},
		sceneTitle: "Landing",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByLabelText("Case preset")).toHaveValue(
			"github-redirect",
		);
		await expect(canvas.getByText("GitHub OAuth pending")).toBeInTheDocument();
		await expect(canvas.getByText("Landing Controls")).toBeInTheDocument();
	},
};

export const AdminJobsSurfaceStates: Story = {
	args: {
		snapshot: {
			...baseSnapshot,
			shareState: {
				...baseSnapshot.shareState,
				sceneId: "admin-jobs-running",
				personaId: "admin",
				contentDataCase: "empty",
				contentNetworkProfile: "normal",
				llmDataCase: "many",
				llmNetworkProfile: "slow",
			},
			model: buildDemoModel({
				sceneId: "admin-jobs-running",
				personaId: "admin",
				includeOwnReleases: true,
				publicationState: "published",
			}),
		},
		sceneTitle: "Admin Jobs",
		activeAdminJobsSurface: "content",
		shareHref:
			"/admin/jobs/ai-records?demo=admin-jobs-running&d_persona=admin&d_content_case=empty&d_llm_case=many&d_llm_net=slow",
	},
	render: (args) => {
		const [surface, setSurface] = useState<"content" | "llm">(
			args.activeAdminJobsSurface ?? "content",
		);
		const [contentDataCase, setContentDataCase] = useState(
			args.snapshot.shareState.contentDataCase,
		);
		const [llmDataCase, setLlmDataCase] = useState(
			args.snapshot.shareState.llmDataCase,
		);
		return (
			<div
				data-visual-evidence-surface="admin-jobs-inspector-story"
				className="bg-background p-6"
			>
				<div data-visual-evidence-target="admin-jobs-inspector-story">
					<DemoInspectorPanel
						{...args}
						activeAdminJobsSurface={surface}
						snapshot={{
							...args.snapshot,
							shareState: {
								...args.snapshot.shareState,
								contentDataCase,
								llmDataCase,
							},
						}}
						onSurfaceChange={(value) => {
							setSurface(value);
							args.onSurfaceChange?.(value);
						}}
						onContentDataCaseChange={(value) => {
							setContentDataCase(value);
							args.onContentDataCaseChange?.(value);
						}}
						onLlmDataCaseChange={(value) => {
							setLlmDataCase(value);
							args.onLlmDataCaseChange?.(value);
						}}
					/>
				</div>
			</div>
		);
	},
	tags: ["demo-inspector"],
	play: async ({ args, canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: /Admin Jobs/ }),
		).toBeInTheDocument();
		await expect(canvas.getByLabelText("Surface")).toHaveValue("content");
		await expect(canvas.getByLabelText("Data case")).toHaveValue("empty");
		await userEvent.selectOptions(canvas.getByLabelText("Surface"), "llm");
		await expect(args.onSurfaceChange).toHaveBeenCalledWith("llm");
		await expect(canvas.getByLabelText("Data case")).toHaveValue("many");
		await userEvent.selectOptions(
			canvas.getByLabelText("Data case"),
			"loading",
		);
		await expect(args.onLlmDataCaseChange).toHaveBeenCalledWith("loading");
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
		await expect(
			canvas.getByDisplayValue(
				"/demo/focus/repo/octo-demo/release-lab?demo=dashboard-repo-publish&d_persona=member&d_own=1&d_pub=published",
			),
		).toBeInTheDocument();
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

export const WideDockedRail: Story = {
	parameters: {
		layout: "fullscreen",
	},
	render: (args) => (
		<div className="h-[820px] w-[380px] overflow-hidden">
			<DemoInspectorDockedRail onCollapse={fn()}>
				{({ density }) => <DemoInspectorPanel {...args} density={density} />}
			</DemoInspectorDockedRail>
		</div>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("Pinned on wide desktop"),
		).toBeInTheDocument();
		await expect(canvas.getByText("Dashboard")).toBeInTheDocument();
		await expect(canvas.getByText("Copy Share URL")).toBeInTheDocument();
		await expect(
			canvasElement.ownerDocument.body.querySelector(
				'[data-demo-inspector-collapse="true"]',
			),
		).toBeTruthy();
	},
};
