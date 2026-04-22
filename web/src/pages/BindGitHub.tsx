import { useEffect, useMemo, useState } from "react";
import { Fingerprint, Link2, LoaderCircle, ShieldAlert } from "lucide-react";

import { apiGetAuthBindContext, type AuthBindContextResponse } from "@/api";
import { AuthProviderIcon } from "@/components/brand/AuthProviderIcon";
import { BrandLogo } from "@/components/brand/BrandLogo";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { ThemeToggle } from "@/components/theme/ThemeToggle";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { InternalLink } from "@/lib/internalNavigation";
import { cn } from "@/lib/utils";

const LINUXDO_BIND_STATUS_META: Record<
	string,
	{
		tone: "success" | "error";
		title: string;
		description: string;
	}
> = {
	connected: {
		tone: "success",
		title: "可以继续绑定 GitHub",
		description: "完成 GitHub 绑定后，即可继续登录。",
	},
	linuxdo_already_bound: {
		tone: "error",
		title: "LinuxDO 账号已被占用",
		description:
			"这个 LinuxDO 账号已经绑定到其他 OctoRill 账号，不能继续补绑。",
	},
	linuxdo_account_conflict: {
		tone: "error",
		title: "当前账号已绑定其他 LinuxDO",
		description:
			"当前 GitHub 账号对应的 OctoRill 账号已经绑定了其他 LinuxDO，不能自动合并。",
	},
	github_already_bound: {
		tone: "error",
		title: "GitHub 账号已被占用",
		description:
			"这个 GitHub 账号已经绑定到其他 OctoRill 账号，请换一个 GitHub 再试。",
	},
	not_configured: {
		tone: "error",
		title: "未启用 LinuxDO 登录",
		description: "当前环境没有启用 LinuxDO 登录，暂时无法继续。",
	},
	state_mismatch: {
		tone: "error",
		title: "LinuxDO 状态校验失败",
		description: "登录状态已失效，请重新从 LinuxDO 登录入口发起。",
	},
	exchange_failed: {
		tone: "error",
		title: "LinuxDO 授权失败",
		description: "服务端没能完成 LinuxDO 授权交换，请稍后再试。",
	},
	fetch_user_failed: {
		tone: "error",
		title: "读取 LinuxDO 账号失败",
		description: "授权成功后没能读取 LinuxDO 用户资料，请稍后重试。",
	},
};

const PASSKEY_BIND_STATUS_META: Record<
	string,
	{
		tone: "success" | "error";
		title: string;
		description: string;
	}
> = {
	created: {
		tone: "success",
		title: "Passkey 已暂存",
		description: "完成 GitHub 绑定后，这把 Passkey 就会挂接到你的账号上。",
	},
	passkey_retry_required: {
		tone: "error",
		title: "需要重新添加 Passkey",
		description:
			"这次 GitHub 绑定没有自动挂接之前创建的 Passkey，请登录后到设置页重新添加。",
	},
	passkey_already_bound: {
		tone: "error",
		title: "Passkey 已被占用",
		description: "这把 Passkey 已经绑定到其他 OctoRill 账号，不能重复挂接。",
	},
	expired: {
		tone: "error",
		title: "Passkey 已过期",
		description: "之前创建的 Passkey 暂存状态已经过期，请重新创建一次。",
	},
};

function statusToneClassName(tone: "success" | "error") {
	return tone === "success"
		? "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
		: "border-destructive/30 bg-destructive/8 text-destructive";
}

