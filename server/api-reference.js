// Mounts Scalar's API reference on /api/docs (ADR 0035). Everything the page
// needs comes from this Music Server: the response's Content-Security-Policy
// allows no other host, and the options below switch off the features that
// would call Scalar's hosted services.
Scalar.createApiReference("#app", {
	url: "/api/openapi.yaml",
	// "Try it" requests go to the server that serves this page, not the
	// development address in the spec's `servers`.
	servers: [{ url: window.location.origin, description: "This Music Server" }],
	// Inter and JetBrains Mono load from fonts.scalar.com; use system fonts.
	withDefaultFonts: false,
	telemetry: false,
	// The AI agent chat, MCP links and developer tools (registry sharing,
	// dashboard deploy) talk to scalar.com; on localhost the agent and the
	// developer tools would otherwise be on by default.
	agent: { disabled: true },
	mcp: { disabled: true },
	showDeveloperTools: "never",
	hideClientButton: true,
});
