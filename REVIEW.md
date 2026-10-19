# Review guide

What the automated reviewers (`.github/workflows/claude-review.yml`) look for and how they work. Each section says what a failure looks like in that part of the code; flag those, not style. CLAUDE.md holds the coding rules; this file holds what matters when they are broken.

## Process

Both passes, bug and security, run in rounds. In each round three finders look for problems in parallel, then three voters judge every candidate, and the workflow posts the candidates that at least two voters call real. The prompt says which role you have. Neither role posts to the PR.

### Finder

1. Get the diff with `gh pr diff`. When the prompt gives a `REVIEWED UP TO` commit, earlier rounds already reviewed the PR up to it: review only the changes since then, from `gh api repos/<REPO>/compare/<REVIEWED UP TO>...<HEAD SHA>`. If that call fails (for example after a force push), review the full diff.
2. Read the PR's existing review comments with `gh api repos/<REPO>/pulls/<PR NUMBER>/comments`. A finding already posted that the code still has: report it again with that comment's id as `previous_comment_id`, so it still counts but is not posted twice. A finding the author answered with a reason it is not a problem is closed; leave it out.
3. Before reporting a problem, open the code and confirm it is real. If you are not sure, leave it out.
4. Give each problem a file path and a line in the PR's version, on a line the diff touches where you can. When the same mistake appears in several places, report it once and name every place in `detail`, so it is fixed in one go. Skip style nits a linter would catch.
5. Write `title` and `detail` in plain, full English; people read them.
6. Return `findings`: every problem you confirmed, or an empty list.

### Voter

1. Read `.claude-review/candidates.json`: the finders' candidates, each with an `id`.
2. For each candidate, open the code and decide whether it is a real problem in this PR under this file's section for the pass. Be strict: a guess, a style point, or something that belongs to the other pass is not real.
3. Return `votes`: one per candidate `id`, with `real` and a one-sentence `reason`.

## Skip

- `server/.sqlx/` and `packages/api-client/src/generated/`: generated; flag only when the source they come from changed without them.
- `docs/design-refs/`, lockfiles.
- Anything Oxlint, Oxfmt, Clippy or `vp check` already reports.

## Bug review

- **Migrations** (`server/migrations`): it destroys data. An existing migration edited instead of a new one added, a Down that drops data the Up kept, a column made `NOT NULL` without a default for existing rows.
- **Client-facing JSON** (`server/src/gojson.rs`, handlers): it changes shape. A renamed or dropped field, a whole float written as `1.0`, a time not in RFC 3339; the Web and Desktop Clients break silently.
- **Contracts** (`packages/contracts/openapi.yaml`): it drifts. A handler changed without the contract, or the contract without a regenerated client.
- **Database writes**: they half-apply. Multi-statement writes outside `db::begin_write`, an `unwrap` on a nullable column instead of an `AS "name!"` override.
- **Module layering**: one module reaching into another module's `repository.rs`.
- **Managed Import** (`server/src/modules/managed_import`): it loses the user's files. The previous file removed before the replacement is verified, a path that follows a symlink out of Managed Storage.
- **UI** (`web/src`, `packages/ui/src`): it works in one client only. A Web-only API used in shared UI, a value outside `docs/design-rules.md` (read it when the diff touches these paths).
- **Docs**: they lie. A README or doc that says something the code does not do.
- **New logic without a test**: a new branch, parser or loop with no test covering it.

## Security review

The Music Server has no authentication yet: every request is the single default user (`server/src/auth.rs`). Do not flag missing login on an endpoint; flag what is dangerous even for a single-user, self-hosted server.

Flag only a concrete exploit path in this diff: trace the input to the sink in the code. A theoretical issue with no path to exploitation stays out. Each finding's `detail` says how it is exploited.

- **Filesystem**: a request-supplied path reaching the disk outside the library or Managed Storage (`..`, absolute paths, symlinks).
- **Radio proxy** (`server/src/modules/radio`): it fetches a URL that was not signed by the server's HLS resource token, letting a request make the server fetch arbitrary URLs.
- **SQL**: a query built from strings instead of `sqlx::query!` parameters.
- **Desktop trust boundary** (`desktop/src/main/security.ts`, `packages/desktop-bridge`): the CSP widened, `nodeIntegration`/`sandbox`/`contextIsolation` relaxed, a command added to the allowlist without validating its arguments, the media proxy serving without its token.
- **Web**: user or remote text (lyrics, biographies, tags) rendered as HTML.
- **Secrets**: a token, key or password committed, logged or sent to a third party.
