import { useSearch } from "@tanstack/react-router";
import { ArtistsPage } from "./-artists-page";

// Route components live outside the route file: with automatic code
// splitting, a component defined inside the route module can be rendered
// from its split chunk before that chunk finished evaluating after an HMR
// update in WebKit, which surfaces as "_s is not a function".
export function ArtistsRoute() {
	const { q } = useSearch({ from: "/library/artists/" });
	return <ArtistsPage initialSearch={q} />;
}
