// A panel: Kule's colours arrive as --kule-* CSS variables.
async function render() {
  const [{ greeting }, tasks] = await Promise.all([kule.config.get(), kule.tasks.list()]);
  document.getElementById("greeting").textContent = greeting;
  const list = document.getElementById("tasks");
  list.replaceChildren(
    ...tasks.map((t) => {
      const li = document.createElement("li");
      li.textContent = `${t.label || t.id} · ${t.changes} change${t.changes === 1 ? "" : "s"}`;
      if (t.id === kule.context.task) li.className = "current";
      return li;
    })
  );
}

document.getElementById("greet").addEventListener("click", () => kule.commands.execute("ext.kule.hello.greet"));
kule.ready.then(render);
kule.events.on("task-selected", ({ task }) => {
  kule.context.task = task;
  render();
});
kule.events.on("config", render);
