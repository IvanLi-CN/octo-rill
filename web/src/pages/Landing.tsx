import {
	apiPostPasskeyAuthenticateOptions,
	apiPostPasskeyAuthenticateVerify,
	apiPostPasskeyRegisterOptions,
	apiPostPasskeyRegisterVerify,
} from "@/api";
import {
	browserSupportsPasskeys,
	createPasskeyCredential,
	getPasskeyCredential,
	normalizePasskeyErrorMessage,
} from "@/auth/passkeys";
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
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";
import { Inbox, Package2, Users } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

type LandingProps = {
	bootError?: string | null;
	passkeySupportOverride?: boolean | null;
};

const heroTitle = "集中查看与你相关的 GitHub 动态";
const heroDescription =
	"登录后可在同一页面查看发布更新、获星与关注动态，并使用日报与通知入口；发布内容支持中文翻译与要点整理。";
const loginCardDescription =
	"可直接使用 GitHub 或 Passkey 登录；也可以先创建 Passkey，再继续绑定 GitHub。";

const heroHighlights = [
	{
		title: "发布更新",
		description: "查看发布译文与要点",
		icon: Package2,
	},
	{
		title: "社交动态",
		description: "查看获星与关注变化",
		icon: Users,
	},
	{
		title: "日报通知",
		description: "查看日报与通知入口",
		icon: Inbox,
	},
] as const;

