import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, within } from "storybook/test";

import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";
import {
	type VersionMonitorValue,
	VersionMonitorStateProvider,
} from "@/version/versionMonitor";

type VersionUpdateNoticePreviewProps = Pick<
	VersionMonitorValue,
	| "loadedVersion"
	| "availableVersion"
	| "hasUpdate"
	| "hasServiceWorkerUpdate"
	| "serviceWorkerUpdatePhase"
> & {
	refreshPage: () => void;
};

function VersionUpdateNoticePreview(props: VersionUpdateNoticePreviewProps) {
	const value: VersionMonitorValue = {
		...props,
		canInstallPwa: false,
		isPwaInstalled: true,
	};

	return (
		<VersionMonitorStateProvider value={value}>
			<div className="bg-background min-h-32">
				<VersionUpdateNotice />
			</div>
		</VersionMonitorStateProvider>
	);
}

const meta = {
	title: "Layout/Version Update Notice",
	component: VersionUpdateNoticePreview,
	tags: ["autodocs", "version-update-notice"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"统一展示服务端发布版本变化与 Service Worker 资源更新；页脚版本仍由当前页面构建决定。",
			},
		},
	},
	args: {
		loadedVersion: "v2.72.3",
		availableVersion: null,
		hasUpdate: false,
		hasServiceWorkerUpdate: false,
		serviceWorkerUpdatePhase: "idle",
		refreshPage: fn(),
	},
	argTypes: {
		refreshPage: { control: false },
	},
} satisfies Meta<typeof VersionUpdateNoticePreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const BackendReleaseAvailable: Story = {
	args: {
		availableVersion: "v2.72.4",
		hasUpdate: true,
	},
	play: async ({ args, canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("v2.72.4")).toBeVisible();
		const refreshButton = canvas.getByRole("button", { name: "刷新" });
		await refreshButton.click();
		await expect(args.refreshPage).toHaveBeenCalledTimes(1);
	},
};

export const ServiceWorkerResourcesReady: Story = {
	args: {
		hasUpdate: true,
		hasServiceWorkerUpdate: true,
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("应用资源更新已准备好，刷新后完成切换"),
		).toBeVisible();
		await expect(canvas.getByRole("button", { name: "刷新" })).toBeEnabled();
	},
};

export const ServiceWorkerActivating: Story = {
	args: {
		hasUpdate: true,
		hasServiceWorkerUpdate: true,
		serviceWorkerUpdatePhase: "activating",
	},
	play: async ({ canvasElement }) => {
		const refreshButton = within(canvasElement).getByRole("button", {
			name: "更新中",
		});
		await expect(refreshButton).toBeDisabled();
	},
};

export const ServiceWorkerRetryAfterTimeout: Story = {
	args: {
		hasUpdate: true,
		hasServiceWorkerUpdate: true,
		serviceWorkerUpdatePhase: "failed",
	},
	play: async ({ args, canvasElement }) => {
		const refreshButton = within(canvasElement).getByRole("button", {
			name: "重试",
		});
		await expect(refreshButton).toBeEnabled();
		await refreshButton.click();
		await expect(args.refreshPage).toHaveBeenCalledTimes(1);
	},
};
