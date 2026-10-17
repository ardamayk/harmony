import { ContextMenu as ContextMenuPrimitive } from "@base-ui/react/context-menu";

import {
	DropdownMenuItem,
	DropdownMenuSeparator,
	DropdownMenuSub,
	DropdownMenuSubContent,
	DropdownMenuSubTrigger,
	menuContentClass,
} from "#/components/ui/dropdown-menu.tsx";
import { cn } from "#/lib/utils.ts";

function ContextMenu({ ...props }: ContextMenuPrimitive.Root.Props) {
	return <ContextMenuPrimitive.Root data-slot="context-menu" {...props} />;
}

function ContextMenuTrigger({ ...props }: ContextMenuPrimitive.Trigger.Props) {
	return (
		<ContextMenuPrimitive.Trigger data-slot="context-menu-trigger" {...props} />
	);
}

function ContextMenuContent({
	className,
	align = "start",
	alignOffset = 0,
	side = "right",
	sideOffset = 0,
	...props
}: ContextMenuPrimitive.Popup.Props &
	Pick<
		ContextMenuPrimitive.Positioner.Props,
		"align" | "alignOffset" | "side" | "sideOffset"
	>) {
	return (
		<ContextMenuPrimitive.Portal>
			<ContextMenuPrimitive.Positioner
				className="isolate z-50 outline-none"
				align={align}
				alignOffset={alignOffset}
				side={side}
				sideOffset={sideOffset}
			>
				<ContextMenuPrimitive.Popup
					data-slot="context-menu-content"
					className={cn(menuContentClass, className)}
					{...props}
				/>
			</ContextMenuPrimitive.Positioner>
		</ContextMenuPrimitive.Portal>
	);
}

// Base UI's ContextMenu reuses the Menu parts below its root and trigger.
const ContextMenuItem = DropdownMenuItem;
const ContextMenuSeparator = DropdownMenuSeparator;
const ContextMenuSub = DropdownMenuSub;
const ContextMenuSubTrigger = DropdownMenuSubTrigger;
const ContextMenuSubContent = DropdownMenuSubContent;

export {
	ContextMenu,
	ContextMenuContent,
	ContextMenuItem,
	ContextMenuSeparator,
	ContextMenuSub,
	ContextMenuSubContent,
	ContextMenuSubTrigger,
	ContextMenuTrigger,
};
