import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, within } from "storybook/test";

import { Button } from "@/components/ui/button";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";
import {
	type VersionMonitorValue,
	VERSION_UNKNOWN,
	VersionMonitorStateProvider,
} from "@/version/versionMonitor";

type ShellScenario = "steady" | "update" | "unknown";

type AppShellPreviewProps = {
	scenario: ShellScenario;
	onRefresh: () => void;
	mobileChrome?: boolean;
};

function makeVersionState(
	scenario: ShellScenario,
	onRefresh: () => void,
): VersionMonitorValue {
	switch (scenario) {
		case "update":
			return {
				loadedVersion: "v2.4.6",
				availableVersion: "v2.5.0",
				hasUpdate: true,
				refreshPage: onRefresh,
			};
		case "unknown":
			return {
				loadedVersion: VERSION_UNKNOWN,
				availableVersion: null,
				hasUpdate: false,
				refreshPage: onRefresh,
			};
		default:
			return {
				loadedVersion: "v2.4.6",
				availableVersion: null,
				hasUpdate: false,
				refreshPage: onRefresh,
			};
	}
}

function AppShellPreview({
	scenario,
	onRefresh,
	mobileChrome = false,
}: AppShellPreviewProps) {
	return (
		<VersionMonitorStateProvider value={makeVersionState(scenario, onRefresh)}>
			<AppShell
				header={
					<div className="flex items-center justify-between gap-3">
						<div>
							<p className="text-lg font-semibold tracking-tight">OctoRill</p>
							<p className="text-muted-foreground text-sm">
								Shared app shell preview
							</p>
						</div>
						<Button size="sm" variant="outline" disabled>
							同步
						</Button>
					</div>
				}
				notice={<VersionUpdateNotice />}
				subheader={
					mobileChrome ? (
						<div className="-mx-1 overflow-x-auto px-1 no-scrollbar">
							<div className="flex min-w-max items-center gap-2">
								<Button
									size="sm"
									variant="outline"
									className="font-mono text-xs"
								>
									全部
								</Button>
								<Button size="sm" variant="ghost" className="font-mono text-xs">
									发布
								</Button>
								<Button size="sm" variant="ghost" className="font-mono text-xs">
									关注
								</Button>
								<Button size="sm" variant="ghost" className="font-mono text-xs">
									收件箱
								</Button>
							</div>
						</div>
					) : null
				}
				subheaderClassName={mobileChrome ? "sm:hidden" : undefined}
				footer={<AppMetaFooter />}
				mobileChrome={mobileChrome}
			>
				<div className="space-y-4">
					<section className="rounded-2xl border bg-card p-6 shadow-sm">
						<h2 className="text-base font-semibold">Dashboard shell</h2>
						<p className="text-muted-foreground mt-2 text-sm leading-relaxed">
							用于预览全站壳层在稳态、未知版本与检测到新版本后的顶部轻提示表现。
						</p>
					</section>
					<section className="grid gap-3 md:grid-cols-2">
						<div className="rounded-2xl border bg-card p-5 shadow-sm">
							<p className="text-sm font-medium">Loaded shell content</p>
							<p className="text-muted-foreground mt-2 text-sm">
								顶部提示只在检测到服务端版本变化时出现，不会遮挡主体内容。
							</p>
						</div>
						<div className="rounded-2xl border bg-card p-5 shadow-sm">
							<p className="text-sm font-medium">Footer semantics</p>
							<p className="text-muted-foreground mt-2 text-sm">
								footer 始终显示当前页实际加载版本，而不是尚未刷新的未来版本。
							</p>
						</div>
					</section>
					{mobileChrome ? (
						<section className="grid gap-3">
							{Array.from({ length: 8 }, (_, index) => {
								const blockNumber = index + 1;
								return (
									<div
										key={`mobile-shell-block-${blockNumber}`}
										className="rounded-2xl border bg-card p-5 shadow-sm"
									>
										<p className="text-sm font-medium">
											Mobile shell block #{blockNumber}
										</p>
										<p className="text-muted-foreground mt-2 text-sm">
											用于在移动端视口下验证 sticky subheader、compact header 与
											footer auto-hide。
										</p>
									</div>
								);
							})}
						</section>
					) : null}
				</div>
			</AppShell>
		</VersionMonitorStateProvider>
	);
}

const meta = {
	title: "Layout/App Shell",
	component: AppShellPreview,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"App shell 负责承载共享 header / notice / footer。这里重点验证版本轮询发现服务端更新后，顶部轻提示是否足够克制，以及 footer 是否仍保持当前页实际加载版本语义。",
			},
		},
	},
	args: {
		scenario: "steady",
		onRefresh: fn(),
		mobileChrome: false,
	},
	argTypes: {
		scenario: {
			control: "inline-radio",
			options: ["steady", "update", "unknown"],
		},
		onRefresh: {
			control: false,
		},
		mobileChrome: {
			control: "boolean",
		},
	},
} satisfies Meta<typeof AppShellPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Stable: Story = {
	parameters: {
		docs: {
			description: {
				story: "默认稳态：顶部不出现更新提示，footer 展示当前页已加载版本。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.queryByText(/检测到新版本/)).not.toBeInTheDocument();
		await expect(canvas.getByText("Version v2.4.6")).toBeVisible();
	},
};

export const UpdateAvailable: Story = {
	args: {
		scenario: "update",
	},
	parameters: {
		docs: {
			description: {
				story:
					"服务端版本变化后，顶部轻提示显示新版本号；footer 仍保持旧的 loadedVersion，直到用户主动刷新。",
			},
		},
	},
	play: async ({ args, canvasElement, userEvent }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText(/检测到新版本/)).toBeVisible();
		await expect(canvas.getByText("Version v2.4.6")).toBeVisible();
		await userEvent.click(canvas.getByRole("button", { name: "刷新" }));
		await expect(args.onRefresh).toHaveBeenCalled();
	},
};

export const UnknownFallback: Story = {
	args: {
		scenario: "unknown",
	},
	parameters: {
		docs: {
			description: {
				story:
					"首次版本解析失败时，footer 降级为 unknown，且不误报顶部更新提示。",
			},
		},
	},
};

export const EvidenceUpdateNotice: Story = {
	name: "Evidence / Update Notice",
	args: {
		scenario: "update",
	},
	parameters: {
		docs: {
			disable: true,
		},
	},
};

export const MobileChrome: Story = {
	args: {
		scenario: "steady",
		mobileChrome: true,
	},
	parameters: {
		docs: {
			description: {
				story:
					"移动端壳层预览：启用 compact header / sticky rail / footer auto-hide 的共享壳层能力，供浏览器窄屏审阅。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvasElement.querySelector("[data-app-shell-mobile-chrome='true']"),
		).not.toBeNull();
		await expect(
			canvasElement.querySelector("[data-app-meta-footer-hidden='false']"),
		).not.toBeNull();
		await expect(canvas.getByText("Mobile shell block #1")).toBeVisible();
	},
};
