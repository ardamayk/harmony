import { Input as InputPrimitive } from "@base-ui/react/input";
import { cva, type VariantProps } from "class-variance-authority";
import type * as React from "react";

import { cn } from "#/lib/utils.ts";

const inputVariants = cva(
	"h-9 w-full min-w-0 rounded-sm border border-input bg-transparent px-3 py-1 text-base shadow-xs transition-ring outline-none selection:bg-primary selection:text-primary-foreground file:mr-3 file:inline-flex file:h-7 file:cursor-pointer file:items-center file:rounded-sm file:border-0 file:bg-muted file:px-3 file:text-sm file:font-medium file:text-heading hover:file:bg-neutral-active placeholder:text-muted-foreground disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm dark:bg-input/30 focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 aria-invalid:border-destructive aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40",
	{
		variants: {
			variant: {
				default: "",
				// Search field on a shell surface (page header, Player Bar).
				shell:
					"h-11 rounded-lg border-border bg-player text-sm text-player-foreground placeholder:text-player-foreground/75",
			},
			leadingIcon: {
				true: "pl-8",
			},
		},
		defaultVariants: {
			variant: "default",
		},
	},
);

function Input({
	className,
	type,
	variant,
	leadingIcon,
	...props
}: React.ComponentProps<"input"> & VariantProps<typeof inputVariants>) {
	return (
		<InputPrimitive
			type={type}
			data-slot="input"
			className={cn(inputVariants({ variant, leadingIcon }), className)}
			{...props}
		/>
	);
}

export { Input, inputVariants };
