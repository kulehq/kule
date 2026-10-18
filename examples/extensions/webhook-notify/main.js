// Watches what happens to tasks and posts it to a Slack webhook. Kule lets
// it reach hooks.slack.com only (the manifest's hosts), over https.
const WORDS = { created: "was created", committed: "was committed", rolled_back: "was rolled back", undone: "had its last commit undone", removed: "was removed" };

async function post(text) {
  const { webhook } = await kule.config.get();
  if (!webhook) return;
  const answer = await kule.net.fetch(webhook, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ text }),
  });
  if (answer.status >= 300) console.error(`the webhook answered ${answer.status}`);
}

kule.events.on("task", async ({ task, change }) => {
  const { on } = await kule.config.get();
  if (on === "commits" && change !== "committed") return;
  let name = task;
  try {
    const t = await kule.tasks.get(task);
    name = t.label || t.workspace.split("/").pop();
  } catch {
    // A removed task is no longer listed.
  }
  await post(`Kule: ${name} ${WORDS[change] || change}`);
});

kule.commands.register("test", () => post("Kule: a test message from Webhook notify"));
