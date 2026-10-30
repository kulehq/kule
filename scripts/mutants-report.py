#!/usr/bin/env python3
"""Joins the results of cargo-mutants runs over the same mutants on several
platforms (ci.yml's `mutants-report` job): a mutant is caught where any
platform's tests caught it, and missed only where every platform that
could build it missed it, since code one platform compiles out is nothing
to its tests. Prints the score, writes it to $GITHUB_STEP_SUMMARY when set,
and exits 2 when a mutant was missed everywhere.

    scripts/mutants-report.py DIR    # DIR holds the shards' outcomes.json files
"""

import json
import os
import sys
from pathlib import Path


def mutants(directory):
    """By mutant, the summaries every shard gave it; the shards read."""
    seen = {}
    shards = 0
    for path in sorted(Path(directory).rglob("outcomes.json")):
        shards += 1
        with open(path, encoding="utf-8") as f:
            outcomes = json.load(f).get("outcomes", [])
        for outcome in outcomes:
            scenario = outcome.get("scenario")
            if not isinstance(scenario, dict) or "Mutant" not in scenario:
                continue  # the baseline
            mutant = scenario["Mutant"]
            key = (
                mutant.get("file", "?"),
                mutant.get("span", {}).get("start", {}).get("line", 0),
                mutant.get("span", {}).get("start", {}).get("column", 0),
                mutant.get("replacement", "?"),
            )
            seen.setdefault(key, []).append(outcome.get("summary", "?"))
    return seen, shards


def verdicts(seen):
    caught, missed, other = [], [], []
    for key, summaries in sorted(seen.items()):
        if "CaughtMutant" in summaries:
            caught.append(key)
        elif "MissedMutant" in summaries:
            missed.append(key)
        else:
            other.append((key, summaries))  # unviable or timed out everywhere
    return caught, missed, other


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 1
    seen, shards = mutants(argv[1])
    if shards == 0:
        print(f"no outcomes.json under {argv[1]}: the mutants jobs left no results", file=sys.stderr)
        return 1
    caught, missed, other = verdicts(seen)
    tested = len(caught) + len(missed)
    score = f"{100 * len(caught) // tested}%" if tested else "no mutants to test"
    lines = [
        f"Mutation score: {score} ({len(caught)} caught, {len(missed)} missed, "
        f"{len(other)} neither, of {len(seen)} in the change)",
    ]
    for file, line, column, replacement in missed:
        lines.append(f"missed: {file}:{line}:{column}: {replacement}")
    for (file, line, column, replacement), summaries in other:
        lines.append(f"neither: {file}:{line}:{column}: {replacement} ({', '.join(summaries)})")
    text = "\n".join(lines)
    print(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write("```\n" + text + "\n```\n")
    return 2 if missed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
