const MINI_PLAYER_PATH = "/mini";

/**
 * The mini player window loads the app at /mini and must render without the
 * shell (sidebar, queue, player bar); only the compact player belongs there.
 */
export function isMiniPlayerRoute(pathname: string): boolean {
	return pathname === MINI_PLAYER_PATH || pathname === `${MINI_PLAYER_PATH}/`;
}
