import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "./button";
import {
	Card,
	CardContent,
	CardDescription,
	CardFooter,
	CardHeader,
	CardTitle,
} from "./card";

const meta = {
	title: "UI/Card",
	component: Card,
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Card 是 Dashboard 与侧栏区块最常见的容器。这里用最小内容和带操作的卡片来校验标题、描述、正文与 footer 的层级关系。\n\n相关公开文档：[产品说明](../product.html) · [Storybook 入口](../storybook.html)",
			},
		},
	},
} satisfies Meta<typeof Card>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Basic: Story = {
	render: () => (
		<Card className="w-[360px]">
			<CardHeader>
				<CardTitle>Daily brief</CardTitle>
				<CardDescription>
					Generate a summary of recent release updates.
				</CardDescription>
			</CardHeader>
			<CardContent>
				<p className="text-sm">No brief generated yet.</p>
			</CardContent>
		</Card>
	),
};

export const WithActions: Story = {
	parameters: {
		docs: {
			description: {
				story: "补充带主次操作的卡片状态，模拟同步或处理动作入口。",
			},
		},
	},
	render: () => (
		<Card className="w-[360px]">
			<CardHeader>
				<CardTitle>Sync notifications</CardTitle>
				<CardDescription>
					Pull your latest GitHub inbox threads.
				</CardDescription>
			</CardHeader>
			<CardContent>
				<p className="text-sm">12 unread notifications from 4 repositories.</p>
			</CardContent>
			<CardFooter className="gap-2">
				<Button>Sync now</Button>
				<Button variant="outline">Dismiss</Button>
			</CardFooter>
		</Card>
	),
};
