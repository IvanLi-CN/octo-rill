import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import { AppMetaFooter } from "@/layout/AppMetaFooter";
import {
	type VersionMonitorValue,
	VERSION_UNKNOWN,
	VersionMonitorStateProvider,
} from "@/version/versionMonitor";

type FooterPreviewProps = {
	loadedVersion: string;
	availableVersion: string | null;
	hasUpdate: boolean;
};

function FooterPreview(props: FooterPreviewProps) {
	const value: VersionMonitorValue = {
		loadedVersion: props.loadedVersion,
		availableVersion: props.availableVersion,
		hasUpdate: props.hasUpdate,
		refreshPage: () => undefined,
	};

	return (
		<div className="bg-background min-h-screen">
			<div className="mx-auto max-w-6xl px-6 py-10">
				<p className="text-muted-foreground font-mono text-sm">
					AppMetaFooter component preview
				</p>
			</div>
			<VersionMonitorStateProvider value={value}>
				<AppMetaFooter />
			</VersionMonitorStateProvider>
		</div>
	);
}

const meta = {
	title: "Layout/App Meta Footer",
	component: FooterPreview,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"Footer 负责展示当前前端构建已经加载到浏览器里的版本号，而不是等待接口后才知道自身版本。`/api/version` 与 `/api/health` 只用于比较后端当前版本，并决定是否提示发现更新。",
			},
		},
	},
	args: {
		loadedVersion: "v0.1.0",
		availableVersion: null,
		hasUpdate: false,
	},
	argTypes: {
		loadedVersion: {
			control: "text",
		},
		availableVersion: {
			control: "text",
		},
		hasUpdate: {
			control: "boolean",
		},
	},
} satisfies Meta<typeof FooterPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const storyRoot = within(canvasElement.ownerDocument.body);
		await expect(storyRoot.getByText("Version v0.1.0")).toBeVisible();
		await expect(storyRoot.getByRole("link", { name: "GitHub" })).toBeVisible();
	},
	parameters: {
		docs: {
			description: {
				story: "正常稳态：footer 直接显示当前已加载前端构建版本。",
			},
		},
	},
};

export const BuildMetadataVersion: Story = {
	args: {
		loadedVersion: "v0.2.0-beta.1+git.abc123",
	},
};

export const UnknownFallback: Story = {
	args: {
		loadedVersion: VERSION_UNKNOWN,
	},
	parameters: {
		docs: {
			description: {
				story:
					"极端降级态：只有在构建版本本身不可解析时，footer 才退回 unknown。正常轮询失败不会再显示 loading。",
			},
		},
	},
};
