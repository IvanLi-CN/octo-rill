import type { AnchorHTMLAttributes } from "react";

import {
	CalendarDays,
	Check,
	ChevronDown,
	ChevronUp,
	Github,
	Grid2x2,
	KeyRound,
	Menu,
	Search,
	UserRound,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { useOptionalTheme } from "@/theme/ThemeProvider";

type ReplicaTheme = {
	page: string;
	surface: string;
	subtleSurface: string;
	border: string;
	text: string;
	muted: string;
	link: string;
	accent: string;
	accentSoft: string;
	input: string;
	search: string;
	button: string;
	buttonText: string;
	topMask: string;
	topMaskAlt: string;
	avatar: string;
	shadow: string;
};

type ScopeItem = {
	label: string;
	description: string;
	checked?: boolean;
	disabled?: boolean;
	child?: boolean;
};

type GitHubPatGuideCardProps = {
	compact?: boolean;
};

type ReplicaKind = "desktop" | "mobile";

const desktopRows: ScopeItem[] = [
	{
		label: "repo",
		description: "Full control of private repositories",
		checked: true,
	},
	{
		label: "repo:status",
		description: "Access commit status",
		checked: true,
		disabled: true,
		child: true,
	},
	{
		label: "repo_deployment",
		description: "Access deployment status",
		checked: true,
		disabled: true,
		child: true,
	},
	{
		label: "public_repo",
		description: "Access public repositories",
		checked: true,
		disabled: true,
		child: true,
	},
	{
		label: "repo:invite",
		description: "Access repository invitations",
		checked: true,
		disabled: true,
		child: true,
	},
	{
		label: "security_events",
		description: "Read and write security events",
		checked: true,
		disabled: true,
		child: true,
	},
	{ label: "workflow", description: "Update GitHub Action workflows" },
	{
		label: "write:packages",
		description: "Upload packages to GitHub Package Registry",
	},
	{
		label: "read:packages",
		description: "Download packages from GitHub Package Registry",
	},
	{
		label: "delete:packages",
		description: "Delete packages from GitHub Package Registry",
	},
	{
		label: "admin:org",
		description: "Full control of orgs and teams, read and write org projects",
	},
	{
		label: "write:org",
		description:
			"Read and write org and team membership, read and write org projects",
		child: true,
	},
	{
		label: "read:org",
		description: "Read org and team membership, read org projects",
		child: true,
	},
	{
		label: "manage_runners:org",
		description: "Manage org runners and runner groups",
		child: true,
	},
	{
		label: "admin:public_key",
		description: "Full control of user public keys",
	},
];

const mobileRows = desktopRows.slice(0, 7);

const lightTheme: ReplicaTheme = {
	page: "#ffffff",
	surface: "#ffffff",
	subtleSurface: "#f6f8fa",
	border: "#d0d7de",
	text: "#1f2328",
	muted: "#59636e",
	link: "#0969da",
	accent: "#0969da",
	accentSoft: "#ddf4ff",
	input: "#ffffff",
	search: "#f6f8fa",
	button: "#1f883d",
	buttonText: "#ffffff",
	topMask: "#e7edf7",
	topMaskAlt: "#dde6f2",
	avatar: "#d6af72",
	shadow: "0 1px 0 rgba(31,35,40,0.04)",
};

const darkTheme: ReplicaTheme = {
	page: "#0d1117",
	surface: "#0d1117",
	subtleSurface: "#161b22",
	border: "#30363d",
	text: "#e6edf3",
	muted: "#8b949e",
	link: "#2f81f7",
	accent: "#2f81f7",
	accentSoft: "#1f6feb26",
	input: "#0d1117",
	search: "#0d1117",
	button: "#238636",
	buttonText: "#ffffff",
	topMask: "#2b3240",
	topMaskAlt: "#232937",
	avatar: "#c59a62",
	shadow: "0 0 0 1px rgba(240,246,252,0.02)",
};

function topChrome(ui: ReplicaTheme) {
	return (
		<div className="flex items-center gap-3">
			<div
				className="hidden lg:flex h-10 items-center gap-2 rounded-md border px-3 text-[15px]"
				style={{
					backgroundColor: ui.search,
					borderColor: ui.border,
					color: ui.muted,
				}}
			>
				<Search className="size-4" />
				<span>Type / to search</span>
			</div>
			<div className="flex items-center gap-2">
				<div
					className="h-7 w-7 rounded-md"
					style={{ backgroundColor: ui.topMask }}
				/>
				<div
					className="h-7 w-7 rounded-md"
					style={{ backgroundColor: ui.topMaskAlt }}
				/>
				<div
					className="h-7 w-7 rounded-full"
					style={{ backgroundColor: ui.avatar }}
				/>
			</div>
		</div>
	);
}

function ReferenceLink(props: AnchorHTMLAttributes<HTMLAnchorElement>) {
	return <a {...props} rel="noreferrer noopener" target="_blank" />;
}

function ReplicaCheckbox({
	checked,
	disabled,
	ui,
}: {
	checked?: boolean;
	disabled?: boolean;
	ui: ReplicaTheme;
}) {
	return (
		<span
			aria-hidden="true"
			className="mt-[2px] inline-flex size-4 items-center justify-center rounded-[4px] border"
			style={{
				backgroundColor: checked ? ui.accent : ui.input,
				borderColor: checked ? ui.accent : ui.border,
				opacity: disabled ? 0.72 : 1,
			}}
		>
			{checked ? <Check className="size-3 text-white" strokeWidth={3} /> : null}
		</span>
	);
}

function ScopeRow({ item, ui }: { item: ScopeItem; ui: ReplicaTheme }) {
	return (
		<div
			className="grid grid-cols-[minmax(0,44%)_minmax(0,56%)] gap-3 border-t px-4 py-3 text-[15px] leading-6 md:grid-cols-[minmax(0,320px)_minmax(0,1fr)] md:px-5"
			style={{ borderColor: ui.border }}
		>
			<div
				className={cn("flex items-start gap-3", item.child ? "pl-7" : "pl-0")}
			>
				<ReplicaCheckbox
					checked={item.checked}
					disabled={item.disabled}
					ui={ui}
				/>
				<span
					className={cn(
						"min-w-0 break-all",
						item.child ? "font-normal" : "font-semibold",
					)}
				>
					{item.label}
				</span>
			</div>
			<div className="min-w-0" style={{ color: ui.muted }}>
				{item.description}
			</div>
		</div>
	);
}

function PatForm({
	mobile,
	ui,
	compact,
	replicaKind,
}: {
	mobile?: boolean;
	ui: ReplicaTheme;
	compact?: boolean;
	replicaKind: ReplicaKind;
}) {
	const rows = mobile ? mobileRows : desktopRows;
	const showCompactIntro = compact && mobile;
	const noteInputId = `github-pat-note-${replicaKind}`;
	return (
		<div
			className={cn(
				"min-w-0",
				mobile ? "px-4 pb-6" : "px-8 pb-10 pt-8 lg:px-10",
			)}
		>
			<div className={cn("min-w-0", mobile ? "max-w-none" : "max-w-[820px]")}>
				<h2
					className={cn(
						"font-semibold tracking-[-0.02em]",
						mobile ? "text-[32px]" : "text-[48px]",
					)}
				>
					New personal access token (classic)
				</h2>
				<div className="mt-4 border-b" style={{ borderColor: ui.border }} />
				{showCompactIntro ? (
					<div
						aria-hidden="true"
						className="mt-6 h-4 w-full rounded-md"
						style={{ backgroundColor: ui.subtleSurface }}
					/>
				) : (
					<p
						className="mt-6 text-[16px] leading-7 md:text-[17px]"
						style={{ color: ui.text }}
					>
						Personal access tokens (classic) function like ordinary OAuth access
						tokens. They can be used instead of a password for Git over HTTPS,
						or can be used to{" "}
						<ReferenceLink
							href="https://docs.github.com/v3/auth/#basic-authentication"
							style={{ color: ui.link }}
						>
							authenticate to the API over Basic Authentication
						</ReferenceLink>
						.
					</p>
				)}

				<div className="mt-8">
					<label
						htmlFor={noteInputId}
						className="block text-[16px] font-semibold"
					>
						Note
					</label>
					<input
						readOnly
						id={noteInputId}
						aria-label="Note"
						value="OctoRill release feedback"
						className="mt-3 block h-11 w-full rounded-md border bg-transparent px-3 text-[16px] outline-none"
						style={{
							backgroundColor: ui.input,
							borderColor: ui.accent,
							boxShadow: `inset 0 0 0 1px ${ui.accent}`,
						}}
					/>
					<p className="mt-2 text-[14px]" style={{ color: ui.muted }}>
						What’s this token for?
					</p>
				</div>

				<div className="mt-8">
					<div className="text-[16px] font-semibold">Expiration</div>
					<button
						type="button"
						aria-haspopup="menu"
						className="mt-3 inline-flex h-11 items-center gap-3 rounded-md border px-3 text-[16px]"
						style={{
							backgroundColor: ui.subtleSurface,
							borderColor: ui.border,
						}}
					>
						<CalendarDays className="size-4" />
						<span>No expiration</span>
						<ChevronDown className="size-4" />
					</button>
					<p className="mt-3 text-[14px] leading-6" style={{ color: ui.muted }}>
						GitHub strongly recommends that you set an expiration date for your
						token to help keep your information secure.{" "}
						<ReferenceLink
							href="https://github.blog/changelog/2021-07-26-expiration-options-for-personal-access-tokens/"
							style={{ color: ui.link }}
						>
							Learn more
						</ReferenceLink>
					</p>
				</div>

				<div className="mt-8">
					<div className="text-[16px] font-semibold">Select scopes</div>
					<p className="mt-3 text-[16px] leading-7">
						Scopes define the access for personal tokens.{" "}
						<ReferenceLink
							href="https://docs.github.com/apps/building-oauth-apps/scopes-for-oauth-apps/"
							style={{ color: ui.link }}
						>
							Read more about OAuth scopes.
						</ReferenceLink>
					</p>
					<div
						className="mt-4 overflow-hidden rounded-md border"
						style={{ borderColor: ui.border }}
					>
						{rows.map((item) => (
							<div key={item.label}>
								<ScopeRow item={item} ui={ui} />
							</div>
						))}
					</div>
				</div>

				<div className="mt-8 flex items-center gap-4">
					<button
						type="button"
						className="rounded-md px-4 py-2 text-[15px] font-semibold"
						style={{ backgroundColor: ui.button, color: ui.buttonText }}
					>
						Generate token
					</button>
					<ReferenceLink
						href="https://github.com/settings/tokens"
						style={{ color: ui.link }}
					>
						Cancel
					</ReferenceLink>
				</div>
			</div>
		</div>
	);
}

function DesktopSidebar({ ui }: { ui: ReplicaTheme }) {
	const navItemClass =
		"flex items-center gap-3 rounded-md px-3 py-2 text-[15px] font-medium";
	return (
		<aside className="border-r px-4 py-6" style={{ borderColor: ui.border }}>
			<nav className="space-y-2">
				<ReferenceLink
					className={navItemClass}
					href="https://github.com/settings/apps"
				>
					<Grid2x2 className="size-4" />
					<span>GitHub Apps</span>
				</ReferenceLink>
				<ReferenceLink
					className={navItemClass}
					href="https://github.com/settings/developers"
				>
					<UserRound className="size-4" />
					<span>OAuth Apps</span>
				</ReferenceLink>
				<div className="space-y-1">
					<div className={cn(navItemClass, "justify-between")}>
						<div className="flex items-center gap-3">
							<KeyRound className="size-4" />
							<span>Personal access tokens</span>
						</div>
						<ChevronUp className="size-4" />
					</div>
					<div
						className="ml-7 space-y-1 border-l pl-4"
						style={{ borderColor: ui.border }}
					>
						<ReferenceLink
							className={cn(
								navItemClass,
								"px-2 py-1.5 text-[14px] font-normal",
							)}
							href="https://github.com/settings/personal-access-tokens"
						>
							Fine-grained tokens
						</ReferenceLink>
						<div
							className="relative rounded-md px-2 py-2 text-[14px] font-medium"
							style={{ backgroundColor: ui.subtleSurface }}
						>
							<span
								className="absolute bottom-2 left-0 top-2 w-1 rounded-full"
								style={{ backgroundColor: ui.accent }}
							/>
							<span className="pl-3">Tokens (classic)</span>
						</div>
					</div>
				</div>
			</nav>
		</aside>
	);
}

function DesktopReplica({
	ui,
	compact,
}: {
	ui: ReplicaTheme;
	compact?: boolean;
}) {
	return (
		<div
			className="overflow-hidden rounded-[6px] border"
			style={{
				backgroundColor: ui.page,
				borderColor: ui.border,
				color: ui.text,
				boxShadow: ui.shadow,
				fontFamily:
					'ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
			}}
		>
			<header
				className="flex h-[72px] items-center justify-between border-b px-7"
				style={{ borderColor: ui.border, backgroundColor: ui.surface }}
			>
				<div className="flex items-center gap-4 text-[15px] font-semibold">
					<button
						type="button"
						className="inline-flex size-10 items-center justify-center rounded-md border"
						style={{
							borderColor: ui.border,
							backgroundColor: ui.subtleSurface,
						}}
					>
						<Menu className="size-5" />
					</button>
					<Github className="size-8" />
					<div className="flex items-center gap-3 text-[16px]">
						<span>Settings</span>
						<span style={{ color: ui.muted }}>/</span>
						<span>Developer Settings</span>
					</div>
				</div>
				{topChrome(ui)}
			</header>
			<div className="grid min-h-[860px] grid-cols-[280px_minmax(0,1fr)]">
				<DesktopSidebar ui={ui} />
				<PatForm ui={ui} compact={compact} replicaKind="desktop" />
			</div>
		</div>
	);
}

function MobileReplica({
	ui,
	compact,
}: {
	ui: ReplicaTheme;
	compact?: boolean;
}) {
	return (
		<div
			className="overflow-hidden rounded-[6px] border"
			style={{
				backgroundColor: ui.page,
				borderColor: ui.border,
				color: ui.text,
				boxShadow: ui.shadow,
				fontFamily:
					'ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
			}}
		>
			<header
				className="flex h-14 items-center justify-between border-b px-4"
				style={{ borderColor: ui.border, backgroundColor: ui.surface }}
			>
				<div className="flex items-center gap-3">
					<button
						type="button"
						className="inline-flex size-8 items-center justify-center rounded-md border"
						style={{
							borderColor: ui.border,
							backgroundColor: ui.subtleSurface,
						}}
					>
						<Menu className="size-4" />
					</button>
					<Github className="size-7" />
					<div className="flex items-center gap-2 text-[14px] font-semibold">
						<span>…</span>
						<span style={{ color: ui.muted }}>/</span>
						<span>Developer S...</span>
					</div>
				</div>
				<div className="flex items-center gap-2">
					<button
						type="button"
						className="inline-flex size-8 items-center justify-center rounded-md border"
						style={{
							borderColor: ui.border,
							backgroundColor: ui.subtleSurface,
						}}
					>
						<Search className="size-4" />
					</button>
					<div
						className="h-7 w-7 rounded-md"
						style={{ backgroundColor: ui.topMask }}
					/>
					<div
						className="h-7 w-7 rounded-full"
						style={{ backgroundColor: ui.avatar }}
					/>
				</div>
			</header>
			<div className="border-b px-4 py-5" style={{ borderColor: ui.border }}>
				<nav className="space-y-4 text-[14px]">
					<ReferenceLink
						className="flex items-center gap-2"
						href="https://github.com/settings/apps"
					>
						<Grid2x2 className="size-4" />
						<span>GitHub Apps</span>
					</ReferenceLink>
					<ReferenceLink
						className="flex items-center gap-2"
						href="https://github.com/settings/developers"
					>
						<UserRound className="size-4" />
						<span>OAuth Apps</span>
					</ReferenceLink>
					<div>
						<div className="flex items-center justify-between font-medium">
							<div className="flex items-center gap-2">
								<KeyRound className="size-4" />
								<span>Personal access tokens</span>
							</div>
							<ChevronUp className="size-4" />
						</div>
						<div
							className="ml-2 mt-3 space-y-2 border-l pl-4"
							style={{ borderColor: ui.border }}
						>
							<div className="text-[13px]" style={{ color: ui.muted }}>
								Fine-grained tokens
							</div>
							<div
								className="relative rounded-md px-3 py-2 text-[13px] font-medium"
								style={{ backgroundColor: ui.subtleSurface }}
							>
								<span
									className="absolute bottom-2 left-0 top-2 w-1 rounded-full"
									style={{ backgroundColor: ui.accent }}
								/>
								<span>Tokens (classic)</span>
							</div>
						</div>
					</div>
				</nav>
			</div>
			<PatForm mobile ui={ui} compact={compact} replicaKind="mobile" />
		</div>
	);
}

export function GitHubPatGuideCard(props: GitHubPatGuideCardProps) {
	const { compact = false } = props;
	const theme = useOptionalTheme();
	const resolvedTheme = theme?.resolvedTheme ?? "light";
	const ui = resolvedTheme === "dark" ? darkTheme : lightTheme;

	return (
		<section
			data-testid="github-pat-guide-card"
			aria-label="GitHub classic PAT reference"
			className="overflow-hidden"
		>
			<p className="sr-only">
				GitHub Settings. Developer Settings. Personal access tokens. Tokens
				(classic). New personal access token (classic). Note equals OctoRill
				release feedback. Expiration equals No expiration. Scope repo is
				checked.
			</p>
			<div className="hidden md:block">
				<DesktopReplica ui={ui} compact={compact} />
			</div>
			<div className="md:hidden overflow-x-auto pb-1">
				<div className="min-w-[390px]">
					<MobileReplica ui={ui} compact={compact} />
				</div>
			</div>
		</section>
	);
}
