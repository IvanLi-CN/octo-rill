import { replaceIsoTimestampsWithLocal } from "@/lib/datetime";
import { cn } from "@/lib/utils";
import {
	parseInternalDashboardReleaseTarget,
	type DashboardReleaseTarget,
} from "@/dashboard/routeState";
import { isValidElement, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";

type MarkdownNode = {
	type?: string;
	value?: string;
	children?: MarkdownNode[];
};

const GITHUB_HOSTS = new Set(["github.com", "www.github.com"]);

function trimTrailingSlash(raw: string) {
	return raw.endsWith("/") ? raw.slice(0, -1) : raw;
}

function normalizeLinkLiteral(raw: string) {
	return trimTrailingSlash(raw.trim());
}

function safeDecodeUri(raw: string) {
	try {
		return decodeURI(raw);
	} catch {
		return raw;
	}
}

function truncateChars(raw: string, maxChars: number) {
	const chars = Array.from(raw);
	if (chars.length <= maxChars) return raw;
	return `${chars.slice(0, maxChars).join("")}…`;
}

function collectTextContent(node: ReactNode): string {
	if (node == null || typeof node === "boolean") return "";
	if (typeof node === "string" || typeof node === "number") {
		return String(node);
	}
	if (Array.isArray(node)) {
		return node.map((child) => collectTextContent(child)).join("");
	}
	if (isValidElement<{ children?: ReactNode }>(node)) {
		return collectTextContent(node.props.children);
	}
	return "";
}

function compactGithubLinkLabel(raw: string): string | null {
	try {
		const parsed = new URL(raw, window.location.origin);
		if (!GITHUB_HOSTS.has(parsed.host)) return null;
		const segments = parsed.pathname
			.split("/")
			.map((segment) => segment.trim())
			.filter(Boolean);

		if (segments.length >= 4) {
			switch (segments[2]) {
				case "pull":
				case "issues":
					if (/^\d+$/.test(segments[3])) {
						return `#${segments[3]}`;
					}
					break;
				case "commit":
					return segments[3].slice(0, 7);
				case "releases":
					if (segments[3] === "tag" && segments[4]) {
						return truncateChars(segments[4], 32);
					}
					break;
			}
		}

		if (segments.length >= 3 && segments[2] === "releases") {
			return "releases";
		}

		const fallback = parsed.pathname.replace(/^\/+|\/+$/g, "");
		return fallback ? truncateChars(fallback, 40) : "github.com";
	} catch {
		return null;
	}
}

function isAutolinkLiteral(labelText: string, href: string | undefined) {
	if (!href) return false;
	const normalizedLabel = normalizeLinkLiteral(labelText);
	if (!normalizedLabel) return false;

	const variants = new Set<string>([
		normalizeLinkLiteral(href),
		normalizeLinkLiteral(safeDecodeUri(href)),
	]);

	try {
		const absolute = new URL(href, window.location.origin).toString();
		variants.add(normalizeLinkLiteral(absolute));
		variants.add(normalizeLinkLiteral(safeDecodeUri(absolute)));
	} catch {
		// Ignore malformed URLs and fall back to the raw literal comparison.
	}

	return variants.has(normalizedLabel);
}

function localizeTextNodes(node: MarkdownNode | null | undefined) {
	if (!node) return;
	if (node.type === "code" || node.type === "inlineCode") return;
	if (node.type === "text" && typeof node.value === "string") {
		node.value = replaceIsoTimestampsWithLocal(node.value);
	}
	node.children?.forEach(localizeTextNodes);
}

function remarkLocalizeIsoTimestamps() {
	return (tree: MarkdownNode) => {
		localizeTextNodes(tree);
	};
}

function buildMarkdownComponents(
	onInternalReleaseClick?: (target: DashboardReleaseTarget) => void,
): Components {
	return {
		h1: ({ children }) => (
			<h2 className="min-w-0 max-w-full text-base font-semibold tracking-tight [overflow-wrap:anywhere]">
				{children}
			</h2>
		),
		h2: ({ children }) => (
			<h3 className="min-w-0 max-w-full text-sm font-semibold tracking-tight [overflow-wrap:anywhere]">
				{children}
			</h3>
		),
		h3: ({ children }) => (
			<h4 className="text-muted-foreground min-w-0 max-w-full text-xs font-semibold tracking-tight [overflow-wrap:anywhere]">
				{children}
			</h4>
		),
		p: ({ children }) => (
			<p className="text-muted-foreground min-w-0 max-w-full whitespace-pre-wrap [overflow-wrap:anywhere]">
				{children}
			</p>
		),
		ul: ({ children }) => (
			<ul className="min-w-0 max-w-full list-disc space-y-1 pl-5">
				{children}
			</ul>
		),
		ol: ({ children }) => (
			<ol className="min-w-0 max-w-full list-decimal space-y-1 pl-5">
				{children}
			</ol>
		),
		li: ({ children }) => (
			<li className="text-muted-foreground min-w-0 max-w-full [overflow-wrap:anywhere]">
				{children}
			</li>
		),
		a: ({ children, href }) => {
			const releaseTarget = parseInternalDashboardReleaseTarget(href);
			const textContent = collectTextContent(children).trim();
			const compactLabel =
				!releaseTarget && href && isAutolinkLiteral(textContent, href)
					? compactGithubLinkLabel(href)
					: null;
			return (
				<a
					href={href}
					target={releaseTarget ? undefined : "_blank"}
					rel={releaseTarget ? undefined : "noreferrer noopener"}
					className="text-foreground underline underline-offset-4 [overflow-wrap:anywhere]"
					onClick={(e) => {
						if (!releaseTarget || !onInternalReleaseClick) return;
						e.preventDefault();
						onInternalReleaseClick(releaseTarget);
					}}
				>
					{compactLabel ?? children}
				</a>
			);
		},
		code: ({ children }) => (
			<code className="bg-muted/60 rounded px-1 py-0.5 font-mono text-[12px]">
				{children}
			</code>
		),
		pre: ({ children }) => (
			<pre className="bg-muted/40 overflow-x-auto rounded-md border p-3 font-mono text-xs">
				{children}
			</pre>
		),
		blockquote: ({ children }) => (
			<blockquote className="text-muted-foreground min-w-0 max-w-full border-l-2 pl-3 italic [overflow-wrap:anywhere]">
				{children}
			</blockquote>
		),
		table: ({ children }) => (
			<div className="overflow-x-auto rounded-md border">
				<Table>{children}</Table>
			</div>
		),
		thead: ({ children }) => <TableHeader>{children}</TableHeader>,
		tbody: ({ children }) => <TableBody>{children}</TableBody>,
		tr: ({ children }) => <TableRow>{children}</TableRow>,
		th: ({ children }) => <TableHead>{children}</TableHead>,
		td: ({ children }) => <TableCell>{children}</TableCell>,
	};
}

export function Markdown(props: {
	content: string;
	className?: string;
	onInternalReleaseClick?: (target: DashboardReleaseTarget) => void;
}) {
	const { content, className, onInternalReleaseClick } = props;

	return (
		<div
			data-markdown-root="true"
			className={cn(
				"min-w-0 max-w-full space-y-3 text-sm leading-relaxed",
				className,
			)}
		>
			<ReactMarkdown
				remarkPlugins={[remarkGfm, remarkLocalizeIsoTimestamps]}
				skipHtml
				components={buildMarkdownComponents(onInternalReleaseClick)}
			>
				{content}
			</ReactMarkdown>
		</div>
	);
}
