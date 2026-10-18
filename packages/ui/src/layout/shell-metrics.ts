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

/** Height of the Player Bar, docked on the bottom edge; Toaster derives its offset from this. */
export const PLAYER_BAR_HEIGHT_PX = 86;

/** Width of the Queue Drawer. */
export const QUEUE_DRAWER_WIDTH = "var(--queue-drawer-width, 18rem)";
/** The drawer is docked: its top edge on the Top Nav's bottom edge. */
export const QUEUE_DRAWER_TOP = TOP_NAV_HEIGHT;

/** Where the drawer's bottom edge lands: on the docked Player Bar's top edge. */
export const QUEUE_DRAWER_BOTTOM_ABOVE_PLAYER = `${PLAYER_BAR_HEIGHT_PX}px`;
/** Where the drawer's bottom edge lands without a Player Bar. */
export const QUEUE_DRAWER_BOTTOM = "0px";

/** Extra right padding the content takes while the drawer is open; the
 * page's own inset then keeps content off the drawer's edge. */
export const QUEUE_DRAWER_CONTENT_CLEARANCE = QUEUE_DRAWER_WIDTH;
