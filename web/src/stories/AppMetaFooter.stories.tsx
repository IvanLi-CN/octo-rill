import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useState } from "react";

import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { VersionMonitorProvider } from "@/version/versionMonitor";

type FooterPreviewProps = {
	delayMs: number;
	mockMode: "success" | "fallback" | "failure";
	mockVersion: string;
};

function FooterPreview({ delayMs, mockMode, mockVersion }: FooterPreviewProps) {
	const [mockReady, setMockReady] = useState(false);

	useEffect(() => {
		const originalFetch = globalThis.fetch.bind(globalThis);
		setMockReady(false);

		globalThis.fetch = async (input, init) => {
			const url =
				typeof input === "string"
					? input
					: input instanceof URL
						? input.toString()
						: input.url;

			if (url.endsWith("/api/version")) {
				if (mockMode === "failure" || mockMode === "fallback") {
					return new Response(null, { status: 503, statusText: "Unavailable" });
				}

				if (delayMs > 0) {
					await new Promise((resolve) => setTimeout(resolve, delayMs));
				}

				return new Response(
					JSON.stringify({
						ok: true,
						version: mockVersion,
						source: "APP_EFFECTIVE_VERSION",
					}),
					{
						headers: { "Content-Type": "application/json" },
						status: 200,
					},
				);
			}

			if (url.endsWith("/api/health")) {
				if (mockMode === "failure") {
					return new Response(null, { status: 503, statusText: "Unavailable" });
				}

				if (delayMs > 0) {
					await new Promise((resolve) => setTimeout(resolve, delayMs));
				}

				return new Response(
					JSON.stringify({ ok: true, version: mockVersion }),
					{
						headers: { "Content-Type": "application/json" },
						status: 200,
					},
				);
			}

			return originalFetch(input, init);
		};
		setMockReady(true);

		return () => {
			setMockReady(false);
			globalThis.fetch = originalFetch;
		};
	}, [delayMs, mockMode, mockVersion]);

	return (
		<div className="bg-background min-h-screen">
			<div className="mx-auto max-w-6xl px-6 py-10">
				<p className="text-muted-foreground font-mono text-sm">
					AppMetaFooter component preview
				</p>
			</div>
			{mockReady ? (
				<VersionMonitorProvider>
					<AppMetaFooter />
				</VersionMonitorProvider>
			) : null}
		</div>
	);
}

const meta = {
	title: "Layout/App Meta Footer",
	component: FooterPreview,
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"Footer 负责展示当前页实际加载版本，并复用统一版本监视状态；当 `/api/version` 不可用时，会回退到 `/api/health`。适合在这里确认成功、回退与失败三种基础文案。\n\n相关公开文档：[配置参考](../config.html) · [快速开始](../quick-start.html)",
			},
		},
	},
	args: {
		delayMs: 0,
		mockMode: "success",
		mockVersion: "0.1.0",
	},
	argTypes: {
		mockMode: {
			control: "inline-radio",
			options: ["success", "fallback", "failure"],
		},
		mockVersion: {
			control: "text",
		},
		delayMs: {
			control: "number",
		},
	},
} satisfies Meta<typeof FooterPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	parameters: {
		docs: {
			description: {
				story: "正常从版本接口拿到当前页实际加载版本时的默认展示。",
			},
		},
	},
};

export const BuildMetadataVersion: Story = {
	args: {
		mockVersion: "0.2.0-beta.1+git.abc123",
	},
};

export const VersionEndpointFallback: Story = {
	args: {
		mockMode: "fallback",
	},
	parameters: {
		docs: {
			description: {
				story: "模拟 `/api/version` 不可用时，Footer 回退到健康检查元信息。",
			},
		},
	},
};

export const RequestFailed: Story = {
	args: {
		mockMode: "failure",
	},
	parameters: {
		docs: {
			description: {
				story: "模拟所有版本请求都失败时的降级文案与错误状态。",
			},
		},
	},
};
