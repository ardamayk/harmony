export function ImportErrors({ message }: { message: string | undefined }) {
	if (!message) return null;
	return (
		<ul
			aria-label="Import errors"
			className="list-inside list-disc space-y-1 text-destructive text-sm"
		>
			{Array.from(new Set(message.split("\n").filter(Boolean))).map((line) => (
				<li key={line}>{line}</li>
			))}
		</ul>
	);
}
