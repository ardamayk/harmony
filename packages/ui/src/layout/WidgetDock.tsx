import {
	closestCenter,
	DndContext,
	type DragEndEvent,
	KeyboardSensor,
	PointerSensor,
	useSensor,
	useSensors,
} from "@dnd-kit/core";
import {
	SortableContext,
	sortableKeyboardCoordinates,
	useSortable,
	verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GripVertical } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "../lib/utils";
import { renderWidget, widgetRegistry } from "../widgets/registry";
import { useLayout } from "./LayoutProvider";

type PanelSide = "left" | "right";

function SortableWidget({ id, panel }: { id: string; panel: PanelSide }) {
	const {
		attributes,
		listeners,
		setNodeRef,
		transform,
		transition,
		isDragging,
	} = useSortable({
		id,
		data: { panel },
	});

	const widget = renderWidget(id);
	const title = widgetRegistry.find((w) => w.id === id)?.title ?? id;

	const style = {
		transform: CSS.Transform.toString(transform),
		transition,
	};

	if (!widget) {
		return (
			<div
				ref={setNodeRef}
				style={style}
				className="rounded border border-destructive/50 p-2 text-destructive text-xs"
			>
				Unknown widget: {id}
			</div>
		);
	}

	return (
		<div
			ref={setNodeRef}
			style={style}
			className={cn(
				"min-w-0 overflow-hidden rounded-lg border border-border bg-card",
				isDragging && "opacity-60 shadow-md",
			)}
		>
			<div className="flex min-w-0 items-center gap-1 border-border border-b px-2 py-1">
				<button
					type="button"
					className="shrink-0 cursor-grab text-caption hover:text-foreground active:cursor-grabbing"
					aria-label={`Drag ${title}`}
					{...attributes}
					{...listeners}
				>
					<GripVertical className="size-4" />
				</button>
				<span className="min-w-0 truncate font-medium text-xs">{title}</span>
			</div>
			<div className="min-w-0 p-2">{widget}</div>
		</div>
	);
}

export function WidgetDock({ panel }: { panel: PanelSide }) {
	const { preferences } = useLayout();
	const widgetIds = preferences.layout.panels[panel];

	if (widgetIds.length === 0) {
		return <p className="px-2 py-3 text-caption text-xs">Drop widgets here</p>;
	}

	return (
		<SortableContext items={widgetIds} strategy={verticalListSortingStrategy}>
			<div
				className="flex min-w-0 flex-col gap-3 p-2 [contain:inline-size]"
				data-widget-dock
			>
				{widgetIds.map((id) => (
					<SortableWidget key={id} id={id} panel={panel} />
				))}
			</div>
		</SortableContext>
	);
}

export function WidgetDndProvider({ children }: { children: ReactNode }) {
	const { preferences, reorderWidgets, moveWidgetToPanel } = useLayout();
	const sensors = useSensors(
		useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
		useSensor(KeyboardSensor, {
			coordinateGetter: sortableKeyboardCoordinates,
		}),
	);

	const handleDragEnd = (event: DragEndEvent) => {
		const { active, over } = event;
		if (!over || active.id === over.id) return;

		const widgetId = String(active.id);
		const activePanel = active.data.current?.panel as PanelSide | undefined;
		const overPanel =
			(over.data.current?.panel as PanelSide | undefined) ?? activePanel;

		if (!activePanel || !overPanel) return;

		if (activePanel === overPanel) {
			reorderWidgets(activePanel, widgetId, String(over.id));
			return;
		}

		const targetIds = preferences.layout.panels[overPanel];
		const overIndex = targetIds.indexOf(String(over.id));
		moveWidgetToPanel(
			widgetId,
			overPanel,
			overIndex >= 0 ? overIndex : targetIds.length,
		);
	};

	return (
		<DndContext
			sensors={sensors}
			collisionDetection={closestCenter}
			onDragEnd={handleDragEnd}
		>
			{children}
		</DndContext>
	);
}
