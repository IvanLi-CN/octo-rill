#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
UI/UX Pro Max Search - BM25 search engine for UI/UX style guides
Usage: python search.py "<query>" [--domain <domain>] [--stack <stack>] [--max-results 3]
       python search.py "<query>" --design-system [-p "Project Name"]
       python search.py "<query>" --design-system --persist [-p "Project Name"] [--page "dashboard"]

Domains: style, color, chart, landing, product, ux, typography, icons, react, web
Stacks: html-tailwind, react, nextjs

Persistence (Master + Overrides pattern):
  --persist    Save design system to design-system/<project-slug>/MASTER.md
  --page       Also create a page-specific override file in design-system/<project-slug>/pages/
"""

import argparse
import sys
import io
from pathlib import Path
from core import CSV_CONFIG, AVAILABLE_STACKS, MAX_RESULTS, search, search_stack
from design_system import DesignSystemGenerator, format_ascii_box, format_markdown, persist_design_system

# Force UTF-8 for stdout/stderr to handle emojis on Windows (cp1252 default)
if sys.stdout.encoding and sys.stdout.encoding.lower() != 'utf-8':
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')
if sys.stderr.encoding and sys.stderr.encoding.lower() != 'utf-8':
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding='utf-8')


def format_output(result):
    """Format results for Claude consumption (token-optimized)"""
    if "error" in result:
        return f"Error: {result['error']}"

    output = []
    if result.get("stack"):
        output.append(f"## UI Pro Max Stack Guidelines")
        output.append(f"**Stack:** {result['stack']} | **Query:** {result['query']}")
    else:
        output.append(f"## UI Pro Max Search Results")
        output.append(f"**Domain:** {result['domain']} | **Query:** {result['query']}")
    output.append(f"**Source:** {result['file']} | **Found:** {result['count']} results\n")

    for i, row in enumerate(result['results'], 1):
        output.append(f"### Result {i}")
        for key, value in row.items():
            value_str = str(value)
            if len(value_str) > 300:
                value_str = value_str[:300] + "..."
            output.append(f"- **{key}:** {value_str}")
        output.append("")

    return "\n".join(output)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="UI Pro Max Search")
    parser.add_argument("query", help="Search query")
    parser.add_argument("--domain", "-d", choices=list(CSV_CONFIG.keys()), help="Search domain")
    parser.add_argument("--stack", "-s", choices=AVAILABLE_STACKS, help="Stack-specific search (html-tailwind, react, nextjs)")
    parser.add_argument("--max-results", "-n", type=int, default=MAX_RESULTS, help="Max results (default: 3)")
    parser.add_argument("--json", action="store_true", help="Output as JSON")
    # Design system generation
    parser.add_argument("--design-system", "-ds", action="store_true", help="Generate complete design system recommendation")
    parser.add_argument("--project-name", "-p", type=str, default=None, help="Project name for design system output")
    parser.add_argument("--format", "-f", choices=["ascii", "markdown"], default="ascii", help="Output format for design system")
    # Persistence (Master + Overrides pattern)
    parser.add_argument("--persist", action="store_true", help="Save design system to design-system/<project-slug>/MASTER.md (creates hierarchical structure)")
    parser.add_argument("--page", type=str, default=None, help="Create page-specific override file in design-system/<project-slug>/pages/")
    parser.add_argument("--output-dir", "-o", type=str, default=None, help="Output directory for persisted files (default: current directory)")

    args = parser.parse_args()

    # Design system takes priority
    if args.design_system:
        generator = DesignSystemGenerator()
        design_system = generator.generate(args.query, args.project_name)
        persist_result = None
        if args.persist:
            persist_result = persist_design_system(
                design_system,
                page=args.page,
                output_dir=args.output_dir,
                page_query=args.query
            )

        if args.format == "markdown":
            print(format_markdown(design_system))
        else:
            print(format_ascii_box(design_system))
        
        # Print persistence confirmation
        if persist_result:
            design_system_dir = persist_result.get("design_system_dir", "")
            created_files = persist_result.get("created_files", [])
            master_file = next((path for path in created_files if Path(path).name == "MASTER.md"), None)
            page_file = next((path for path in created_files if Path(path).suffix == ".md" and Path(path).name != "MASTER.md"), None)

            print("\n" + "=" * 60)
            print(f"✅ Design system persisted to {design_system_dir}/")
            if master_file:
                print(f"   📄 {master_file} (Global Source of Truth)")
            if args.page and page_file:
                print(f"   📄 {page_file} (Page Overrides)")
            print("")
            print(f"📖 Usage: When building a page, check {design_system_dir}/pages/[page].md first.")
            print(f"   If exists, its rules override MASTER.md. Otherwise, use MASTER.md.")
            print("=" * 60)
    # Stack search
    elif args.stack:
        result = search_stack(args.query, args.stack, args.max_results)
        if args.json:
            import json
            print(json.dumps(result, indent=2, ensure_ascii=False))
        else:
            print(format_output(result))
    # Domain search
    else:
        result = search(args.query, args.domain, args.max_results)
        if args.json:
            import json
            print(json.dumps(result, indent=2, ensure_ascii=False))
        else:
            print(format_output(result))
