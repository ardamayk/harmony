import { Progress as ProgressPrimitive } from "@base-ui/react/progress";

import { cn } from "#/lib/utils.ts";

function Progress({ className, ...props }: ProgressPrimitive.Root.Props) {
	return (
		<ProgressPrimitive.Root data-slot="progress" {...props}>
			<ProgressPrimitive.Track
				data-slot="progress-track"
				className={cn(
					"block h-2 overflow-hidden rounded-full bg-secondary",
					className,
				)}
			>
				<ProgressPrimitive.Indicator
					data-slot="progress-indicator"
					className="block h-full bg-primary transition-[width]"
				/>
			</ProgressPrimitive.Track>
		</ProgressPrimitive.Root>
	);
}

export { Progress };
