import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

type LandingProps = {
	bootError?: string | null;
};

export function Landing({ bootError }: LandingProps) {
	return (
		<div className="min-h-screen">
			<div className="mx-auto max-w-3xl px-6 py-12">
				<div className="mb-8">
					<div className="inline-flex items-center gap-2 rounded-full border bg-card px-3 py-1 text-xs font-medium">
						<span className="font-mono text-muted-foreground">OctoRill</span>
						<span className="text-muted-foreground">
							GitHub 信息流 · 中文翻译
						</span>
					</div>

					<h1 className="mt-6 text-balance text-4xl font-semibold tracking-tight">
						把 GitHub 的更新变成可读的信息流
					</h1>
					<p className="text-muted-foreground mt-3 text-pretty leading-relaxed">
						Releases + Inbox 混合时间线，AI 自动翻译成中文；侧栏提供“昨日
						Release 日报”和 Inbox 快捷入口。需要操作时，直接跳回 GitHub。
					</p>
				</div>

				<Card className="shadow-sm">
					<CardHeader>
						<CardTitle>登录</CardTitle>
						<CardDescription>
							通过 GitHub OAuth 登录后，才能同步你的 starred 与 inbox。
						</CardDescription>
					</CardHeader>
					<CardContent className="flex flex-col gap-3">
						<Button asChild className="w-full">
							<a href="/auth/github/login">使用 GitHub 登录</a>
						</Button>

						{bootError ? (
							<p className="text-destructive text-sm">{bootError}</p>
						) : null}

						<p className="text-muted-foreground text-xs">
							Tip: 在 dev 环境，Vite 会把 <code>/api</code> 和{" "}
							<code>/auth</code> proxy 到 Rust 后端。
						</p>
					</CardContent>
				</Card>
			</div>
		</div>
	);
}
