import type { AnchorHTMLAttributes, MouseEvent } from "react";
import { useCallback } from "react";
import { useRouter } from "@tanstack/react-router";

type NavigationTarget = {
	href: string;
	to: string;
	search?: Record<string, unknown>;
	params?: Record<string, string>;
	replace?: boolean;
};

type InternalLinkProps = AnchorHTMLAttributes<HTMLAnchorElement> &
	NavigationTarget;

function shouldInterceptClick(event: MouseEvent<HTMLAnchorElement>) {
	return !(
		event.defaultPrevented ||
		event.button !== 0 ||
		event.metaKey ||
		event.altKey ||
		event.ctrlKey ||
		event.shiftKey
	);
}

export function useOptionalRouter() {
	return useRouter({ warn: false }) as ReturnType<typeof useRouter> | null;
}

export function useInternalNavigate() {
	const router = useOptionalRouter();

	return useCallback(
		async ({ href, to, search, params, replace }: NavigationTarget) => {
			if (!router) {
				if (replace) {
					window.location.replace(href);
				} else {
					window.location.assign(href);
				}
				return;
			}

			await router.navigate({
				to,
				search: search as never,
				params: params as never,
				replace,
			});
		},
		[router],
	);
}

export function InternalLink(props: InternalLinkProps) {
	const { href, to, search, params, replace, onClick, target, ...rest } = props;
	const router = useOptionalRouter();

	return (
		<a
			{...rest}
			href={href}
			target={target}
			onClick={(event) => {
				onClick?.(event);
				if (!router || target === "_blank" || !shouldInterceptClick(event)) {
					return;
				}
				event.preventDefault();
				void router.navigate({
					to,
					search: search as never,
					params: params as never,
					replace,
				});
			}}
		/>
	);
}
