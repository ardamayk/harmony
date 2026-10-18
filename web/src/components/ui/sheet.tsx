import { Dialog as SheetPrimitive } from "@base-ui/react/dialog";

import { cn } from "#/lib/utils.ts";

// A side drawer: the Dialog primitive pinned to the right edge.
function Sheet({ ...props }: SheetPrimitive.Root.Props) {
	return <SheetPrimitive.Root data-slot="sheet" {...props} />;
}

function SheetTrigger({ ...props }: SheetPrimitive.Trigger.Props) {
	return <SheetPrimitive.Trigger data-slot="sheet-trigger" {...props} />;
}

function SheetClose({ ...props }: SheetPrimitive.Close.Props) {
	return <SheetPrimitive.Close data-slot="sheet-close" {...props} />;
}

function SheetContent({ className, ...props }: SheetPrimitive.Popup.Props) {
	return (
		<SheetPrimitive.Portal data-slot="sheet-portal">
			<SheetPrimitive.Backdrop
				data-slot="sheet-overlay"
				className="fixed inset-0 z-50 bg-background/50 backdrop-blur-sm"
			/>
			<SheetPrimitive.Popup
				data-slot="sheet-content"
				className={cn(
					"fixed top-0 right-0 z-50 flex h-dvh w-sm flex-col gap-6 border-border border-l bg-background p-6 shadow-xl outline-none",
					className,
				)}
				{...props}
			/>
		</SheetPrimitive.Portal>
	);
}

function SheetTitle({ className, ...props }: SheetPrimitive.Title.Props) {
	return (
		<SheetPrimitive.Title
			data-slot="sheet-title"
			className={cn("font-semibold text-heading text-xl", className)}
			{...props}
		/>
	);
}

function SheetDescription({
	className,
	...props
}: SheetPrimitive.Description.Props) {
	return (
		<SheetPrimitive.Description
			data-slot="sheet-description"
			className={cn("text-caption text-sm", className)}
			{...props}
		/>
	);
}

export {
	Sheet,
	SheetClose,
	SheetContent,
	SheetDescription,
	SheetTitle,
	SheetTrigger,
};
