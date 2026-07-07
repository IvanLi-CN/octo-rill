import {
	Copy,
	ExternalLink,
	Globe2,
	LoaderCircle,
	LockKeyhole,
	UploadCloud,
} from "lucide-react";

import type { RepoPublicReleasePublicationStatusResponse } from "@/api";
import {
	AlertDialog,
	AlertDialogAction,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
	AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";

export type RepoPublicReleaseControlsProps = {
	status: RepoPublicReleasePublicationStatusResponse | null;
	publicUrl: string | null;
	loading?: boolean;
	busy?: "publish" | "unpublish" | null;
	error?: string | null;
	onPublish?: () => void;
	onUnpublish?: () => void;
	onCopy?: () => void;
};

function statusLabel(status: RepoPublicReleasePublicationStatusResponse) {
	switch (status.publication_state) {
		case "github_public":
			return "公开仓库 · 可直接访问";
		case "private_owner_published":
			return "已发布 OctoRill 公开页";
		case "private_owner_unpublished":
			return "私有仓库 · 未发布";
		case "not_publishable":
			return "不可发布";
	}
}

function statusDescription(status: RepoPublicReleasePublicationStatusResponse) {
	switch (status.publication_state) {
		case "github_public":
			return "GitHub 公开仓库无需发布开关。";
		case "private_owner_published":
			return "未登录用户可通过这个地址查看 Release。";
		case "private_owner_unpublished":
			return "发布后只公开 Release 页，不改变 GitHub 仓库权限。";
		case "not_publishable":
			return "只有当前 GitHub viewer 拥有的个人私有仓库可以发布。";
	}
}

export function RepoPublicReleaseControls(
	props: RepoPublicReleaseControlsProps,
) {
	const {
		status,
		publicUrl,
		loading = false,
		busy = null,
		error = null,
		onPublish,
		onUnpublish,
		onCopy,
	} = props;

	if (loading) {
		return (
			<section className="mt-4 border-t border-border/60 pt-4">
				<div className="h-5 w-36 rounded-full bg-muted/55" />
				<div className="mt-3 h-9 w-full rounded-lg bg-muted/35" />
			</section>
		);
	}

	if (!status) {
		if (!error) return null;
		return (
			<section className="mt-4 border-t border-border/60 pt-4 text-sm text-muted-foreground">
				{error}
			</section>
		);
	}

	if (status.publication_state === "not_publishable") {
		return null;
	}

	const isPrivate = status.visibility === "private";
	const Icon = isPrivate ? LockKeyhole : Globe2;
	const canCopyOrOpen =
		Boolean(publicUrl) &&
		(status.publication_state === "github_public" ||
			status.publication_state === "private_owner_published");

	return (
		<section className="mt-4 border-t border-border/60 pt-4">
			<div className="flex items-start gap-3">
				<span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-full border border-border/70 bg-background/72 text-foreground">
					<Icon className="size-4" />
				</span>
				<div className="min-w-0 flex-1">
					<div className="flex flex-wrap items-center gap-2">
						<p className="text-sm font-semibold text-foreground">
							{statusLabel(status)}
						</p>
						{status.last_sync_status ? (
							<span className="rounded-full border border-border/65 bg-muted/35 px-2 py-0.5 font-mono text-[10px] uppercase text-muted-foreground">
								{status.last_sync_status}
							</span>
						) : null}
					</div>
					<p className="mt-1 text-xs leading-5 text-muted-foreground">
						{statusDescription(status)}
					</p>
				</div>
			</div>

			<div className="mt-3 flex flex-wrap gap-2">
				{status.can_publish ? (
					<Button
						type="button"
						size="sm"
						onClick={onPublish}
						disabled={busy !== null}
					>
						{busy === "publish" ? (
							<LoaderCircle className="size-4 animate-spin" />
						) : (
							<UploadCloud className="size-4" />
						)}
						发布公开页
					</Button>
				) : null}
				{canCopyOrOpen ? (
					<>
						<Button
							type="button"
							size="sm"
							variant="outline"
							onClick={onCopy}
							disabled={busy !== null}
						>
							<Copy className="size-4" />
							复制地址
						</Button>
						<Button size="sm" variant="outline" asChild>
							<a href={publicUrl ?? "#"} target="_blank" rel="noreferrer">
								<ExternalLink className="size-4" />
								跳转查看
							</a>
						</Button>
					</>
				) : null}
				{status.can_unpublish ? (
					<AlertDialog>
						<AlertDialogTrigger asChild>
							<Button
								type="button"
								size="sm"
								variant="ghost"
								disabled={busy !== null}
							>
								{busy === "unpublish" ? (
									<LoaderCircle className="size-4 animate-spin" />
								) : null}
								取消发布
							</Button>
						</AlertDialogTrigger>
						<AlertDialogContent>
							<AlertDialogHeader>
								<AlertDialogTitle>取消公开 Release 页？</AlertDialogTitle>
								<AlertDialogDescription>
									取消后，未登录用户将不能继续通过公开地址查看这个私有仓库的
									Release。
								</AlertDialogDescription>
							</AlertDialogHeader>
							<AlertDialogFooter>
								<AlertDialogCancel>保留发布</AlertDialogCancel>
								<AlertDialogAction variant="destructive" onClick={onUnpublish}>
									取消发布
								</AlertDialogAction>
							</AlertDialogFooter>
						</AlertDialogContent>
					</AlertDialog>
				) : null}
			</div>
		</section>
	);
}
