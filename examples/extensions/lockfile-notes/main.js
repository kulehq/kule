// A review helper: it reads a task's changes and adds notes to its review.
// It cannot approve, refuse or commit anything; the notes are for you.
const LOCKFILES = new Set([
  "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "bun.lockb",
  "Cargo.lock", "poetry.lock", "uv.lock", "Pipfile.lock",
  "Gemfile.lock", "composer.lock", "go.sum", "packages.lock.json",
]);

kule.events.on("review", async ({ task }) => {
  const review = await kule.review.get(task);
  const notes = review.effects.effects
    .filter((e) => LOCKFILES.has(e.path.split("/").pop()))
    .map((e) => ({
      path: e.path,
      level: "warn",
      text: `Lockfile ${e.change}: look at the packages it adds or moves before you commit.`,
    }));
  // An empty list takes this extension's notes off the review.
  await kule.review.annotate(task, notes);
});
