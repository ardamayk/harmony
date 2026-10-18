import mdx from "@mdx-js/rollup";
import react from "@vitejs/plugin-react";
import { defineConfig, lazyPlugins } from "vite-plus";

export default defineConfig({
	base: "/docs/",
	plugins: lazyPlugins(() => [{ enforce: "pre", ...mdx() }, react()]),
	build: {
		outDir: "dist",
		emptyOutDir: true,
	},
});
