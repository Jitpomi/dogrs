"""Select capacity runs without treating unrelated workspace lock edits as queue changes.

Inspect both lock graphs: removals and dependency rewrites must trigger too.
Ambiguous or invalid input fails the scope job instead of silently skipping tests.
"""
import argparse
import json
import os
import subprocess
import tomllib
from pathlib import Path

ROOTS = {"dog-queue", "hosted-system"}


def closure(text):
    packages = tomllib.loads(text)["package"]
    selected = {}
    pending = [p for p in packages if p["name"] in ROOTS]
    if {p["name"] for p in pending} != ROOTS:
        raise ValueError("capacity roots missing from Cargo.lock")
    while pending:
        package = pending.pop()
        key = (package["name"], package["version"], package.get("source", ""))
        if key in selected:
            continue
        selected[key] = package
        for dependency in package.get("dependencies", []):
            parts = dependency.split(" ", 2)
            matches = [p for p in packages if p["name"] == parts[0]
                       and (len(parts) < 2 or p["version"] == parts[1])
                       and (len(parts) < 3 or p.get("source", "") == parts[2].strip("()"))]
            if len(matches) != 1:
                raise ValueError(f"unresolved or ambiguous lock dependency: {dependency}")
            pending.extend(matches)
    return selected


def needs_capacity(paths, before, after):
    if any(path.startswith(("dog-queue/", "dog-examples/hosted-system/", ".github/scripts/"))
           or path in {".github/workflows/provider-capacity.yml", "Cargo.toml"}
           for path in paths):
        return True
    return "Cargo.lock" in paths and closure(before) != closure(after)


def git(*args):
    return subprocess.check_output(["git", *args]).decode()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("base")
    parser.add_argument("head")
    args = parser.parse_args()
    # GitHub PR paths describe changes since the merge base, not unrelated
    # commits added to the target branch after the PR was opened.
    base = git("merge-base", args.base, args.head).strip()
    paths = git("diff", "--name-only", "-z", base, args.head).split("\0")
    result = needs_capacity(paths, git("show", f"{base}:Cargo.lock"),
                            git("show", f"{args.head}:Cargo.lock"))
    reason = "Queue, fixture, workflow or dependency changes" if result else "No capacity workload dependency changes"
    print(json.dumps({"required": result, "reason": reason}))
    if "GITHUB_OUTPUT" in os.environ:
        with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
            output.write(f"required={str(result).lower()}\n")
    if "GITHUB_STEP_SUMMARY" in os.environ:
        with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a") as summary:
            summary.write(f"## Capacity scope\n\n{reason}.\n\nA skipped workload is not a capacity pass. Existing capacity failures remain unresolved.\n")


if __name__ == "__main__":
    main()
