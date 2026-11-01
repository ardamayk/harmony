import { readFileSync } from "node:fs";
import { join } from "node:path";
import { PHONE_SHELL_QUERY } from "@repo/ui";
import { expect, it } from "vite-plus/test";

it("keeps the phone variant on the phone shell's query", () => {
	const styles = readFileSync(join(import.meta.dirname, "styles.css"), "utf8");
	// The variant, and the global rules that cannot use it.
	expect(styles.split(`@media ${PHONE_SHELL_QUERY} {`)).toHaveLength(3);
});
