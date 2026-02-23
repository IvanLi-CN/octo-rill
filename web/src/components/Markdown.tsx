import { cn } from "@/lib/utils";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

function parseInternalReleaseLink(href: string | undefined): string | null {
	if (!href) return null;
	try {
		const url = new URL(href, window.location.origin);
		const release = url.searchParams.get("release");
		const tab = url.searchParams.get("tab");
		if (!release || !/^\d+$/.test(release)) return null;
		if (tab && tab !== "briefs") return null;
		return release;
	} catch {
		return null;
	}
}

function buildMarkdownComponents(
	onInternalReleaseClick?: (releaseId: string) => void,
): Components {
	return {
	h1: ({ children }) => (
		<h2 className="text-base font-semibold tracking-tight">{children}</h2>
	),
	h2: ({ children }) => (
		<h3 className="text-sm font-semibold tracking-tight">{children}</h3>
	),
	h3: ({ children }) => (
		<h4 className="text-xs font-semibold tracking-tight text-muted-foreground">
			{children}
		</h4>
	),
	p: ({ children }) => (
		<p className="text-muted-foreground whitespace-pre-wrap">{children}</p>
	),
	ul: ({ children }) => (
		<ul className="list-disc space-y-1 pl-5">{children}</ul>
	),
	ol: ({ children }) => (
		<ol className="list-decimal space-y-1 pl-5">{children}</ol>
	),
	li: ({ children }) => <li className="text-muted-foreground">{children}</li>,
		a: ({ children, href }) => {
			const releaseId = parseInternalReleaseLink(href);
			return (
				<a
					href={href}
					target={releaseId ? undefined : "_blank"}
					rel={releaseId ? undefined : "noreferrer noopener"}
					className="text-foreground underline underline-offset-4"
					onClick={(e) => {
						if (!releaseId || !onInternalReleaseClick) return;
						e.preventDefault();
						onInternalReleaseClick(releaseId);
					}}
				>
					{children}
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
		<blockquote className="border-l-2 pl-3 text-muted-foreground italic">
			{children}
		</blockquote>
	),
	table: ({ children }) => (
		<div className="overflow-x-auto">
			<table className="w-full border-collapse text-left text-sm">
				{children}
			</table>
		</div>
	),
	th: ({ children }) => (
		<th className="border-b px-2 py-1 font-semibold">{children}</th>
	),
	td: ({ children }) => <td className="border-b px-2 py-1">{children}</td>,
	};
}

export function Markdown(props: {
	content: string;
	className?: string;
	onInternalReleaseClick?: (releaseId: string) => void;
}) {
	const { content, className, onInternalReleaseClick } = props;

	return (
		<div className={cn("space-y-3 text-sm leading-relaxed", className)}>
			<ReactMarkdown
				remarkPlugins={[remarkGfm]}
				skipHtml
				components={buildMarkdownComponents(onInternalReleaseClick)}
			>
				{content}
			</ReactMarkdown>
		</div>
	);
}
