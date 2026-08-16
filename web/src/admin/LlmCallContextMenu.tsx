import { List, ListX, MoreHorizontal } from "lucide-react";
import { cloneElement, type KeyboardEvent, type ReactElement } from "react";
import { ContextMenu, DropdownMenu } from "radix-ui";

import { Button } from "@/components/ui/button";

export type LlmCallDrilldown = {
	model: string;
	finishedFrom?: string;
	finishedBefore?: string;
};

type LlmCallDrilldownStatus = "all" | "failed";

type LlmCallContextMenuProps = {
	target: LlmCallDrilldown;
	onOpen: (
		target: LlmCallDrilldown & { status: LlmCallDrilldownStatus },
	) => void;
	children: ReactElement<{
		onKeyDown?: (event: KeyboardEvent<HTMLElement>) => void;
	}>;
};

const menuContentClassName =
	"bg-popover text-popover-foreground z-50 min-w-44 rounded-md border p-1 shadow-lg";
const menuItemClassName =
	"focus:bg-accent focus:text-accent-foreground flex h-8 cursor-default items-center gap-2 rounded-sm px-2 text-sm outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground";

function LlmCallMenuItems(props: {
	target: LlmCallDrilldown;
	onOpen: LlmCallContextMenuProps["onOpen"];
	Item: typeof ContextMenu.Item;
}) {
	const { target, onOpen, Item } = props;
	return (
		<>
			<Item
				className={menuItemClassName}
				onSelect={() => onOpen({ ...target, status: "failed" })}
			>
				<ListX className="size-4" />
				查看失败调用
			</Item>
			<Item
				className={menuItemClassName}
				onSelect={() => onOpen({ ...target, status: "all" })}
			>
				<List className="size-4" />
				查看全部调用
			</Item>
		</>
	);
}

export function LlmCallContextMenu({
	target,
	onOpen,
	children,
}: LlmCallContextMenuProps) {
	const openFromKeyboard = (event: KeyboardEvent<HTMLElement>) => {
		if (
			event.key !== "ContextMenu" &&
			!(event.key === "F10" && event.shiftKey)
		) {
			return;
		}
		event.preventDefault();
		const target = event.target;
		if (!(target instanceof HTMLElement)) return;
		const rect = target.getBoundingClientRect();
		target.dispatchEvent(
			new MouseEvent("contextmenu", {
				bubbles: true,
				clientX: rect.left + rect.width / 2,
				clientY: rect.top + rect.height / 2,
			}),
		);
	};

	const trigger = cloneElement(children, {
		onKeyDown: (event: KeyboardEvent<HTMLElement>) => {
			children.props.onKeyDown?.(event);
			if (!event.defaultPrevented) openFromKeyboard(event);
		},
	});

	return (
		<ContextMenu.Root>
			<ContextMenu.Trigger asChild>{trigger}</ContextMenu.Trigger>
			<ContextMenu.Portal>
				<ContextMenu.Content className={menuContentClassName}>
					<LlmCallMenuItems
						target={target}
						onOpen={onOpen}
						Item={ContextMenu.Item}
					/>
				</ContextMenu.Content>
			</ContextMenu.Portal>
		</ContextMenu.Root>
	);
}

export function LlmCallActionsMenu(props: {
	target: LlmCallDrilldown;
	onOpen: LlmCallContextMenuProps["onOpen"];
	label: string;
}) {
	const { target, onOpen, label } = props;
	return (
		<DropdownMenu.Root>
			<DropdownMenu.Trigger asChild>
				<Button
					type="button"
					variant="ghost"
					size="icon"
					className="size-8"
					aria-label={label}
					title={label}
				>
					<MoreHorizontal className="size-4" />
				</Button>
			</DropdownMenu.Trigger>
			<DropdownMenu.Portal>
				<DropdownMenu.Content
					className={menuContentClassName}
					align="end"
					sideOffset={6}
					onMouseDown={(event) => event.stopPropagation()}
				>
					<LlmCallMenuItems
						target={target}
						onOpen={onOpen}
						Item={DropdownMenu.Item}
					/>
				</DropdownMenu.Content>
			</DropdownMenu.Portal>
		</DropdownMenu.Root>
	);
}
