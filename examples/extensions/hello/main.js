// Runs in a hidden frame while Kule is open.
kule.commands.register("greet", async () => {
  const { greeting } = await kule.config.get();
  const tasks = await kule.tasks.list();
  await kule.ui.notify(greeting, `You have ${tasks.length} task${tasks.length === 1 ? "" : "s"}.`);
});
