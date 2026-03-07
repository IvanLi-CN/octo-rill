import type { ReactNode } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";

import {
	AlertDialog,
	AlertDialogAction,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
} from "./alert-dialog";
import { Badge } from "./badge";
import { Button } from "./button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "./dialog";
import { Input } from "./input";
import { Label } from "./label";
import {
	Select,
	SelectContent,
	SelectGroup,
	SelectItem,
	SelectLabel,
	SelectSeparator,
	SelectTrigger,
	SelectValue,
} from "./select";
import {
	Sheet,
	SheetContent,
	SheetDescription,
	SheetHeader,
	SheetTitle,
} from "./sheet";
import {
	Table,
	TableBody,
	TableCaption,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "./table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./tabs";
import { Tooltip, TooltipContent, TooltipTrigger } from "./tooltip";

type Story = StoryObj<typeof meta>;

type StoryPanelProps = {
	title: string;
	description: string;
	children: ReactNode;
	className?: string;
};

function StoryPanel({
	title,
	description,
	children,
	className,
}: StoryPanelProps) {
	return (
		<div
			className={
				className ?? "w-[360px] rounded-xl border bg-card p-6 shadow-sm"
			}
		>
			<div className="mb-4 space-y-1">
				<p className="font-medium text-sm">{title}</p>
				<p className="text-muted-foreground text-xs">{description}</p>
			</div>
			{children}
		</div>
	);
}

const meta = {
	title: "UI/Primitives",
	parameters: {
		layout: "centered",
	},
} satisfies Meta;

export default meta;

function isDocsMode() {
	if (typeof window === "undefined") return false;
	return new URLSearchParams(window.location.search).get("viewMode") === "docs";
}

export const FormControls: Story = {
	render: () => (
		<StoryPanel
			title="基础表单控件"
			description="展示 Input + Select + Label 的最小组合，便于观察对齐与间距。"
		>
			<div className="space-y-4">
				<div className="space-y-2">
					<Label htmlFor="storybook-query">Search</Label>
					<Input id="storybook-query" placeholder="repo / login / email" />
				</div>
				<div className="space-y-2">
					<Label htmlFor="storybook-role">Role</Label>
					<Select defaultValue="admin">
						<SelectTrigger id="storybook-role" className="w-full">
							<SelectValue placeholder="Choose a role" />
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="all">All</SelectItem>
							<SelectItem value="admin">Admin</SelectItem>
							<SelectItem value="user">User</SelectItem>
						</SelectContent>
					</Select>
				</div>
			</div>
		</StoryPanel>
	),
};

export const FilterToolbarState: Story = {
	render: () => (
		<StoryPanel
			title="筛选条组合态"
			description="对应 Admin Users / Admin Jobs 的实际使用方式，确认多控件并排时的布局和密度。"
			className="w-[760px] rounded-xl border bg-card p-6 shadow-sm"
		>
			<div className="grid gap-3 md:grid-cols-[minmax(0,1.6fr)_180px_180px]">
				<div className="space-y-2">
					<Label htmlFor="storybook-filter-query">搜索</Label>
					<Input
						id="storybook-filter-query"
						defaultValue="octo"
						placeholder="login / name / email"
					/>
				</div>
				<div className="space-y-2">
					<Label htmlFor="storybook-filter-role">角色</Label>
					<Select defaultValue="user">
						<SelectTrigger id="storybook-filter-role" className="w-full">
							<SelectValue placeholder="选择角色" />
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="all">全部角色</SelectItem>
							<SelectItem value="admin">管理员</SelectItem>
							<SelectItem value="user">普通用户</SelectItem>
						</SelectContent>
					</Select>
				</div>
				<div className="space-y-2">
					<Label htmlFor="storybook-filter-status">状态</Label>
					<Select defaultValue="enabled">
						<SelectTrigger id="storybook-filter-status" className="w-full">
							<SelectValue placeholder="选择状态" />
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="all">全部状态</SelectItem>
							<SelectItem value="enabled">启用中</SelectItem>
							<SelectItem value="disabled">已禁用</SelectItem>
						</SelectContent>
					</Select>
				</div>
			</div>
		</StoryPanel>
	),
};

export const SelectOpenState: Story = {
	render: () => (
		<StoryPanel
			title="下拉展开态"
			description="补齐 portal 型 Select 的可见态，便于检查列表宽度、hover 与选中标记。"
		>
			<div className="space-y-2">
				<Label htmlFor="storybook-select-open">任务状态</Label>
				<Select
					open={!isDocsMode()}
					defaultValue="running"
					onOpenChange={() => undefined}
				>
					<SelectTrigger id="storybook-select-open" className="w-full">
						<SelectValue placeholder="选择状态" />
					</SelectTrigger>
					<SelectContent>
						<SelectGroup>
							<SelectLabel>任务状态</SelectLabel>
							<SelectItem value="all">全部</SelectItem>
							<SelectItem value="queued">排队中</SelectItem>
							<SelectItem value="running">运行中</SelectItem>
							<SelectItem value="succeeded">已完成</SelectItem>
						</SelectGroup>
						<SelectSeparator />
						<SelectGroup>
							<SelectLabel>异常状态</SelectLabel>
							<SelectItem value="failed">失败</SelectItem>
							<SelectItem value="canceled">已取消</SelectItem>
						</SelectGroup>
					</SelectContent>
				</Select>
			</div>
		</StoryPanel>
	),
};

export const BadgeStates: Story = {
	render: () => (
		<StoryPanel
			title="Badge 基础态"
			description="展示 variant 基础外观，作为页面 tone 覆写前的基座。"
		>
			<div className="flex flex-wrap items-center gap-2">
				<Badge>Default</Badge>
				<Badge variant="secondary">Secondary</Badge>
				<Badge variant="outline">Outline</Badge>
				<Badge variant="destructive">Failed</Badge>
			</div>
		</StoryPanel>
	),
};

export const BadgeOperationalTones: Story = {
	render: () => (
		<StoryPanel
			title="业务状态胶囊"
			description="补齐 Admin Users / Admin Jobs 实际会出现的 tone 组合，避免只看基础 variant。"
			className="w-[540px] rounded-xl border bg-card p-6 shadow-sm"
		>
			<div className="flex flex-wrap items-center gap-2">
				<Badge className="border-slate-200 bg-slate-100 text-slate-700 hover:bg-slate-100">
					queued
				</Badge>
				<Badge className="border-sky-200 bg-sky-100 text-sky-700 hover:bg-sky-100">
					running
				</Badge>
				<Badge className="border-emerald-200 bg-emerald-100 text-emerald-700 hover:bg-emerald-100">
					succeeded
				</Badge>
				<Badge variant="destructive">failed</Badge>
				<Badge className="border-amber-200 bg-amber-100 text-amber-700 hover:bg-amber-100">
					canceled
				</Badge>
				<Badge variant="outline">admin</Badge>
				<Badge className="border-zinc-300 bg-zinc-100 text-zinc-700 hover:bg-zinc-100">
					disabled
				</Badge>
			</div>
		</StoryPanel>
	),
};

export const TabsState: Story = {
	render: () => (
		<Tabs defaultValue="realtime" className="w-[420px]">
			<TabsList>
				<TabsTrigger value="realtime">实时异步任务</TabsTrigger>
				<TabsTrigger value="scheduled">定时任务</TabsTrigger>
				<TabsTrigger value="llm">LLM 调度</TabsTrigger>
			</TabsList>
			<TabsContent value="realtime" className="rounded-xl border bg-card p-4">
				<p className="text-sm">实时任务列表与重试操作。</p>
			</TabsContent>
			<TabsContent value="scheduled" className="rounded-xl border bg-card p-4">
				<p className="text-sm">查看运行记录与时间槽。</p>
			</TabsContent>
			<TabsContent value="llm" className="rounded-xl border bg-card p-4">
				<p className="text-sm">查看调度状态与调用记录。</p>
			</TabsContent>
		</Tabs>
	),
};

export const TableState: Story = {
	render: () => (
		<StoryPanel
			title="紧凑表格"
			description="对应 Admin Jobs 的常规列表密度，确认表头、单元格和 Badge 嵌套表现。"
			className="w-[560px] rounded-xl border bg-card p-4 shadow-sm"
		>
			<Table>
				<TableHeader>
					<TableRow>
						<TableHead>Task</TableHead>
						<TableHead>Status</TableHead>
						<TableHead>Updated</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					<TableRow>
						<TableCell>sync.releases</TableCell>
						<TableCell>
							<Badge variant="secondary">running</Badge>
						</TableCell>
						<TableCell>08:05</TableCell>
					</TableRow>
					<TableRow>
						<TableCell>brief.daily_slot</TableCell>
						<TableCell>
							<Badge variant="outline">queued</Badge>
						</TableCell>
						<TableCell>08:10</TableCell>
					</TableRow>
				</TableBody>
			</Table>
		</StoryPanel>
	),
};

export const TableLongContent: Story = {
	render: () => (
		<StoryPanel
			title="长内容与横向滚动"
			description="补齐 Markdown / 管理页长字段场景，直接观察 nowrap 与 overflow-x 的表现。"
			className="w-[520px] rounded-xl border bg-card p-4 shadow-sm"
		>
			<Table className="min-w-[860px]">
				<TableCaption>
					长 repo/source 字段应保持单行，并通过容器横向滚动查看完整内容。
				</TableCaption>
				<TableHeader>
					<TableRow>
						<TableHead>Repo</TableHead>
						<TableHead>Source</TableHead>
						<TableHead>Status</TableHead>
						<TableHead>Updated</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					<TableRow>
						<TableCell>octo-rill/platform-observability-release-feed</TableCell>
						<TableCell>
							job.api.translate_release_batch_with_extended_audit
						</TableCell>
						<TableCell>
							<Badge className="border-sky-200 bg-sky-100 text-sky-700 hover:bg-sky-100">
								running
							</Badge>
						</TableCell>
						<TableCell>2026-03-07 11:28:40</TableCell>
					</TableRow>
					<TableRow>
						<TableCell>
							octo-rill/briefs-daily-digest-summary-renderer
						</TableCell>
						<TableCell>job.scheduler.daily_brief_slot_pipeline</TableCell>
						<TableCell>
							<Badge variant="outline">queued</Badge>
						</TableCell>
						<TableCell>2026-03-07 11:32:05</TableCell>
					</TableRow>
				</TableBody>
			</Table>
		</StoryPanel>
	),
};

export const TooltipOpen: Story = {
	render: () => (
		<Tooltip open={!isDocsMode()}>
			<TooltipTrigger asChild>
				<Button variant="outline">实时异步任务说明</Button>
			</TooltipTrigger>
			<TooltipContent>
				<p>监控系统内部任务，并支持重试与取消。</p>
			</TooltipContent>
		</Tooltip>
	),
};

export const DialogOpen: Story = {
	render: () => (
		<Dialog open={!isDocsMode()}>
			<DialogContent
				className="sm:max-w-lg"
				onInteractOutside={(event) => event.preventDefault()}
			>
				<DialogHeader>
					<DialogTitle>配置 GitHub PAT</DialogTitle>
					<DialogDescription>
						输入后自动校验，保存按钮仅在校验通过后可用。
					</DialogDescription>
				</DialogHeader>
				<div className="space-y-3">
					<div className="space-y-2">
						<Label htmlFor="storybook-pat">GitHub PAT</Label>
						<Input
							id="storybook-pat"
							type="password"
							value="ghp_****************"
							readOnly
						/>
					</div>
					<p className="text-xs text-emerald-600">
						输入后会自动检查 PAT 是否可用。
					</p>
				</div>
				<DialogFooter>
					<Button variant="outline">稍后再说</Button>
					<Button>保存并继续</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	),
};

export const DialogInvalidState: Story = {
	render: () => (
		<Dialog open={!isDocsMode()}>
			<DialogContent
				showCloseButton={false}
				className="sm:max-w-lg"
				onInteractOutside={(event) => event.preventDefault()}
			>
				<DialogHeader>
					<DialogTitle>配置 GitHub PAT</DialogTitle>
					<DialogDescription>
						展示校验失败时的按钮禁用与错误反馈，覆盖 Dashboard 的关键异常态。
					</DialogDescription>
				</DialogHeader>
				<div className="space-y-3">
					<div className="space-y-2">
						<Label htmlFor="storybook-invalid-pat">GitHub PAT</Label>
						<Input
							id="storybook-invalid-pat"
							type="password"
							value="ghp_invalid_demo_token"
							readOnly
							aria-invalid="true"
						/>
					</div>
					<p className="text-destructive text-xs">
						PAT 无效或已过期，请重新填写并校验。
					</p>
				</div>
				<DialogFooter>
					<Button variant="outline">稍后再说</Button>
					<Button disabled>保存并继续</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	),
};

export const SheetOpen: Story = {
	render: () => (
		<Sheet open={!isDocsMode()}>
			<SheetContent className="max-w-md">
				<SheetHeader>
					<SheetTitle>用户详情</SheetTitle>
					<SheetDescription>查看账号状态与日报时间。</SheetDescription>
				</SheetHeader>
				<div className="space-y-3 px-4 pb-4 text-sm">
					<div className="rounded-lg border p-3">最后活动：08:05</div>
					<div className="rounded-lg border p-3">
						日报时间（本地时区）：16:00
					</div>
					<div className="rounded-lg border p-3">账户角色：管理员</div>
				</div>
			</SheetContent>
		</Sheet>
	),
};

export const AlertDialogOpen: Story = {
	render: () => (
		<AlertDialog open={!isDocsMode()}>
			<AlertDialogContent>
				<AlertDialogHeader>
					<AlertDialogTitle>确认管理员变更</AlertDialogTitle>
					<AlertDialogDescription>
						此操作属于高权限变更，需要二次确认后才会提交。
					</AlertDialogDescription>
				</AlertDialogHeader>
				<AlertDialogFooter>
					<AlertDialogCancel>取消</AlertDialogCancel>
					<AlertDialogAction>确认更改</AlertDialogAction>
				</AlertDialogFooter>
			</AlertDialogContent>
		</AlertDialog>
	),
};
