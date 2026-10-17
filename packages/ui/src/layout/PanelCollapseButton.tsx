import { ChevronsLeft, ChevronsRight } from "lucide-react";
import { cn } from "../lib/utils";

export function PanelCollapseButton({
	edge,
	collapsed,
	onToggle,
	className,
}: {
	edge: "left" | "right";
	collapsed: boolean;
	onToggle: () => void;
	className?: string;
}) {
	const Icon = collapsed
		? edge === "left"
			? ChevronsRight
			: ChevronsLeft
		: edge === "left"
			? ChevronsLeft
			: ChevronsRight;

	const label = collapsed ? "Expand panel" : "Collapse panel";

	return (
		<button
			type="button"
			onClick={onToggle}
			aria-label={label}
			title={label}
			className={cn(
				"inline-flex size-8 shrink-0 items-center justify-center rounded-sm text-caption transition hover:bg-sidebar-accent hover:text-sidebar-foreground",
				className,
			)}
		>
			<Icon className="size-4" />
		</button>
	);
}
