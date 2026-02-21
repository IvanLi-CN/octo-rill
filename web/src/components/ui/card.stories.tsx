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
