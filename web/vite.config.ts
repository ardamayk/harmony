import babel from "@rolldown/plugin-babel";
import tailwindcss from "@tailwindcss/vite";
import { devtools } from "@tanstack/devtools-vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import viteReact, { reactCompilerPreset } from "@vitejs/plugin-react";
import { defineConfig, lazyPlugins } from "vite-plus";

const LAZY_HLS_CHUNK_SIZE_WARNING_LIMIT_KB = 550;
const DEFAULT_API_PROXY_TARGET = "http://localhost:8090";

const apiProxy = {
	target: process.env.VITE_PROXY_TARGET ?? DEFAULT_API_PROXY_TARGET,
	changeOrigin: true,
};

const proxyRoutes = {
	"/api": apiProxy,
	"/docs": apiProxy,
};

const config = defineConfig({
	resolve: { tsconfigPaths: true },
	build: {
		chunkSizeWarningLimit: LAZY_HLS_CHUNK_SIZE_WARNING_LIMIT_KB,
		outDir: "dist",
		emptyOutDir: true,
	},
	server: {
		port: 3000,
		proxy: proxyRoutes,
	},
	preview: { proxy: proxyRoutes },
	plugins: lazyPlugins(() => [
		devtools(),
		tailwindcss(),
		tanstackRouter({ target: "react", autoCodeSplitting: true }),
		viteReact(),
		babel({ presets: [reactCompilerPreset()] }),
	]),
});

export default config;
