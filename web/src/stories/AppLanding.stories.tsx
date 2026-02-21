import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import type { Meta, StoryObj } from "@storybook/react-vite";

function AppLandingPreview() {
	return (
		<div className="min-h-screen bg-background">
			<div className="mx-auto max-w-3xl px-6 py-10">
				<div className="mb-6">
					<h1 className="text-3xl font-semibold tracking-tight">OctoRill</h1>
					<p className="text-muted-foreground mt-2">
						Starred releases + Notifications + AI daily brief.
					</p>
				</div>
				<Card>
					<CardHeader>
						<CardTitle>Sign in</CardTitle>
						<CardDescription>
							Login via GitHub OAuth to sync your starred repos and inbox.
						</CardDescription>
					</CardHeader>
					<CardContent className="flex flex-col gap-3">
						<Button asChild>
							<a href="#github-login">Login with GitHub</a>
						</Button>
						<p className="text-muted-foreground text-xs">
							This Storybook example is static and does not call the backend.
						</p>
					</CardContent>
				</Card>
			</div>
		</div>
	);
}

const meta = {
	title: "Pages/AppLanding",
	component: AppLandingPreview,
	parameters: {
		layout: "fullscreen",
	},
} satisfies Meta<typeof AppLandingPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
