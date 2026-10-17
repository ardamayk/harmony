/**
 * Horizontal padding every page region shares so headers, list content and
 * detail content line up along the same edges as the Top Nav and the Player
 * Bar. `--shell-inset` is defined in styles.css and mirrored by the App
 * Shell in packages/ui (shell-metrics.ts).
 */
export const PAGE_CONTENT_PADDING_CLASS = "px-(--shell-inset) py-6";

/**
 * Page content fills the column between the insets; the App Shell narrows
 * that column while the Queue Drawer is open, so no max-width cap is needed.
 * `page-content-column` is a marker (no styles) so tests can prove a header
 * and its content share the column.
 */
export const PAGE_CONTENT_WIDTH_CLASS = "page-content-column w-full";

export const HEADER_SEARCH_CONTAINER_CLASS = "relative w-full sm:w-[28rem]";
export const HEADER_SEARCH_INPUT_CLASS =
	"h-11 rounded-lg bg-player pl-8 text-sm";
