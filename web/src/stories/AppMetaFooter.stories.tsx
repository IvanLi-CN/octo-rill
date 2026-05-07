import type { Meta, StoryObj } from "@storybook/react-vite";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
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

const FOOTER_RELEASE_HREF = "/public/IvanLi-CN/octo-rill/releases/tag/v2.29.0";
const FOOTER_RAW_SEMVER_RELEASE_HREF =
	"/public/IvanLi-CN/octo-rill/releases/tag/v2.30.0";
const APP_META_FOOTER_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	footerMobile390: {
		name: "Footer mobile 390x844",
		styles: {
			height: "844px",
			width: "390px",
		},
		type: "mobile",
	},
} as const;

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
		viewport: {
			options: APP_META_FOOTER_VIEWPORTS,
		},
		docs: {
			description: {
				component:
					"Footer 负责展示当前前端构建已经加载到浏览器里的版本号，而不是等待接口后才知道自身版本。`/api/version` 与 `/api/health` 只用于比较后端当前版本，并决定是否提示发现更新。",
			},
		},
	},
	args: {
		loadedVersion: "v2.29.0",
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
		const versionLink = storyRoot.getByRole("link", {
			name: "Version v2.29.0",
		});
		await expect(versionLink).toBeVisible();
		await expect(versionLink).toHaveAttribute("href", FOOTER_RELEASE_HREF);
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

export const MobileVersionLink: Story = {
	parameters: {
		viewport: {
			defaultViewport: "footerMobile390",
		},
		docs: {
			description: {
				story:
					"移动端稳态：footer 在窄屏换行后仍展示可点击版本号，链接到 OctoRill 自身公开 Release 详情页。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const storyRoot = within(canvasElement.ownerDocument.body);
		const versionLink = storyRoot.getByRole("link", {
			name: "Version v2.29.0",
		});
		await expect(versionLink).toBeVisible();
		await expect(versionLink).toHaveAttribute("href", FOOTER_RELEASE_HREF);
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
	play: async ({ canvasElement }) => {
		const storyRoot = within(canvasElement.ownerDocument.body);
		await expect(storyRoot.getByText("Version unknown")).toBeVisible();
		await expect(
			storyRoot.queryByRole("link", { name: "Version unknown" }),
		).not.toBeInTheDocument();
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

export const RawSemverReleaseTag: Story = {
	args: {
		loadedVersion: "2.30.0",
	},
	play: async ({ canvasElement }) => {
		const storyRoot = within(canvasElement.ownerDocument.body);
		const versionLink = storyRoot.getByRole("link", {
			name: "Version 2.30.0",
		});
		await expect(versionLink).toBeVisible();
		await expect(versionLink).toHaveAttribute(
			"href",
			FOOTER_RAW_SEMVER_RELEASE_HREF,
		);
	},
	parameters: {
		docs: {
			description: {
				story:
					"兼容 release 流程输出的无 v 前缀有效版本：展示文本保持当前版本值，跳转 tag 规范化为真实 GitHub Release tag。",
			},
		},
	},
};
