"""Merge finder candidates and tally voter verdicts for the Claude review.

Used by .github/workflows/claude-review-pass.yml:

  claude_review.py merge <finder-dir> <out.json>
      Joins the finders' candidates into one numbered list, dropping duplicates
      (the same earlier comment, or the same file within a few lines).

  claude_review.py post <candidates.json> <voter-dir>
      Keeps candidates that at least VOTES_NEEDED voters judged real, posts the
      new ones as inline comments plus one summary comment, and writes
      `findings` (open problems) to $GITHUB_OUTPUT.

Each file in a finder or voter directory is that agent's structured output.
With DRY_RUN=1 the GitHub calls are printed instead of made.
"""

import json
import os
import subprocess
import sys
from pathlib import Path

DUPLICATE_LINE_DISTANCE = 3
# The account this workflow posts as. Only its comments count as already
# posted: a finder can point at another bot's comment on the same problem.
OWN_LOGIN = "github-actions[bot]"


def load_outputs(directory, key):
    items = []
    for path in sorted(Path(directory).rglob("*.json")):
        items.append(json.loads(path.read_text()).get(key, []))
    return items


def is_duplicate(candidate, kept):
    previous = candidate.get("previous_comment_id")
    if previous and previous == kept.get("previous_comment_id"):
        return True
    return (
        candidate["path"] == kept["path"]
        and abs(candidate["line"] - kept["line"]) <= DUPLICATE_LINE_DISTANCE
    )


def merge(finder_outputs):
    merged = []
    for findings in finder_outputs:
        for candidate in findings:
            if not any(is_duplicate(candidate, kept) for kept in merged):
                merged.append({**candidate, "id": f"C{len(merged) + 1}"})
    return merged


def accepted(candidates, voter_outputs, votes_needed):
    real = {}
    for votes in voter_outputs:
        for vote in votes:
            if vote.get("real"):
                real[vote["id"]] = real.get(vote["id"], 0) + 1
    return [
        {**c, "votes": real.get(c["id"], 0)}
        for c in candidates
        if real.get(c["id"], 0) >= votes_needed
    ]


def gh(*args):
    if os.environ.get("DRY_RUN"):
        print("gh", *args)
        return True
    return subprocess.run(["gh", *args], capture_output=True).returncode == 0


def own_comment_ids(repo, pr):
    if os.environ.get("DRY_RUN"):
        return None
    result = subprocess.run(
        [
            "gh", "api", "--paginate", f"repos/{repo}/pulls/{pr}/comments",
            "--jq", f'.[] | select(.user.login == "{OWN_LOGIN}") | .id',
        ],
        capture_output=True, text=True, check=True,
    )
    return {int(line) for line in result.stdout.split()}


def post(found, candidate_count, env):
    repo, pr, label = env["REPO"], env["PR"], env["PASS_LABEL"]
    voters = env.get("VOTERS", "3")
    own = own_comment_ids(repo, pr)
    new = [
        f for f in found
        if not f.get("previous_comment_id")
        or (own is not None and f["previous_comment_id"] not in own)
    ]
    for finding in new:
        body = f"**{finding['title']}**\n\n{finding['detail']}\n\n_{label}: {finding['votes']} of {voters} voters agreed._"
        inline = gh(
            "api", f"repos/{repo}/pulls/{pr}/comments",
            "-f", f"body={body}", "-f", f"commit_id={env['HEAD_SHA']}",
            "-f", f"path={finding['path']}", "-F", f"line={finding['line']}", "-f", "side=RIGHT",
        )
        # A line outside the diff cannot take an inline comment.
        if not inline:
            gh("pr", "comment", pr, "--repo", repo, "--body", f"`{finding['path']}:{finding['line']}`\n\n{body}")
    if found:
        summary = (
            f"**{label}, round {env['ROUND']}:** {len(found)} problem(s) still open, {len(new)} new. "
            f"{candidate_count} candidate(s) from the finders; kept when at least {env['VOTES_NEEDED']} of {voters} voters agreed."
        )
    else:
        summary = f"**{label}, round {env['ROUND']}:** nothing found ({candidate_count} candidate(s), none kept by the voters)."
    gh("pr", "comment", pr, "--repo", repo, "--body", summary)
    return len(found)


def main():
    command = sys.argv[1]
    if command == "merge":
        merged = merge(load_outputs(sys.argv[2], "findings"))
        Path(sys.argv[3]).write_text(json.dumps(merged, indent=2))
        print(f"{len(merged)} candidate(s)")
    elif command == "post":
        candidates = json.loads(Path(sys.argv[2]).read_text())
        voter_outputs = load_outputs(sys.argv[3], "votes") if Path(sys.argv[3]).exists() else []
        found = accepted(candidates, voter_outputs, int(os.environ["VOTES_NEEDED"]))
        open_count = post(found, len(candidates), os.environ)
        with open(os.environ.get("GITHUB_OUTPUT", os.devnull), "a") as out:
            out.write(f"findings={open_count}\n")
        print(f"findings={open_count}")
    else:
        sys.exit(f"unknown command: {command}")


if __name__ == "__main__":
    main()
