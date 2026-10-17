/**
 * Geometry shared by the App Shell pieces (Top Nav, Queue Drawer, Player Bar
 * dock) and mirrored by the Web Client's page layout classes, so headers,
 * content, the bar and the drawer all sit on the same lines.
 */

/**
 * Horizontal inset every shell region and page uses. The Web Client defines
 * `--shell-inset` on `:root`; the fallback keeps the shell usable in
 * isolation (tests, storybook-like hosts).
 */
export const SHELL_INSET_CLASS = "px-(--shell-inset,2rem)";

/** Height of the Top Nav bar. */
export const TOP_NAV_HEIGHT_CLASS = "h-16";
const TOP_NAV_HEIGHT = "4rem";

/** Height of the floating Player Bar; Toaster derives its offset from this. */
export const PLAYER_BAR_HEIGHT_PX = 86;
/** Gap between the bottom edge and the Player Bar. */
export const PLAYER_BAR_INSET_PX = 16;

/** Width of the Queue Drawer. */
export const QUEUE_DRAWER_WIDTH = "var(--queue-drawer-width, 18rem)";
/** Space between the open Queue Drawer and the content, nav and Player Bar. */
const QUEUE_DRAWER_GAP = "1.5rem";
/** Margin between the Top Nav and the Queue Drawer's top edge. */
const QUEUE_DRAWER_TOP_GAP = "1rem";

/** Where the drawer's top edge lands: a margin below the Top Nav. */
export const QUEUE_DRAWER_TOP = `calc(${TOP_NAV_HEIGHT} + ${QUEUE_DRAWER_TOP_GAP})`;

/** Where the drawer's bottom edge lands when a Player Bar is docked. */
export const QUEUE_DRAWER_BOTTOM_ABOVE_PLAYER = `calc(${PLAYER_BAR_INSET_PX}px + ${PLAYER_BAR_HEIGHT_PX}px + ${QUEUE_DRAWER_GAP})`;
/** Where the drawer's bottom edge lands without a Player Bar. */
export const QUEUE_DRAWER_BOTTOM = `${PLAYER_BAR_INSET_PX}px`;

/** Extra right padding the content takes while the drawer is open. */
export const QUEUE_DRAWER_CONTENT_CLEARANCE = `calc(${QUEUE_DRAWER_WIDTH} + ${QUEUE_DRAWER_GAP})`;
