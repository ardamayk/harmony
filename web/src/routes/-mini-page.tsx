import { MiniPlayerWindow } from "@repo/ui";
import { isDesktopClient } from "#/desktop/bridge";
import { closeMiniPlayer, showMainWindow } from "#/desktop/mini-player-bridge";

// Route components live outside the route file so automatic code splitting
// never renders a component from a chunk that is still evaluating.
export function MiniPlayerPage() {
	const isDesktop = isDesktopClient();
	return (
		<MiniPlayerWindow
			onExpand={
				isDesktop
					? () => {
							void showMainWindow().then(() => closeMiniPlayer());
						}
					: undefined
			}
			onClose={isDesktop ? () => void closeMiniPlayer() : undefined}
		/>
	);
}
