import type { ReactNode } from "react";
import { cn } from "../lib/utils";
import { renderWidget } from "../widgets/registry";

export function Slot({
	widgetIds,
	className,
}: {
	widgetIds: string[];
	className?: string;
}) {
	return (
		<div className={cn("flex flex-col gap-3", className)}>
			{widgetIds.map((id) => {
				const widget = renderWidget(id);
				if (!widget) {
					return (
						<div
							key={id}
							className="rounded-sm border border-destructive/50 p-2 text-xs text-destructive"
						>
							Unknown widget: {id}
						</div>
					);
				}
				return <div key={id}>{widget}</div>;
			})}
		</div>
	);
}

export function Panel({
	side,
	collapsed,
	onToggle,
	children,
}: {
	side: "left" | "right";
	collapsed: boolean;
	onToggle: () => void;
	children: ReactNode;
}) {
	if (collapsed) {
		return (
			<aside className="flex w-10 shrink-0 flex-col items-center border-border border-r bg-muted/30 py-2">
				<button
					type="button"
					onClick={onToggle}
					className="rounded-sm px-1 text-xs hover:bg-muted"
					aria-label={`Expand ${side} panel`}
				>
					{side === "left" ? "›" : "‹"}
				</button>
			</aside>
		);
	}

	return (
		<aside className="flex w-64 shrink-0 flex-col border-border border-r bg-muted/20">
			<div className="flex items-center justify-between border-border border-b px-3 py-2">
				<span className="font-medium text-sm capitalize">{side}</span>
				<button
					type="button"
					onClick={onToggle}
					className="text-caption text-xs hover:text-foreground"
					aria-label={`Collapse ${side} panel`}
				>
					{side === "left" ? "‹" : "›"}
				</button>
			</div>
			<div className="flex-1 overflow-y-auto p-3">{children}</div>
		</aside>
	);
}
