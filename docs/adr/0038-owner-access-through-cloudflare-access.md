---
status: accepted
---

# Owner access through Cloudflare Access

Running the Music Server on a VPS leaves the Owner on the far side of an SSH tunnel for every use of the Web Client, and [ADR 0037](0037-listen-together-rooms-on-the-owners-music-server.md) left Owner sign-in to a later decision. This is that decision: the Owner's listener may answer one public host, and only with a token from Cloudflare Access.

## How it works

- `OWNER_PUBLIC_URL`, `CF_ACCESS_TEAM_DOMAIN` and `CF_ACCESS_AUD` are set together or not at all. Unset, the Owner's listener is exactly as ADR 0037 describes.
- `SERVER_ADDR` stays loopback. A `cloudflared` tunnel on the same host carries the traffic, so no port opens and nothing can reach the listener without passing Access.
- Who may sign in is the Access policy's business: One-time PIN for a browser and a Service Token for a program both end in the same signed token, so the server does not change with the method.
- The server verifies that token itself on every request for `OWNER_PUBLIC_URL`'s host: RS256 only, signed with a key from the team's certs endpoint, with the application's audience, the team's issuer and a live expiry. An unknown key refreshes the certs at most once a minute. Without keys it refuses, so a Cloudflare outage cannot open the listener.
- A request addressed to loopback needs no token, as before. The Host check stays what separates the two.
- The Guest Listener stays separate, on a host of its own, and `OWNER_PUBLIC_URL` must differ from `PUBLIC_URL`.

## Considered Options

- Owner sign-in in the Music Server: rejected for now. A password or PIN would face the internet, and the server would need sessions and attempt limits of its own.
- Trusting Access without checking its token: rejected. One wrong policy would hand the whole server, deletion included, to the internet.
- NetBird Reverse Proxy with a fixed PIN and a header token: rejected. It is in beta, and nothing signed reaches the server, so the proxy would be the only check.
- A DNS record to Traefik: rejected. Traefik cannot reach loopback, so a forwarder would be needed, and a neighbouring container could send it a loopback Host and skip Access.

## Consequences

- This narrows ADR 0037's "never exposed": the listener is reachable from the internet through Access. The trust in Cloudflare is the price, and the operator's own uploads keep Cloudflare's request size limit, so very large files still go through an SSH tunnel.
- A forwarder in front of `SERVER_ADDR` would undo this, because a loopback Host needs no token. The documentation says so.
- The Desktop Client still accepts loopback origins only. Reaching a public address with a Service Token is a separate change in the core.
