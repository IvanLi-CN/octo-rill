import type { Meta, StoryObj } from "@storybook/react-vite";
import { fn } from "storybook/test";

import type { RepoPublicReleasePublicationStatusResponse } from "@/api";
import {
	RepoPublicReleaseControls,
	type RepoPublicReleaseControlsProps,
} from "@/dashboard/RepoPublicReleaseControls";

const publicStatus: RepoPublicReleasePublicationStatusResponse = {
	repo_full_name: "IvanLi-CN/octo-rill",
	public_path: "/IvanLi-CN/octo-rill/releases",
	visibility: "github_public",
	publication_state: "github_public",
	can_publish: false,
	can_unpublish: false,
	last_sync_status: "ready",
	published_at: null,
	reason: "github_public_default",
};

const privateUnpublishedStatus: RepoPublicReleasePublicationStatusResponse = {
	repo_full_name: "IvanLi-CN/private-kit",
	public_path: "/IvanLi-CN/private-kit/releases",
	visibility: "private",
	publication_state: "private_owner_unpublished",
	can_publish: true,
	can_unpublish: false,
	last_sync_status: null,
	published_at: null,
	reason: "owner_private_repo_unpublished",
};

const privatePublishedStatus: RepoPublicReleasePublicationStatusResponse = {
	repo_full_name: "IvanLi-CN/private-kit",
	public_path: "/IvanLi-CN/private-kit/releases",
	visibility: "private",
	publication_state: "private_owner_published",
	can_publish: false,
	can_unpublish: true,
	last_sync_status: "ready",
	published_at: "2026-07-07T18:20:00Z",
	reason: null,
};

function FramedControls(props: RepoPublicReleaseControlsProps) {
	return (
		<div className="max-w-[420px] rounded-[28px] border border-border/70 bg-card/82 p-5 shadow-sm">
			<RepoPublicReleaseControls {...props} />
		</div>
	);
}

const meta = {
	title: "Dashboard/RepoPublicReleaseControls",
	component: FramedControls,
	args: {
		onPublish: fn(),
		onUnpublish: fn(),
		onCopy: fn(),
	},
	parameters: {
		layout: "centered",
	},
} satisfies Meta<typeof FramedControls>;

export default meta;
type Story = StoryObj<typeof meta>;

export const PublicRepo: Story = {
	args: {
		status: publicStatus,
		publicUrl: "http://127.0.0.1:55174/IvanLi-CN/octo-rill/releases",
	},
};

export const PrivateUnpublished: Story = {
	args: {
		status: privateUnpublishedStatus,
		publicUrl: "http://127.0.0.1:55174/IvanLi-CN/private-kit/releases",
	},
};

export const PrivatePublished: Story = {
	args: {
		status: privatePublishedStatus,
		publicUrl: "http://127.0.0.1:55174/IvanLi-CN/private-kit/releases",
	},
};

export const Loading: Story = {
	args: {
		status: null,
		publicUrl: null,
		loading: true,
	},
};

export const Failure: Story = {
	args: {
		status: null,
		publicUrl: null,
		error: "无法读取公开发布状态",
	},
};
