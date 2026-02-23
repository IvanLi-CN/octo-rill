import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useState } from "react";

import { AppMetaFooter } from "@/layout/AppMetaFooter";

type FooterPreviewProps = {
	delayMs: number;
	mockMode: "success" | "failure";
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
			{mockReady ? <AppMetaFooter /> : null}
		</div>
	);
}

const meta = {
	title: "Layout/AppMetaFooter",
	component: FooterPreview,
	parameters: {
		layout: "fullscreen",
	},
	args: {
		delayMs: 0,
		mockMode: "success",
		mockVersion: "0.1.0",
	},
	argTypes: {
		mockMode: {
			control: "inline-radio",
			options: ["success", "failure"],
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

export const Default: Story = {};

export const BuildMetadataVersion: Story = {
	args: {
		mockVersion: "0.2.0-beta.1+git.abc123",
	},
};

export const RequestFailed: Story = {
	args: {
		mockMode: "failure",
	},
};
