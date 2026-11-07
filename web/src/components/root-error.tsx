import type { ErrorComponentProps } from "@tanstack/react-router";
import { Link } from "@tanstack/react-router";
import { useEffect } from "react";
import { Button, buttonVariants } from "#/components/ui/button";
import { reportClientError } from "#/lib/client-errors";

/**
 * Last-resort screen for an unhandled render error. It keeps the app frame
 * usable instead of showing the router's raw stack trace. The router catches
 * the error, so it is reported here: to the owner's Sentry through the Music
 * Server when it advertises client-errors.v1 (ADR 0043), otherwise nowhere.
 */
export function RootErrorComponent({ error, reset }: ErrorComponentProps) {
	useEffect(() => reportClientError(error), [error]);
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
