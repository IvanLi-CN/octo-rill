const DEMO_RUNTIME_REQUEST_MARKER = "__demo_runtime";

function buildUrl(input: string | URL | Request) {
	if (input instanceof URL) {
		return new URL(input.toString());
	}
	if (input instanceof Request) {
		return new URL(input.url);
	}
	if (typeof window === "undefined") {
		throw new Error("window is required to resolve relative demo request URLs");
	}
	return new URL(input, window.location.origin);
}

export function appendDemoRuntimeRequestMarker(input: string | URL) {
	const url = buildUrl(input);
	url.searchParams.set(DEMO_RUNTIME_REQUEST_MARKER, "1");
	return `${url.pathname}${url.search}${url.hash}`;
}

export function hasDemoRuntimeRequestMarker(input: string | URL | Request) {
	return buildUrl(input).searchParams.get(DEMO_RUNTIME_REQUEST_MARKER) === "1";
}
