import { MoreVertical } from "lucide-react";
import { cn } from "../lib/utils";

/**
 * The phone's way into a row's or card's context menu: a touch has no right
 * click. It opens the menu the row already has, so there is one set of items.
 * Goes inside the menu's trigger, or right after it (`afterTrigger`) where the
 * trigger is itself a button.
 */
export function RowMenuButton({
	label = "More actions",
	afterTrigger = false,
	className,
}: {
	label?: string;
	afterTrigger?: boolean;
	className?: string;
}) {
	return (
		<button
			type="button"
			aria-label={label}
			aria-haspopup="menu"
			title={label}
			className={cn(
				"hidden size-11 shrink-0 items-center justify-center rounded-md text-caption outline-none hover:bg-muted hover:text-heading focus-visible:bg-muted focus-visible:text-heading phone:inline-flex",
				className,
			)}
			onClick={(event) => {
				event.stopPropagation();
				const bounds = event.currentTarget.getBoundingClientRect();
				const button = event.currentTarget;
				(afterTrigger ? button.previousElementSibling : button)?.dispatchEvent(
					new MouseEvent("contextmenu", {
						bubbles: true,
						cancelable: true,
						clientX: bounds.left,
						clientY: bounds.bottom,
					}),
				);
			}}
			onKeyDown={(event) => event.stopPropagation()}
		>
			<MoreVertical className="size-4" />
		</button>
	);
}
