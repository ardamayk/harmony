import { Toaster as SonnerToaster } from "sonner";
import { useResolvedThemeMode } from "../theme/use-resolved-theme-mode";
import { PLAYER_BAR_HEIGHT_PX } from "./AppShell";
import { useLayout } from "./LayoutProvider";
import { usePhoneShell } from "./use-phone-shell";

// Above the mini player and tab bar, whose height the Phone Shell tracks.
const PHONE_OFFSET = {
	bottom: "calc(var(--player-dock-clearance) + 0.75rem)",
	left: "0.75rem",
	right: "0.75rem",
};

/**
 * Application-wide toast surface. Mounted once by AppShell so both the Web
 * and Desktop Clients raise toasts through the same `toast()` export; the
 * theme follows the user's Theme Preferences like ThemeProvider does. The
 * offset keeps toasts clear of the Player Bar along the bottom edge.
 */
export function Toaster() {
	const { preferences } = useLayout();
	const theme = useResolvedThemeMode(preferences.theme.mode);
	const isPhone = usePhoneShell();
	return (
		<SonnerToaster
			theme={theme}
			position="bottom-right"
			offset={
				isPhone ? PHONE_OFFSET : `calc(${PLAYER_BAR_HEIGHT_PX}px + 0.75rem)`
			}
			mobileOffset={isPhone ? PHONE_OFFSET : undefined}
			closeButton
			toastOptions={{
				classNames: {
					toast:
						"group toast group-[.toaster]:bg-popover group-[.toaster]:text-popover-foreground group-[.toaster]:border-border group-[.toaster]:shadow-lg",
					description: "group-[.toast]:text-caption",
					actionButton:
						"group-[.toast]:bg-primary group-[.toast]:text-primary-foreground",
					cancelButton:
						"group-[.toast]:bg-muted group-[.toast]:text-muted-foreground",
				},
			}}
		/>
	);
}
