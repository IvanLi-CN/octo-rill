import {
	ArrowRight,
	Check,
	ChevronRight,
	Copy,
	KeyRound,
	ShieldCheck,
} from "lucide-react";
import type { ReactNode } from "react";

import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { PAT_CREATE_PATH } from "@/settings/reactionTokenEditor";

function GuideChrome(props: {
	step: string;
	title: string;
	description: string;
	children: ReactNode;
	className?: string;
}) {
	const { step, title, description, children, className } = props;
	return (
		<section
			className={cn(
				"rounded-2xl border border-slate-300/90 bg-slate-50/95 p-4 text-slate-900 shadow-[0_1px_0_rgba(15,23,42,0.04)] dark:border-slate-700 dark:bg-slate-950/60 dark:text-slate-100",
				className,
			)}
		>
			<div className="flex flex-wrap items-start justify-between gap-3">
				<div className="space-y-1">
					<p className="text-[11px] font-semibold tracking-[0.18em] text-slate-500 uppercase dark:text-slate-400">
						{step}
					</p>
					<h3 className="text-sm font-semibold text-slate-950 dark:text-slate-50">
						{title}
					</h3>
				</div>
				<Badge
					variant="outline"
					className="rounded-full border-slate-300 bg-white/90 px-2.5 py-0.5 text-[11px] text-slate-600 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300"
				>
					GitHub mock
				</Badge>
			</div>
			<p className="mt-2 text-xs leading-5 text-slate-600 dark:text-slate-300">
				{description}
			</p>
			<div className="mt-4">{children}</div>
		</section>
	);
}

function SidebarRow(props: {
	label: string;
	active?: boolean;
	nested?: boolean;
}) {
	const { label, active = false, nested = false } = props;
	return (
		<div
			className={cn(
				"flex items-center gap-2 rounded-md px-2 py-1.5 text-xs font-medium text-slate-600 dark:text-slate-300",
				nested && "ml-4 text-[11px]",
				active &&
					"bg-slate-900 text-white dark:bg-slate-100 dark:text-slate-950",
			)}
		>
			<div
				className={cn(
					"size-1.5 rounded-full bg-slate-300 dark:bg-slate-600",
					active && "bg-current",
				)}
			/>
			<span>{label}</span>
		</div>
	);
}

function ScopeRow(props: {
	scope: string;
	tone: "primary" | "muted";
	title: string;
	description: string;
}) {
	const { scope, tone, title, description } = props;
	return (
		<div
			className={cn(
				"rounded-xl border px-3 py-3",
				tone === "primary"
					? "border-emerald-200 bg-emerald-50/90 dark:border-emerald-800/70 dark:bg-emerald-950/30"
					: "border-slate-200 bg-white/85 dark:border-slate-700 dark:bg-slate-900/80",
			)}
		>
			<div className="flex items-center gap-2">
				<div
					className={cn(
						"flex size-4 items-center justify-center rounded-sm border",
						tone === "primary"
							? "border-emerald-500 bg-emerald-500 text-white"
							: "border-slate-300 bg-transparent text-transparent dark:border-slate-600",
					)}
				>
					<Check className="size-3" />
				</div>
				<code className="rounded bg-slate-900 px-1.5 py-0.5 text-[11px] font-semibold text-white dark:bg-slate-100 dark:text-slate-950">
					{scope}
				</code>
				<p className="text-xs font-semibold text-slate-900 dark:text-slate-100">
					{title}
				</p>
			</div>
			<p className="mt-2 text-[11px] leading-5 text-slate-600 dark:text-slate-300">
				{description}
			</p>
		</div>
	);
}

