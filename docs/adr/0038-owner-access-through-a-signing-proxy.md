---
status: accepted
---

# Owner access through a signing proxy

Running the Music Server on a VPS leaves the Owner on the far side of an SSH tunnel for every use of the Web Client, and [ADR 0037](0037-listen-together-rooms-on-the-owners-music-server.md) left Owner sign-in to a later decision. This is that decision: the Owner's listener may answer one public host, and only with a token signed by the identity proxy the operator runs in front of it. Which proxy that is stays the operator's business, so nothing in the code names one.

## How it works

- `OWNER_PUBLIC_URL`, `OWNER_JWT_HEADER`, `OWNER_JWKS_URL`, `OWNER_JWT_ISSUER` and `OWNER_JWT_AUDIENCE` are set together or not at all. Unset, the Owner's listener is exactly as ADR 0037 describes.
- `SERVER_ADDR` stays loopback, so the proxy has to run on the same host, or reach it through a tunnel that does.
- Who may sign in is the proxy's business. A person and a program can use different methods as long as both end in the same signed token.
- The server verifies that token itself on every request for `OWNER_PUBLIC_URL`'s host: signed with a key from the JWKS, with the configured audience and issuer and a live expiry. RS256 and ES256 keys are accepted, and each key allows only its own algorithm, so a token cannot choose how it is checked. An unknown key refreshes the JWKS at most once a minute. Without keys it refuses, so an outage at the proxy's provider cannot open the listener.
- A request addressed to loopback needs no token, as before. The Host check stays what separates the two.
- The Guest Listener stays separate, on a host of its own, and `OWNER_PUBLIC_URL` must differ from `PUBLIC_URL`.

## Considered Options

- Owner sign-in in the Music Server: rejected for now. A password or PIN would face the internet, and the server would need sessions and attempt limits of its own.
- A public host with no token, trusting the proxy: rejected. One wrong policy would hand the whole server, deletion included, to the internet.
- Settings for one provider: rejected. Operators host where they choose, and a JWT, a JWKS and a header name cover every proxy that signs requests.
- A DNS record to a containerized reverse proxy: rejected for this server's shape. It cannot reach loopback, so a forwarder would be needed, and a neighbouring container could send it a loopback Host and skip the token.

## Consequences

- This narrows ADR 0037's "never exposed": the listener is reachable from the internet through the proxy. The operator trusts that proxy, and its limits, such as a maximum request size, apply to uploads. Files over such a limit still go through an SSH tunnel.
- A forwarder in front of `SERVER_ADDR` would undo this, because a loopback Host needs no token. The documentation says so.
- An operator without a signing proxy keeps the SSH tunnel.
