import { Search } from "lucide-react";
import type * as React from "react";
import { Input } from "#/components/ui/input";
import { cn } from "#/lib/utils";

/**
 * Search box with the leading icon every page uses. `className` sizes the
 * wrapper; `variant="shell"` is the header search on a shell surface.
 */
export function SearchField({
	className,
	...props
}: React.ComponentProps<typeof Input>) {
	return (
		<div className={cn("relative", className)}>
			{/* The shell field takes the Player Bar's tones; the icon, its placeholder's. */}
			<Search
				className={cn(
					"pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2",
					props.variant === "shell"
						? "text-player-foreground/75"
						: "text-caption",
				)}
			/>
			<Input leadingIcon {...props} />
		</div>
	);
}
