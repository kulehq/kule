#!/usr/bin/env python3
"""Joins the results of cargo-mutants runs over the same mutants on several
platforms (ci.yml's `mutants-report` job). Code one platform compiles out
is nothing to its tests: there the mutant is reported as missed without
having been built. So a mutant is caught where any platform's tests caught
it; missed only where every platform missed it; and neither where no
platform caught it but one found it unviable (it did not build where the
code compiles) or timed out. Prints the score, writes it to
$GITHUB_STEP_SUMMARY when set, and exits 2 when a mutant was missed
everywhere.

    scripts/mutants-report.py DIR    # DIR holds the shards' outcomes.json files
    scripts/mutants-report.py --self-test
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
        elif all(s == "MissedMutant" for s in summaries):
            missed.append(key)
        else:
            # Unviable or timed out where the code compiles; a "missed"
            # beside that came from a platform that never built it.
            other.append((key, summaries))
    return caught, missed, other


def self_test():
    """The join's rules, each on the case that needs it."""
    key = lambda n: ("f.rs", n, 1, "()")  # noqa: E731
    seen = {
        key(1): ["CaughtMutant", "MissedMutant"],  # caught on one platform
        key(2): ["MissedMutant", "MissedMutant"],  # missed everywhere
        key(3): ["Unviable", "MissedMutant"],  # compiled nowhere it was tested
        key(4): ["Timeout", "MissedMutant"],
        key(5): ["MissedMutant"],  # one shard only
    }
    caught, missed, other = verdicts(seen)
    assert caught == [key(1)], caught
    assert missed == [key(2), key(5)], missed
    assert [k for k, _ in other] == [key(3), key(4)], other
    print("self-test ok")
    return 0


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 1
    if argv[1] == "--self-test":
        return self_test()
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
