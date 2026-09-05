import type { Meta, StoryObj } from "@storybook/react-vite";
import type { ReactNode } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";

import { LlmCallDiagnosticDetail } from "@/admin/LlmCallDiagnosticDetail";
import {
	expiredCall,
	failedCall,
} from "@/stories/AiOperationsRecordsSection.stories";

const meta = {
	title: "Admin/LlmCallDiagnosticDetail",
	component: LlmCallDiagnosticDetail,
	tags: ["autodocs"],
	parameters: {
		viewport: { viewports: INITIAL_VIEWPORTS, defaultViewport: "desktop" },
	},
} satisfies Meta<typeof LlmCallDiagnosticDetail>;

export default meta;
type Story = StoryObj<typeof meta>;

function EvidenceSurface({ children }: { children: ReactNode }) {
	return (
		<div
			data-visual-evidence-surface
			className="mx-auto box-border w-full max-w-[1072px] bg-background p-6"
		>
			<div data-visual-evidence-target className="mx-auto max-w-5xl">
				{children}
			</div>
		</div>
	);
}

export const FailedResponseWithDiagnostics: Story = {
	args: { detail: failedCall },
	render: (args) => (
		<EvidenceSurface>
			<LlmCallDiagnosticDetail detail={args.detail} />
		</EvidenceSurface>
	),
};

export const ExpiredDiagnosticEvidence: Story = {
	args: { detail: expiredCall },
	render: (args) => (
		<EvidenceSurface>
			<LlmCallDiagnosticDetail detail={args.detail} />
		</EvidenceSurface>
	),
};

export const MobileFailedResponse: Story = {
	args: { detail: failedCall },
	parameters: {
		viewport: { viewports: INITIAL_VIEWPORTS, defaultViewport: "iphone12" },
	},
	render: (args) => (
		<EvidenceSurface>
			<LlmCallDiagnosticDetail detail={args.detail} />
		</EvidenceSurface>
	),
};
