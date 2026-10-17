import { Radio as RadioPrimitive } from "@base-ui/react/radio";
import { RadioGroup as RadioGroupPrimitive } from "@base-ui/react/radio-group";

import { cn } from "#/lib/utils.ts";

function RadioGroup({ className, ...props }: RadioGroupPrimitive.Props) {
	return (
		<RadioGroupPrimitive
			data-slot="radio-group"
			className={cn("grid w-full gap-2", className)}
			{...props}
		/>
	);
}

function RadioGroupItem({ className, ...props }: RadioPrimitive.Root.Props) {
	return (
		<RadioPrimitive.Root
			data-slot="radio-group-item"
			className={cn(
				"peer relative flex aspect-square size-4 shrink-0 items-center justify-center rounded-full border border-input outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 data-checked:border-primary data-disabled:cursor-not-allowed data-disabled:opacity-50",
				className,
			)}
			{...props}
		>
			<RadioPrimitive.Indicator
				data-slot="radio-group-indicator"
				className="size-2 rounded-full bg-primary"
			/>
		</RadioPrimitive.Root>
	);
}

export { RadioGroup, RadioGroupItem };
