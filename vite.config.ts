import { defineConfig } from "vite-plus";

const generatedAndBuildOutput = [
	"**/dist/**",
	"**/.output/**",
	"**/src/generated/**",
	"**/routeTree.gen.ts",
	"desktop/core/**",
	"target/**",
	"graphify-out/**",
	"server/**",
	"docs/**",
	".agents/**",
	".claude/**",
	".codex/**",
	".cursor/**",
	".superpowers/**",
	".playwright*/**",
];

export default defineConfig({
	fmt: {
		useTabs: true,
		printWidth: 80,
		ignorePatterns: [
			...generatedAndBuildOutput,
			"**/*.md",
			"**/*.mdx",
			"**/*.yaml",
			"**/*.yml",
		],
		overrides: [
			{ files: ["**/*.json", "**/*.jsonc"], options: { useTabs: false } },
		],
	},
	lint: {
		ignorePatterns: generatedAndBuildOutput,
		plugins: ["typescript", "react", "jsx-a11y", "import", "vitest"],
		categories: { correctness: "error" },
		jsPlugins: [
			{ name: "vite-plus", specifier: "vite-plus/oxlint-plugin" },
			{ name: "shadcn", specifier: "@shadcn/lint" },
		],
		rules: {
			"shadcn/no-arbitrary-values": "warn",
			"shadcn/no-raw-colors": "warn",
			// Range-input and equalizer styling needs ::-webkit-slider-* and
			// keyframe rules that stay as plain CSS classes in web/src/styles.css.
			"shadcn/no-unknown-classes": [
				"warn",
				{
					allow: [
						"player-seek-slider",
						"player-seek-slider--waveform",
						"player-volume-slider",
						"queue-equalizer",
					],
				},
			],
			"shadcn/no-inline-styles": "warn",
			"shadcn/require-static-classes": "warn",
			"shadcn/no-restyle": ["warn", { allow: ["layout"] }],
			"vite-plus/prefer-vite-plus-imports": "error",
			"react/react-in-jsx-scope": "off",
			"vitest/require-mock-type-parameters": "off",
			// Listbox/option/dialog widgets intentionally use ARIA roles.
			"jsx-a11y/prefer-tag-over-role": "off",
			// React Compiler diagnostics.
			"react/set-state-in-effect": "error",
			"react/refs": "error",
			"react/static-components": "error",
			"react/preserve-manual-memoization": "error",
			"react-hooks/exhaustive-deps": "error",
		},
		overrides: [
			{
				// node:test registers tests through returned promises.
				files: ["scripts/**"],
				rules: { "typescript/no-floating-promises": "off" },
			},
			{
				// Sortable queue rows must stay list items.
				files: ["packages/ui/src/layout/QueueTrackRow.tsx"],
				rules: {
					"jsx-a11y/no-noninteractive-element-to-interactive-role": "off",
				},
			},
			{
				files: ["**/*.test.{ts,tsx}", "**/testing/**"],
				rules: {
					"typescript/unbound-method": "off",
					"typescript/no-base-to-string": "off",
					"typescript/no-misused-spread": "off",
					"vitest/no-conditional-expect": "off",
					"jsx-a11y/no-noninteractive-element-to-interactive-role": "off",
				},
			},
		],
		options: { typeAware: true, typeCheck: true },
	},
	staged: {
		"*.{js,cjs,mjs,jsx,ts,tsx,json,jsonc}": "vp check --fix",
		"{desktop/core,server}/**/*.rs": () =>
			"bash scripts/check-staged-rust-format.sh",
	},
});
