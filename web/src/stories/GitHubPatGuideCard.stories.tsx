import type { Meta, StoryObj } from "@storybook/react-vite";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, within } from "storybook/test";

import { GitHubPatGuideCard } from "@/settings/GitHubPatGuideCard";
import { ThemeProvider } from "@/theme/ThemeProvider";
import type { ThemePreference } from "@/theme/theme";

const GITHUB_PAT_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	githubPatMobile390: {
		name: "GitHub PAT mobile 390x844",
		styles: {
			height: "844px",
			width: "390px",
		},
		type: "mobile",
	},
	githubPatDesktop1280: {
		name: "GitHub PAT desktop 1280x1000",
		styles: {
			height: "1000px",
			width: "1280px",
		},
		type: "desktop",
	},
} as const;

type StoryArgs = {
	themePreference: ThemePreference;
};

function Scene(args: StoryArgs) {
	return (
		<ThemeProvider defaultPreference={args.themePreference} persist={false}>
			<GitHubPatGuideCard />
		</ThemeProvider>
	);
}

const meta = {
	title: "Mocks/GitHub PAT 1:1",
	component: Scene,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		viewport: {
			options: GITHUB_PAT_VIEWPORTS,
		},
		docs: {
			description: {
				component:
					"GitHub classic PAT 的高仿 DOM mock，直接预填建议值，方便用户逐项照抄。",
			},
		},
	},
	args: {
		themePreference: "light",
	},
} satisfies Meta<typeof Scene>;

export default meta;
type Story = StoryObj<typeof meta>;

async function assertReplicaVisible(canvasElement: HTMLElement) {
	const canvas = within(canvasElement);
	await expect(
		canvas.getByRole("heading", { name: /new personal access token/i }),
	).toBeVisible();
	await expect(canvas.getByRole("textbox", { name: "Note" })).toHaveValue(
		"OctoRill release feedback",
	);
	await expect(
		canvas.getByRole("button", { name: "No expiration" }),
	).toBeVisible();
	await expect(canvas.getAllByText("repo")[0]).toBeVisible();
}

export const DesktopLight: Story = {
	name: "Desktop / Light",
	globals: {
		viewport: {
			value: "githubPatDesktop1280",
			isRotated: false,
		},
	},
	args: {
		themePreference: "light",
	},
	play: async ({ canvasElement }) => {
		await assertReplicaVisible(canvasElement);
	},
};

export const DesktopDark: Story = {
	name: "Desktop / Dark",
	globals: {
		viewport: {
			value: "githubPatDesktop1280",
			isRotated: false,
		},
	},
	args: {
		themePreference: "dark",
	},
	play: async ({ canvasElement }) => {
		await assertReplicaVisible(canvasElement);
	},
};

export const MobileLight: Story = {
	name: "Mobile / Light",
	globals: {
		viewport: {
			value: "githubPatMobile390",
			isRotated: false,
		},
	},
	args: {
		themePreference: "light",
	},
	play: async ({ canvasElement }) => {
		await assertReplicaVisible(canvasElement);
	},
};

export const MobileDark: Story = {
	name: "Mobile / Dark",
	globals: {
		viewport: {
			value: "githubPatMobile390",
			isRotated: false,
		},
	},
	args: {
		themePreference: "dark",
	},
	play: async ({ canvasElement }) => {
		await assertReplicaVisible(canvasElement);
	},
};
