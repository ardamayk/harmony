import { type MouseEvent, useState } from "react";
import { isDesktopClient, openExternalUrl } from "#/desktop/bridge";
import { cn } from "#/lib/utils";

/**
 * Click handler for an external `<a target="_blank">`: the Desktop Client's
 * webview cannot open windows, so the link goes to the system browser.
 */
export function openExternalLink(href: string) {
	return (event: MouseEvent<HTMLAnchorElement>) => {
		if (!isDesktopClient()) return;
		event.preventDefault();
		void openExternalUrl(href).catch((error) => {
			console.warn("Failed to open external link", { href, error });
		});
	};
}

/**
 * External links open a new browser tab on the web. Inside the Desktop
 * Client the webview cannot open windows, so the link is handed to the
 * system browser instead.
 */
export function ExternalLinkButton({
	href,
	name,
	short,
	iconSrc,
	iconClassName,
}: {
	href: string;
	name: string;
	short: string;
	iconSrc?: string;
	iconClassName?: string;
}) {
	const [iconFailed, setIconFailed] = useState(false);
	const showIcon = iconSrc && !iconFailed;

	return (
		<a
			href={href}
			target="_blank"
			rel="noopener noreferrer"
			title={name}
			// The logo alone, no ring around it; hover and focus show the
			// neutral step behind it.
			className="flex size-10 items-center justify-center rounded-md text-caption transition hover:bg-muted hover:text-foreground focus-visible:bg-muted"
			onClick={openExternalLink(href)}
		>
			{showIcon ? (
				<img
					src={iconSrc}
					alt=""
					className={cn("size-8 object-contain", iconClassName)}
					onError={() => setIconFailed(true)}
				/>
			) : (
				<span className="font-semibold text-xs">{short}</span>
			)}
		</a>
	);
}
