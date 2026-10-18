import type { ErrorComponentProps } from "@tanstack/react-router";
import { Link } from "@tanstack/react-router";
import { Button, buttonVariants } from "#/components/ui/button";

/**
 * Last-resort screen for an unhandled render error. It keeps the app frame
 * usable instead of showing the router's raw stack trace; nothing is reported
 * anywhere because the Music Server is self-hosted.
 */
export function RootErrorComponent({ error, reset }: ErrorComponentProps) {
	const message = error instanceof Error ? error.message : String(error);
	return (
		<div
			role="alert"
			className="flex min-h-0 flex-1 flex-col items-center justify-center gap-4 p-8 text-center"
		>
			<h1 className="font-semibold text-2xl text-heading">
				Something went wrong
			</h1>
			<p className="max-w-lg break-words text-foreground text-sm">{message}</p>
			<div className="flex gap-2">
				<Button type="button" variant="outline" onClick={reset}>
					Try again
				</Button>
				<Link to="/library/albums" className={buttonVariants()}>
					Go to albums
				</Link>
			</div>
		</div>
	);
}
