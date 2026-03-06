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
	SelectItem,
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
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "./table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./tabs";
import { Tooltip, TooltipContent, TooltipTrigger } from "./tooltip";

const meta = {
	title: "UI/Primitives",
	parameters: {
		layout: "centered",
	},
} satisfies Meta;

export default meta;
type Story = StoryObj<typeof meta>;

export const FormControls: Story = {
	render: () => (
		<div className="w-[320px] space-y-4 rounded-xl border bg-card p-6 text-card-foreground shadow-sm">
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
	),
};

export const BadgeStates: Story = {
	render: () => (
		<div className="flex flex-wrap items-center gap-2">
			<Badge>Default</Badge>
			<Badge variant="secondary">Secondary</Badge>
			<Badge variant="outline">Outline</Badge>
			<Badge variant="destructive">Failed</Badge>
		</div>
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
		<div className="w-[520px] rounded-xl border bg-card p-4 shadow-sm">
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
		</div>
	),
};

export const TooltipOpen: Story = {
	render: () => (
		<Tooltip open>
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
		<Dialog open>
			<DialogContent
				className="sm:max-w-lg"
				onInteractOutside={(e) => e.preventDefault()}
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
					<p className="text-xs text-muted-foreground">
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

export const SheetOpen: Story = {
	render: () => (
		<Sheet open>
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
				</div>
			</SheetContent>
		</Sheet>
	),
};

export const AlertDialogOpen: Story = {
	render: () => (
		<AlertDialog open>
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
