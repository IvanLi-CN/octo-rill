import { Download, RefreshCcw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useVersionMonitor } from "@/version/versionMonitor";

export function VersionUpdateNotice() {
	const {
		availableVersion,
		canInstallPwa = false,
		hasServiceWorkerUpdate,
		hasUpdate,
		isPwaInstalled = false,
		promptInstallPwa,
		refreshPage,
	} = useVersionMonitor();
	const showInstallAction = canInstallPwa && !isPwaInstalled;

	if (!hasUpdate && !hasServiceWorkerUpdate && !showInstallAction) {
		return null;
	}

	const updateMessage = availableVersion ? (
		<>
			检测到新版本{" "}
			<span className="font-mono font-medium">{availableVersion}</span>
			，按需刷新即可。
		</>
	) : hasServiceWorkerUpdate ? (
		"检测到新前端版本，按需刷新即可。"
	) : (
		"检测到新版本，按需刷新即可。"
	);
	const message =
		(hasUpdate || hasServiceWorkerUpdate) && showInstallAction ? (
			availableVersion ? (
				<>
					检测到新版本{" "}
					<span className="font-mono font-medium">{availableVersion}</span>
					，也可安装为独立应用。
				</>
			) : (
				"检测到新前端版本，也可安装为独立应用。"
			)
		) : hasUpdate || hasServiceWorkerUpdate ? (
			updateMessage
		) : (
			"可安装为独立应用，之后可从系统直接启动。"
		);

	return (
		<div
			className="border-b border-amber-200/70 bg-amber-50/80 dark:border-amber-500/25 dark:bg-amber-500/10"
			data-version-update-notice
		>
			<div className="mx-auto flex max-w-6xl flex-col gap-2 px-6 py-2 sm:min-h-10 sm:flex-row sm:items-center sm:justify-between sm:gap-3">
				<p className="min-w-0 text-[13px] text-amber-950/85 dark:text-amber-100/90 sm:text-sm">
					{message}
				</p>
				<div className="flex flex-wrap items-center gap-2 sm:justify-end">
					{showInstallAction ? (
						<Button
							type="button"
							variant="outline"
							size="sm"
							className="h-7 border-amber-300/70 bg-amber-100/70 px-2.5 text-xs text-amber-950 hover:bg-amber-100 hover:text-amber-950 dark:border-amber-400/30 dark:bg-amber-500/10 dark:text-amber-50 dark:hover:bg-amber-500/20 dark:hover:text-amber-50"
							data-pwa-install-action
							onClick={() => {
								void promptInstallPwa?.();
							}}
						>
							<Download className="size-3.5" />
							安装
						</Button>
					) : null}
					{hasUpdate || hasServiceWorkerUpdate ? (
						<Button
							type="button"
							variant="ghost"
							size="sm"
							className="h-7 px-2.5 text-xs text-amber-950 hover:bg-amber-100 hover:text-amber-950 dark:text-amber-50 dark:hover:bg-amber-500/20 dark:hover:text-amber-50"
							onClick={refreshPage}
						>
							<RefreshCcw className="size-3.5" />
							刷新
						</Button>
					) : null}
				</div>
			</div>
		</div>
	);
}
