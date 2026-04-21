import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/bind/github")({
	validateSearch: (search: Record<string, unknown>) => ({
		linuxdo: typeof search.linuxdo === "string" ? search.linuxdo : undefined,
	}),
});
