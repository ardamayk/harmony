import { Combobox as ComboboxPrimitive } from "@base-ui/react/combobox";
import { CheckIcon, ChevronDownIcon, SearchIcon } from "lucide-react";

import {
	menuContentClass,
	menuItemClass,
} from "#/components/ui/dropdown-menu.tsx";
import { inputClass } from "#/components/ui/input.tsx";
import { cn } from "#/lib/utils.ts";

// A select with a search field in its popup.
const Combobox = ComboboxPrimitive.Root;

function ComboboxValue({ ...props }: ComboboxPrimitive.Value.Props) {
	return <ComboboxPrimitive.Value data-slot="combobox-value" {...props} />;
}

function ComboboxTrigger({
	className,
	children,
	...props
}: ComboboxPrimitive.Trigger.Props) {
	return (
		<ComboboxPrimitive.Trigger
			data-slot="combobox-trigger"
			className={cn(
				"flex h-9 w-fit items-center justify-between gap-3 rounded-sm border border-input bg-transparent px-3 text-left text-sm whitespace-nowrap shadow-xs transition-[color,box-shadow] outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 data-disabled:cursor-not-allowed data-disabled:opacity-50 dark:bg-input/30 dark:hover:bg-input/50",
				className,
			)}
			{...props}
		>
			{children}
			<ChevronDownIcon className="size-4 shrink-0 text-caption" aria-hidden />
		</ComboboxPrimitive.Trigger>
	);
}

function ComboboxContent({
	className,
	children,
	side = "bottom",
	sideOffset = 8,
	align = "start",
	...props
}: ComboboxPrimitive.Popup.Props &
	Pick<ComboboxPrimitive.Positioner.Props, "side" | "sideOffset" | "align">) {
	return (
		<ComboboxPrimitive.Portal>
			<ComboboxPrimitive.Positioner
				side={side}
				sideOffset={sideOffset}
				align={align}
				className="isolate z-50"
			>
				<ComboboxPrimitive.Popup
					data-slot="combobox-content"
					className={cn(
						menuContentClass,
						"group/combobox-content flex max-h-80 w-80 flex-col overflow-hidden",
						className,
					)}
					{...props}
				>
					{children}
				</ComboboxPrimitive.Popup>
			</ComboboxPrimitive.Positioner>
		</ComboboxPrimitive.Portal>
	);
}

function ComboboxInput({ className, ...props }: ComboboxPrimitive.Input.Props) {
	return (
		<div className="relative m-1 mb-2">
			<SearchIcon
				className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-caption"
				aria-hidden
			/>
			<ComboboxPrimitive.Input
				data-slot="combobox-input"
				className={cn(inputClass, "pl-8", className)}
				{...props}
			/>
		</div>
	);
}

function ComboboxList({ className, ...props }: ComboboxPrimitive.List.Props) {
	return (
		<ComboboxPrimitive.List
			data-slot="combobox-list"
			className={cn(
				"overflow-y-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
				className,
			)}
			{...props}
		/>
	);
}

function ComboboxItem({
	className,
	children,
	...props
}: ComboboxPrimitive.Item.Props) {
	return (
		<ComboboxPrimitive.Item
			data-slot="combobox-item"
			className={cn(
				menuItemClass,
				"w-full pr-8 data-highlighted:bg-accent data-highlighted:text-accent-foreground",
				className,
			)}
			{...props}
		>
			{children}
			<ComboboxPrimitive.ItemIndicator
				render={
					<span className="pointer-events-none absolute right-2 flex size-4 items-center justify-center" />
				}
			>
				<CheckIcon />
			</ComboboxPrimitive.ItemIndicator>
		</ComboboxPrimitive.Item>
	);
}

function ComboboxEmpty({ className, ...props }: ComboboxPrimitive.Empty.Props) {
	return (
		<ComboboxPrimitive.Empty
			data-slot="combobox-empty"
			className={cn(
				"px-2 py-2 text-center text-caption text-sm empty:hidden",
				className,
			)}
			{...props}
		/>
	);
}

export {
	Combobox,
	ComboboxContent,
	ComboboxEmpty,
	ComboboxInput,
	ComboboxItem,
	ComboboxList,
	ComboboxTrigger,
	ComboboxValue,
};