export function GitHubPatGuideCard() {
	return (
		<section
			data-testid="github-pat-guide-card"
			className="rounded-2xl border border-border/70 bg-card/98 p-4 shadow-sm"
		>
			<div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
				<div className="space-y-1">
					<div className="flex flex-wrap items-center gap-2">
						<h2 className="text-base font-semibold text-foreground">
							照着 GitHub 页面创建 classic PAT
						</h2>
						<Badge variant="secondary">1:1 抄作业</Badge>
					</div>
					<p className="text-muted-foreground text-sm leading-6">
						这是一个静态高仿 mock，用来告诉你去 GitHub
						哪一页点、哪些字段照着填、生成后再回到上方输入框保存。
					</p>
				</div>
				<Badge
					variant="outline"
					className="w-fit rounded-full px-3 py-1 font-mono text-[11px]"
				>
					{PAT_CREATE_PATH}
				</Badge>
			</div>

			<div className="mt-4 space-y-4">
				<div className="grid gap-4 xl:grid-cols-[0.96fr_1.04fr]">
					<GuideChrome
						step="Step 1"
						title="进入 Tokens (classic)"
						description="先按 GitHub 官方路径进入 classic token 页面，左边栏的层级要对上。"
					>
						<div className="overflow-hidden rounded-2xl border border-slate-200 bg-white dark:border-slate-700 dark:bg-slate-950">
							<div className="flex items-center justify-between border-b border-slate-200 bg-slate-100/90 px-3 py-2 dark:border-slate-700 dark:bg-slate-900">
								<div className="flex items-center gap-2">
									<div className="size-6 rounded-full bg-slate-950 dark:bg-slate-100" />
									<p className="text-xs font-semibold">GitHub</p>
								</div>
								<p className="text-[11px] text-slate-500 dark:text-slate-400">
									Settings
								</p>
							</div>
							<div className="grid gap-4 px-3 py-3 md:grid-cols-[160px_1fr]">
								<div className="space-y-1.5">
									<SidebarRow label="Access" />
									<SidebarRow label="Code, planning, and automation" />
									<SidebarRow label="Security" />
									<SidebarRow label="Developer settings" active />
									<SidebarRow label="GitHub Apps" nested />
									<SidebarRow label="OAuth apps" nested />
									<SidebarRow label="Personal access tokens" nested />
									<SidebarRow label="Fine-grained tokens" nested />
									<SidebarRow label="Tokens (classic)" active nested />
								</div>
								<div className="space-y-3 rounded-xl border border-slate-200 bg-slate-50/70 p-3 dark:border-slate-700 dark:bg-slate-900/70">
									<div className="flex flex-wrap items-center gap-1 text-[11px] font-medium text-slate-500 dark:text-slate-400">
										<span>Settings</span>
										<ChevronRight className="size-3" />
										<span>Developer settings</span>
										<ChevronRight className="size-3" />
										<span>Personal access tokens</span>
										<ChevronRight className="size-3" />
										<span className="font-semibold text-slate-900 dark:text-slate-100">
											Tokens (classic)
										</span>
									</div>
									<div className="rounded-xl border border-dashed border-slate-300 bg-white px-3 py-3 dark:border-slate-600 dark:bg-slate-950">
										<p className="text-sm font-semibold text-slate-900 dark:text-slate-100">
											Personal access tokens (classic)
										</p>
										<p className="mt-1 text-[11px] leading-5 text-slate-500 dark:text-slate-400">
											点击右上角 <strong>Generate new token</strong>，再选择{" "}
											<strong>Generate new token (classic)</strong>。
										</p>
									</div>
								</div>
							</div>
						</div>
					</GuideChrome>

					<GuideChrome
						step="Step 2"
						title="填写 Generate new token (classic)"
						description="把关键字段和 scope 先在脑子里对齐，再去 GitHub 填，避免生成后又因为 scope 不对重新来一遍。"
					>
						<div className="overflow-hidden rounded-2xl border border-slate-200 bg-white dark:border-slate-700 dark:bg-slate-950">
							<div className="border-b border-slate-200 px-4 py-3 dark:border-slate-700">
								<p className="text-sm font-semibold text-slate-950 dark:text-slate-50">
									Generate new token (classic)
								</p>
								<p className="mt-1 text-[11px] text-slate-500 dark:text-slate-400">
									创建后只会显示一次 token，请立刻复制保存。
								</p>
							</div>
							<div className="space-y-3 px-4 py-4">
								<div className="space-y-1.5">
									<p className="text-[11px] font-semibold text-slate-700 dark:text-slate-300">
										Note
									</p>
									<div className="rounded-md border border-slate-300 bg-slate-50 px-3 py-2 text-xs text-slate-700 dark:border-slate-600 dark:bg-slate-900 dark:text-slate-200">
										OctoRill release feedback
									</div>
								</div>
								<div className="space-y-1.5">
									<p className="text-[11px] font-semibold text-slate-700 dark:text-slate-300">
										Expiration
									</p>
									<div className="flex items-center justify-between rounded-md border border-slate-300 bg-slate-50 px-3 py-2 text-xs text-slate-700 dark:border-slate-600 dark:bg-slate-900 dark:text-slate-200">
										<span>30 days</span>
										<ChevronRight className="size-3 rotate-90" />
									</div>
								</div>
								<div className="space-y-2">
									<div className="flex items-center gap-2">
										<ShieldCheck className="size-4 text-emerald-600 dark:text-emerald-400" />
										<p className="text-xs font-semibold text-slate-900 dark:text-slate-100">
											Repository scopes（按仓库类型二选一）
										</p>
									</div>
									<ScopeRow
										scope="public_repo"
										tone="primary"
										title="公开仓库最低要求"
										description="只需要给公开仓库做 release feedback 时，勾这个就够了，对应当前后端校验的 minimum scope。"
									/>
									<ScopeRow
										scope="repo"
										tone="muted"
										title="私有仓库改选这个"
										description="如果 OctoRill 需要访问私有仓库，请改勾 repo；它比 public_repo 更宽，也能满足当前校验。"
									/>
								</div>
							</div>
						</div>
					</GuideChrome>
				</div>

				<GuideChrome
					step="Step 3"
					title="复制 token，回填到当前页面上方"
					description="生成后别停留在 GitHub；把新 token 复制回来，上方输入框会自动做 800ms 防抖校验。"
				>
					<div className="grid gap-4 lg:grid-cols-[1fr_auto_1fr]">
						<div className="rounded-2xl border border-slate-200 bg-white p-4 dark:border-slate-700 dark:bg-slate-950">
							<div className="flex items-center gap-2 text-xs font-semibold text-slate-700 dark:text-slate-300">
								<KeyRound className="size-4" />
								New personal access token
							</div>
							<div className="mt-3 flex items-center justify-between rounded-xl border border-slate-300 bg-slate-50 px-3 py-3 font-mono text-xs text-slate-800 dark:border-slate-600 dark:bg-slate-900 dark:text-slate-100">
								<span>ghp_xxxx_xxxx_classic_token</span>
								<Copy className="size-4 text-slate-500 dark:text-slate-400" />
							</div>
							<p className="mt-2 text-[11px] leading-5 text-slate-500 dark:text-slate-400">
								GitHub 只会显示一次，复制后就回到 OctoRill。
							</p>
						</div>

						<div className="flex items-center justify-center text-slate-400">
							<ArrowRight className="size-5" />
						</div>

						<div className="rounded-2xl border border-emerald-200 bg-emerald-50/90 p-4 dark:border-emerald-800/70 dark:bg-emerald-950/30">
							<div className="flex items-center gap-2 text-xs font-semibold text-emerald-800 dark:text-emerald-200">
								<Check className="size-4" />
								回到当前页面上方输入框
							</div>
							<div className="mt-3 rounded-xl border border-emerald-300/80 bg-white/90 px-3 py-3 font-mono text-xs text-slate-800 dark:border-emerald-700 dark:bg-slate-950 dark:text-slate-100">
								粘贴新的 classic PAT
							</div>
							<p className="mt-2 text-[11px] leading-5 text-emerald-900/80 dark:text-emerald-100/90">
								校验通过后再点 <strong>保存 GitHub PAT</strong>，这样当前站点的
								release feedback 才能恢复可用。
							</p>
						</div>
					</div>
				</GuideChrome>
			</div>
		</section>
	);
}
