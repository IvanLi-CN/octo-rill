import { cn } from "@/lib/utils";

type Block =
	| { kind: "h"; level: 1 | 2 | 3; text: string }
	| { kind: "p"; text: string }
	| { kind: "ul"; items: string[] };

function parseMarkdown(md: string): Block[] {
	const lines = md.replaceAll("\r\n", "\n").split("\n");

	const out: Block[] = [];
	let para: string[] = [];
	let list: string[] = [];

	const flushPara = () => {
		const text = para.join(" ").trim();
		if (text) out.push({ kind: "p", text });
		para = [];
	};

	const flushList = () => {
		if (list.length > 0) out.push({ kind: "ul", items: list });
		list = [];
	};

	for (const raw of lines) {
		const line = raw.trimEnd();
		const trimmed = line.trim();

		if (!trimmed) {
			flushList();
			flushPara();
			continue;
		}

		const heading = /^(#{1,3})\s+(.*)$/.exec(trimmed);
		if (heading) {
			flushList();
			flushPara();
			const level = heading[1].length as 1 | 2 | 3;
			const text = heading[2].trim();
			out.push({ kind: "h", level, text });
			continue;
		}

		const bullet = /^[-*]\s+(.*)$/.exec(trimmed);
		if (bullet) {
			flushPara();
			list.push(bullet[1].trim());
			continue;
		}

		flushList();
		para.push(trimmed);
	}

	flushList();
	flushPara();
	return out;
}

function blockKey(b: Block, i: number) {
	if (b.kind === "ul") return `ul:${b.items.join("|")}:${i}`;
	if (b.kind === "h") return `h${b.level}:${b.text}:${i}`;
	return `p:${b.text}:${i}`;
}

export function Markdown(props: { content: string; className?: string }) {
	const { content, className } = props;
	const blocks = parseMarkdown(content);

	return (
		<div className={cn("space-y-3 text-sm leading-relaxed", className)}>
			{blocks.map((b, i) => {
				if (b.kind === "h") {
					const classes =
						b.level === 1
							? "text-base font-semibold tracking-tight"
							: b.level === 2
								? "text-sm font-semibold tracking-tight"
								: "text-xs font-semibold tracking-tight text-muted-foreground";
					const Tag = b.level === 1 ? "h2" : b.level === 2 ? "h3" : "h4";
					return (
						<Tag key={blockKey(b, i)} className={classes}>
							{b.text}
						</Tag>
					);
				}

				if (b.kind === "ul") {
					return (
						<ul key={blockKey(b, i)} className="list-disc space-y-1 pl-5">
							{b.items.map((it) => (
								<li key={it} className="text-muted-foreground">
									{it}
								</li>
							))}
						</ul>
					);
				}

				return (
					<p
						key={blockKey(b, i)}
						className="text-muted-foreground whitespace-pre-wrap"
					>
						{b.text}
					</p>
				);
			})}
		</div>
	);
}
