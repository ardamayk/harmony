import { defineConfig } from "vite-plus";

// The shell is two Node bundles Electron loads directly. They build
// separately so neither imports a shared chunk: a sandboxed preload may only
// require "electron", and must be CommonJS. `electron` comes from the runtime;
// zod (the IPC schemas in main) is bundled, since the app ships no node_modules.
function bundle(name: string, entry: string) {
	return {
		entry: { [name]: entry },
		format: "cjs" as const,
		platform: "node" as const,
		target: "node24",
		outDir: "dist",
		clean: false,
		deps: { neverBundle: ["electron"], onlyBundle: ["zod"] },
	};
}

export default defineConfig({
	pack: [
		bundle("main", "src/main/main.ts"),
		bundle("preload", "src/preload/preload.ts"),
	],
	test: {
		include: ["src/**/*.test.ts"],
		environment: "node",
	},
});