function formatDateTime(value: string) {
	const date = new Date(value);
	return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

export function BindGitHubPage(props: {
	linuxdoStatus?: string | null;
	passkeyStatus?: string | null;
}) {
	const { linuxdoStatus = null, passkeyStatus = null } = props;
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState<string | null>(null);
	const [context, setContext] = useState<AuthBindContextResponse | null>(null);

	useEffect(() => {
		setLoading(true);
		setError(null);
		void apiGetAuthBindContext()
			.then((res) => {
				setContext(res);
			})
			.catch((err) => {
				setError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				setLoading(false);
			});
	}, []);

	const activeLinuxDoStatus = useMemo(
		() =>
			linuxdoStatus ? (LINUXDO_BIND_STATUS_META[linuxdoStatus] ?? null) : null,
		[linuxdoStatus],
	);
	const activePasskeyStatus = useMemo(
		() =>
			passkeyStatus ? (PASSKEY_BIND_STATUS_META[passkeyStatus] ?? null) : null,
		[passkeyStatus],
	);
	const hasPendingContext =
		context?.pending_linuxdo !== null || context?.pending_passkey !== null;

	return (
		<AppShell
			header={
				<div className="flex items-center justify-between gap-3">
					<div className="flex items-center gap-3">
						<BrandLogo variant="wordmark" className="h-8" />
						<div>
							<p className="text-muted-foreground text-xs font-medium tracking-[0.18em] uppercase">
								GitHub 绑定
							</p>
							<h1 className="text-xl font-semibold tracking-tight text-foreground">
								继续绑定 GitHub
							</h1>
						</div>
					</div>
					<ThemeToggle />
				</div>
			}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<div className="mx-auto max-w-2xl space-y-4 py-4">
				{activePasskeyStatus ? (
					<section
						className={cn(
							"rounded-xl border px-3 py-2.5 text-sm shadow-sm",
							statusToneClassName(activePasskeyStatus.tone),
						)}
					>
						<div className="flex items-start gap-2.5">
							{activePasskeyStatus.tone === "error" ? (
								<ShieldAlert className="mt-0.5 size-4 shrink-0" />
							) : (
								<Fingerprint className="mt-0.5 size-4 shrink-0" />
							)}
							<div className="space-y-0.5">
								<p className="font-medium">{activePasskeyStatus.title}</p>
								<p className="text-xs leading-5">
									{activePasskeyStatus.description}
								</p>
							</div>
						</div>
					</section>
				) : null}

				{activeLinuxDoStatus ? (
					<section
						className={cn(
							"rounded-xl border px-3 py-2.5 text-sm shadow-sm",
							statusToneClassName(activeLinuxDoStatus.tone),
						)}
					>
						<div className="flex items-start gap-2.5">
							{activeLinuxDoStatus.tone === "error" ? (
								<ShieldAlert className="mt-0.5 size-4 shrink-0" />
							) : (
								<Link2 className="mt-0.5 size-4 shrink-0" />
							)}
							<div className="space-y-0.5">
								<p className="font-medium">{activeLinuxDoStatus.title}</p>
								<p className="text-xs leading-5">
									{activeLinuxDoStatus.description}
								</p>
							</div>
						</div>
					</section>
				) : null}

				<Card className="border-border/70 shadow-sm">
					<CardHeader>
						<CardTitle>继续绑定 GitHub</CardTitle>
						<CardDescription>
							{context?.pending_passkey
								? "完成 GitHub 绑定后，暂存的 Passkey 会和账号一起落地。"
								: "补上 GitHub 绑定后，才能把待处理的登录上下文正式挂到账号上。"}
						</CardDescription>
					</CardHeader>
					<CardContent className="space-y-4">
						{loading ? (
							<div className="text-muted-foreground flex items-center gap-2 text-sm">
								<LoaderCircle className="size-4 animate-spin" />
								正在读取绑定信息…
							</div>
						) : error ? (
							<div className="rounded-xl border border-destructive/30 bg-destructive/8 px-3 py-2.5 text-sm text-destructive">
								{error}
							</div>
						) : hasPendingContext ? (
							<>
								{context?.pending_passkey ? (
									<div className="space-y-2 rounded-2xl border border-border/70 bg-muted/20 p-4">
										<div className="flex items-center gap-2 text-sm font-semibold text-foreground">
											<Fingerprint className="size-4" />
											待挂接的 Passkey
										</div>
										<p className="text-sm text-foreground">
											{context.pending_passkey.label}
										</p>
										<p className="text-muted-foreground text-xs leading-5">
											创建于{" "}
											{formatDateTime(context.pending_passkey.created_at)}
										</p>
									</div>
								) : null}

								{context?.pending_linuxdo ? (
									<div className="flex items-center gap-3 rounded-2xl border border-border/70 bg-muted/20 p-4">
										<img
											src={context.pending_linuxdo.avatar_url}
											alt={`${context.pending_linuxdo.username} avatar`}
											className="size-12 rounded-full border border-border/70 object-cover"
											referrerPolicy="no-referrer"
										/>
										<div className="min-w-0">
											<p className="truncate text-sm font-semibold text-foreground">
												{context.pending_linuxdo.name ??
													context.pending_linuxdo.username}
											</p>
											<p className="text-muted-foreground truncate text-xs">
												@{context.pending_linuxdo.username} · Trust level{" "}
												{context.pending_linuxdo.trust_level}
											</p>
										</div>
									</div>
								) : null}

								<div className="grid gap-3 sm:grid-cols-2">
									<Button asChild size="lg" className="h-11 rounded-2xl">
										<a href="/auth/github/login">
											<AuthProviderIcon provider="github" />
											绑定 GitHub 并继续
										</a>
									</Button>
									<Button
										asChild
										variant="outline"
										size="lg"
										className="h-11 rounded-2xl"
									>
										<InternalLink href="/" to="/">
											返回首页
										</InternalLink>
									</Button>
								</div>
							</>
						) : (
							<div className="space-y-3 rounded-2xl border border-border/70 bg-muted/20 p-4 text-sm">
								<p className="font-medium text-foreground">
									当前没有待完成的绑定上下文。
								</p>
								<p className="text-muted-foreground leading-6">
									如果你想通过 Passkey 或 LinuxDO
									首次进入，请先从首页重新开始；如果只是想给当前账号增加 GitHub
									绑定，请登录后前往设置页操作。
								</p>
								<div className="flex flex-wrap gap-2">
									<Button asChild variant="outline" size="sm">
										<InternalLink href="/" to="/">
											返回首页
										</InternalLink>
									</Button>
									<Button asChild size="sm">
										<a href="/auth/github/login">
											<AuthProviderIcon provider="github" />
											继续使用 GitHub 登录
										</a>
									</Button>
								</div>
							</div>
						)}
					</CardContent>
				</Card>
			</div>
		</AppShell>
	);
}
