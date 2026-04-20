import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState, type ReactNode } from "react";

import { Label } from "./label";
import { Switch } from "./switch";

function StoryPanel(props: {
	title: string;
	description: string;
	children: ReactNode;
}) {
	const { title, description, children } = props;
	return (
		<div className="w-[280px] rounded-xl border bg-card p-5 shadow-sm">
			<div className="mb-4 space-y-1">
				<p className="text-sm font-medium">{title}</p>
				<p className="text-muted-foreground text-xs leading-5">{description}</p>
			</div>
			{children}
		</div>
	);
}

const meta = {
	title: "UI/Switch",
	component: Switch,
	args: {
		checked: false,
		"aria-label": "Example switch",
	},
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"OctoRill 的基础开关原语，用于设置页这类二元开关场景。这里重点验证 checked / unchecked / disabled 的轨道配色与拇指位置，避免出现‘看起来像没开’的假阳性状态。",
			},
		},
	},
} satisfies Meta<typeof Switch>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Checked: Story = {
	args: {
		checked: true,
		"aria-label": "Checked switch",
	},
};

export const Interactive: Story = {
	render: (args) => {
		const [checked, setChecked] = useState(args.checked ?? false);
		return (
			<div className="flex items-center gap-3 rounded-xl border bg-card px-4 py-3 shadow-sm">
				<Switch
					{...args}
					checked={checked}
					onCheckedChange={setChecked}
					aria-label="Interactive switch"
				/>
				<span className="text-sm font-medium text-foreground">
					{checked ? "Enabled" : "Disabled"}
				</span>
			</div>
		);
	},
};

export const Gallery: Story = {
	render: () => (
		<div className="grid gap-4 md:grid-cols-2">
			<StoryPanel
				title="Unchecked"
				description="默认关闭态，保持中性轨道与清晰的拇指边界。"
			>
				<div className="flex items-center justify-between gap-3 rounded-xl border border-border/70 bg-background/80 px-4 py-3">
					<div className="space-y-1">
						<Label htmlFor="switch-gallery-unchecked">我的发布</Label>
						<p className="text-muted-foreground text-xs">
							仅显示真实已加星仓库的发布
						</p>
					</div>
					<Switch
						id="switch-gallery-unchecked"
						checked={false}
						aria-label="Unchecked gallery switch"
					/>
				</div>
			</StoryPanel>
			<StoryPanel
				title="Checked"
				description="开启态使用主色轨道，确保一眼能看出已启用。"
			>
				<div className="flex items-center justify-between gap-3 rounded-xl border border-border/70 bg-background/80 px-4 py-3">
					<div className="space-y-1">
						<Label htmlFor="switch-gallery-checked">我的发布</Label>
						<p className="text-muted-foreground text-xs">
							把 owner 仓库的 release 纳入阅读面
						</p>
					</div>
					<Switch
						id="switch-gallery-checked"
						checked
						aria-label="Checked gallery switch"
					/>
				</div>
			</StoryPanel>
			<StoryPanel
				title="Disabled off"
				description="加载中或保存中时维持可辨识但不可交互的关闭态。"
			>
				<div className="flex items-center justify-between gap-3 rounded-xl border border-border/70 bg-background/80 px-4 py-3">
					<div className="space-y-1">
						<Label htmlFor="switch-gallery-disabled-off">同步中</Label>
						<p className="text-muted-foreground text-xs">当前设置正在保存</p>
					</div>
					<Switch
						id="switch-gallery-disabled-off"
						checked={false}
						disabled
						aria-label="Disabled off gallery switch"
					/>
				</div>
			</StoryPanel>
			<StoryPanel
				title="Disabled on"
				description="开启后进入保存态时，仍保留主色轨道语义，不会看起来像被关掉。"
			>
				<div className="flex items-center justify-between gap-3 rounded-xl border border-border/70 bg-background/80 px-4 py-3">
					<div className="space-y-1">
						<Label htmlFor="switch-gallery-disabled-on">我的发布</Label>
						<p className="text-muted-foreground text-xs">
							保存过程中保持开启视觉反馈
						</p>
					</div>
					<Switch
						id="switch-gallery-disabled-on"
						checked
						disabled
						aria-label="Disabled on gallery switch"
					/>
				</div>
			</StoryPanel>
		</div>
	),
	parameters: {
		docs: {
			description: {
				story:
					"Gallery 视图集中检查开关在真实设置场景中的四种常用状态，方便后续 UI 回归时直接对照颜色与禁用反馈。",
			},
		},
	},
};
