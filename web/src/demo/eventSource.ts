import { appendDemoRuntimeRequestMarker } from "@/demo/requestMarker";
import { getDemoEventSourceFactory, isDemoMode } from "@/demo/runtime";

export function openAppEventSource(
	url: string,
	options?: {
		withCredentials?: boolean;
	},
): EventSource {
	if (typeof window !== "undefined" && isDemoMode()) {
		const factory = getDemoEventSourceFactory();
		if (factory) {
			return factory(appendDemoRuntimeRequestMarker(url), options);
		}
	}
	return new EventSource(url, options);
}