export function Landing({
	bootError,
	passkeySupportOverride = null,
}: LandingProps) {
	const [passkeySupported, setPasskeySupported] = useState(
		passkeySupportOverride ?? false,
	);
	const [passkeyBusyMode, setPasskeyBusyMode] = useState<
		"authenticate" | "register" | null
	>(null);
	const [passkeyError, setPasskeyError] = useState<string | null>(null);

	useEffect(() => {
		setPasskeySupported(passkeySupportOverride ?? browserSupportsPasskeys());
	}, [passkeySupportOverride]);

	const onAuthenticatePasskey = useCallback(() => {
		if (!passkeySupported) {
			setPasskeyError(
				"当前浏览器不支持 Passkey，请改用 GitHub / LinuxDO 登录。",
			);
			return;
		}
		setPasskeyBusyMode("authenticate");
		setPasskeyError(null);
		void apiPostPasskeyAuthenticateOptions()
			.then((options) => getPasskeyCredential(options, "required"))
			.then((credential) => apiPostPasskeyAuthenticateVerify(credential))
			.then((res) => {
				window.location.assign(res.next_path);
			})
			.catch((err) => {
				setPasskeyError(normalizePasskeyErrorMessage(err));
			})
			.finally(() => {
				setPasskeyBusyMode(null);
			});
	}, [passkeySupported]);

	const onRegisterPasskey = useCallback(() => {
		if (!passkeySupported) {
			setPasskeyError(
				"当前浏览器不支持 Passkey，请改用 GitHub / LinuxDO 登录。",
			);
			return;
		}
		setPasskeyBusyMode("register");
		setPasskeyError(null);
		void apiPostPasskeyRegisterOptions()
			.then((options) => createPasskeyCredential(options))
			.then((credential) => apiPostPasskeyRegisterVerify(credential))
			.then((res) => {
				if (res.next_path) {
					window.location.assign(res.next_path);
				}
			})
			.catch((err) => {
				setPasskeyError(normalizePasskeyErrorMessage(err));
			})
			.finally(() => {
				setPasskeyBusyMode(null);
			});
	}, [passkeySupported]);

	return (
		<AppShell notice={<VersionUpdateNotice />} footer={<AppMetaFooter />}>
			<div className="mx-auto max-w-6xl py-2 sm:py-4">
				<div className="mb-4 flex items-center justify-between gap-4 sm:mb-6">
					<BrandLogo variant="wordmark" className="h-8 sm:h-10" />
					<ThemeToggle />
				</div>

				<div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_360px] lg:items-center xl:grid-cols-[minmax(0,1fr)_392px] xl:gap-8">
					<section className="order-2 lg:order-1">
						<div className="rounded-[32px] border border-border/70 bg-[linear-gradient(180deg,color-mix(in_oklab,var(--color-card)_92%,transparent),color-mix(in_oklab,var(--color-card)_72%,transparent))] p-6 shadow-[0_20px_50px_rgba(15,23,42,0.07)] dark:shadow-[0_28px_60px_rgba(2,6,23,0.42)] sm:p-8 lg:p-10">
							<div className="max-w-3xl space-y-6">
								<div className="space-y-4">
									<h1 className="max-w-3xl text-balance text-4xl font-semibold tracking-tight sm:text-5xl lg:text-6xl">
										{heroTitle}
									</h1>
									<p className="text-muted-foreground max-w-2xl text-pretty text-base leading-7 sm:text-lg">
										{heroDescription}
									</p>
								</div>

								<ul className="grid gap-3 sm:grid-cols-3 lg:grid-cols-1 xl:grid-cols-3">
									{heroHighlights.map(
										({ title, description, icon: Icon }, index) => (
											<li
												key={title}
												className="flex min-h-32 flex-col rounded-2xl border border-border/70 bg-background/72 px-5 py-5 shadow-sm"
											>
												<div className="mb-5 flex items-center justify-between gap-3">
													<span className="text-muted-foreground font-mono text-xs leading-6">
														0{index + 1}
													</span>
													<Icon className="text-muted-foreground size-4" />
												</div>
												<div className="space-y-2">
													<p className="text-lg font-semibold tracking-tight">
														{title}
													</p>
													<p className="text-muted-foreground text-sm leading-6">
														{description}
													</p>
												</div>
											</li>
										),
									)}
								</ul>
							</div>
						</div>
					</section>

					<aside className="order-1 lg:order-2">
						<Card
							className="rounded-[28px] border-border/70 bg-card/96 shadow-[0_18px_45px_rgba(15,23,42,0.08)] dark:shadow-[0_24px_55px_rgba(2,6,23,0.4)]"
							data-landing-login-card
						>
							<CardHeader className="gap-3 px-5 pt-5 pb-0 sm:px-6 sm:pt-6">
								<div className="space-y-2">
									<CardTitle className="text-2xl sm:text-[2rem]">
										登录
									</CardTitle>
									<CardDescription className="text-sm leading-6 sm:text-base">
										{loginCardDescription}
									</CardDescription>
								</div>
							</CardHeader>
							<CardContent className="flex flex-col gap-3 px-5 pt-5 pb-5 sm:px-6 sm:pb-6">
								<Button
									asChild
									className="h-12 w-full rounded-2xl text-base font-semibold sm:h-14"
									data-landing-login-cta
								>
									<a href="/auth/github/login">
										<AuthProviderIcon provider="github" />
										使用 GitHub 登录
									</a>
								</Button>
								<Button
									asChild
									variant="outline"
									className="h-12 w-full rounded-2xl text-base font-semibold sm:h-14"
									data-landing-linuxdo-cta
								>
									<a href="/auth/linuxdo/login">
										<AuthProviderIcon provider="linuxdo" />
										使用 LinuxDO 登录
									</a>
								</Button>
								<Button
									type="button"
									variant="secondary"
									className="h-12 w-full rounded-2xl text-base font-semibold sm:h-14"
									onClick={onAuthenticatePasskey}
									disabled={!passkeySupported || passkeyBusyMode !== null}
									data-landing-passkey-login-cta
								>
									{passkeyBusyMode === "authenticate"
										? "正在验证 Passkey…"
										: "使用 Passkey 登录"}
								</Button>
								<Button
									type="button"
									variant="ghost"
									className="h-11 w-full rounded-2xl text-sm font-medium"
									onClick={onRegisterPasskey}
									disabled={!passkeySupported || passkeyBusyMode !== null}
									data-landing-passkey-register-cta
								>
									{passkeyBusyMode === "register"
										? "正在创建 Passkey…"
										: "首次使用？创建 Passkey 并继续绑定 GitHub"}
								</Button>

								{!passkeySupported ? (
									<div className="rounded-2xl border border-border/70 bg-muted/20 px-4 py-3 text-sm leading-6 text-muted-foreground">
										当前浏览器不支持 Passkey；你仍然可以继续使用 GitHub /
										LinuxDO 登录。
									</div>
								) : null}

								{passkeyError ? (
									<div className="rounded-2xl border border-destructive/25 bg-destructive/5 px-4 py-3 text-sm leading-6 text-destructive">
										{passkeyError}
									</div>
								) : null}

								{bootError ? (
									<div className="rounded-2xl border border-destructive/25 bg-destructive/5 px-4 py-3 text-sm leading-6 text-destructive">
										{bootError}
									</div>
								) : null}
							</CardContent>
						</Card>
					</aside>
				</div>
			</div>
		</AppShell>
	);
}
