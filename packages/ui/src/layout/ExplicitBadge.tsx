/** Marks a Track with explicit lyrics, after its title. */
export function ExplicitBadge() {
	return (
		<span
			role="img"
			aria-label="Explicit"
			title="Explicit"
			className="inline-flex size-4 shrink-0 items-center justify-center rounded-sm bg-foreground font-semibold text-background text-xs leading-none"
		>
			E
		</span>
	);
}
