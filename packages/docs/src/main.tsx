import { StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import IndexPage from "../content/index.mdx";
import GettingStarted from "../content/getting-started.mdx";
import ApiReference from "../content/api-reference.mdx";
import Architecture from "../content/architecture.mdx";
import LayoutCustomization from "../content/layout-customization.mdx";
import ListenTogether from "../content/listen-together.mdx";

const pages = {
	index: { title: "Home", Component: IndexPage },
	"getting-started": { title: "Getting Started", Component: GettingStarted },
	architecture: { title: "Architecture", Component: Architecture },
	"layout-customization": {
		title: "Layout Customization",
		Component: LayoutCustomization,
	},
	"listen-together": { title: "Listen Together", Component: ListenTogether },
	"api-reference": { title: "API Reference", Component: ApiReference },
} as const;

type PageKey = keyof typeof pages;

/** A link can open a page directly, as the Listen Together page's does. */
function initialPage(): PageKey {
	const key = window.location.hash.slice(1);
	return Object.hasOwn(pages, key) ? (key as PageKey) : "index";
}

function App() {
	const [page, setPage] = useState<PageKey>(initialPage);
	const { Component } = pages[page];

	return (
		<>
			<nav>
				{(Object.keys(pages) as PageKey[]).map((key) => (
					<a
						key={key}
						href={`#${key}`}
						onClick={(e) => {
							e.preventDefault();
							setPage(key);
						}}
						aria-current={page === key ? "page" : undefined}
					>
						{pages[key].title}
					</a>
				))}
			</nav>
			<article>
				<Component />
			</article>
		</>
	);
}

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<App />
	</StrictMode>,
);
