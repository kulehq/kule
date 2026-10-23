//! Budgets and limits, each said with the scope it is set for: every task,
//! each task in a group, or one task. Every line names where a value comes
//! from, so a person always knows which of the three they changed.

use std::path::Path;

use anyhow::{Result, anyhow, bail};
use kule_engine::{BudgetScope, Group, Limits, Money, Task, TaskLimits};

/// A limit as a person sets it: `key=VALUE` in `unit`, kept in the engine's
/// units (`scale` of them to one of the person's).
struct Field {
    key: &'static str,
    name: &'static str,
    unit: &'static str,
    scale: f64,
}

const FIELDS: [Field; 6] = [
    Field {
        key: "cpu",
        name: "processor",
        unit: "cores",
        scale: 100.0,
    },
    Field {
        key: "memory",
        name: "memory",
        unit: "GB",
        scale: 1024.0,
    },
    Field {
        key: "disk",
        name: "disk",
        unit: "GB",
        scale: 1024.0,
    },
    Field {
        key: "run",
        name: "one run",
        unit: "min",
        scale: 1.0,
    },
    Field {
        key: "task",
        name: "all of a task's runs",
        unit: "h",
        scale: 60.0,
    },
    Field {
        key: "idle",
        name: "doing nothing",
        unit: "min",
        scale: 1.0,
    },
];

fn every_value(l: &Limits, key: &str) -> Option<u64> {
    match key {
        "cpu" => l.cpu_percent.map(u64::from),
        "memory" => l.memory_mb,
        "disk" => l.disk_mb,
        "run" => l.run_minutes,
        "task" => l.task_minutes,
        _ => l.idle_minutes,
    }
}

fn set_every_value(l: &mut Limits, key: &str, v: Option<u64>) -> Result<()> {
    match key {
        "cpu" => {
            l.cpu_percent = v
                .map(u32::try_from)
                .transpose()
                .map_err(|_| anyhow!("cpu: too many cores"))?;
        }
        "memory" => l.memory_mb = v,
        "disk" => l.disk_mb = v,
        "run" => l.run_minutes = v,
        "task" => l.task_minutes = v,
        _ => l.idle_minutes = v,
    }
    Ok(())
}

fn own_value(l: &TaskLimits, key: &str) -> Option<Option<u64>> {
    match key {
        "cpu" => l.cpu_percent.map(|v| v.map(u64::from)),
        "memory" => l.memory_mb,
        "disk" => l.disk_mb,
        "run" => l.run_minutes,
        "task" => l.task_minutes,
        _ => l.idle_minutes,
    }
}

fn set_own_value(l: &mut TaskLimits, key: &str, v: Option<Option<u64>>) -> Result<()> {
    match key {
        "cpu" => {
            l.cpu_percent = v
                .map(|v| v.map(u32::try_from).transpose())
                .transpose()
                .map_err(|_| anyhow!("cpu: too many cores"))?;
        }
        "memory" => l.memory_mb = v,
        "disk" => l.disk_mb = v,
        "run" => l.run_minutes = v,
        "task" => l.task_minutes = v,
        _ => l.idle_minutes = v,
    }
    Ok(())
}

fn amount_text(field: &Field, stored: Option<u64>) -> String {
    match stored {
        None => "no limit".to_owned(),
        Some(v) => {
            let v = v as f64 / field.scale;
            let shown = if v.fract() == 0.0 {
                format!("{v:.0}")
            } else {
                format!("{v:.2}").trim_end_matches('0').to_owned()
            };
            format!("{shown} {}", field.unit)
        }
    }
}

/// A limit that is on or off: `key=on|off`.
struct Switch {
    key: &'static str,
    name: &'static str,
    on: &'static str,
    off: &'static str,
}

const SWITCHES: [Switch; 2] = [
    Switch {
        key: "gpu",
        name: "GPU",
        on: "may be used",
        off: "off",
    },
    Switch {
        key: "servers",
        name: "servers (macOS)",
        on: "may listen (weaker isolation)",
        off: "do not listen",
    },
];

fn switch_text(switch: &Switch, on: bool) -> &'static str {
    if on { switch.on } else { switch.off }
}

fn every_switch(l: &Limits, key: &str) -> bool {
    if key == "gpu" { l.gpu } else { l.servers }
}

fn set_every_switch(l: &mut Limits, key: &str, on: bool) {
    if key == "gpu" {
        l.gpu = on;
    } else {
        l.servers = on;
    }
}

fn own_switch(l: &TaskLimits, key: &str) -> Option<bool> {
    if key == "gpu" { l.gpu } else { l.servers }
}

fn set_own_switch(l: &mut TaskLimits, key: &str, on: Option<bool>) {
    if key == "gpu" {
        l.gpu = on;
    } else {
        l.servers = on;
    }
}

/// What a person wrote for one limit, at a scope that may take the one
/// above it (`inherit`) or not.
enum Said {
    /// Back to the scope above's.
    Inherit,
    /// No limit.
    None,
    Value(u64),
    Switch(bool),
}

fn parse(setting: &str, inherits: bool) -> Result<(&'static str, Said)> {
    let (key, value) = setting
        .split_once('=')
        .ok_or_else(|| anyhow!("{setting}: write a limit as FIELD=VALUE, such as memory=4"))?;
    let keys = "cpu, memory, disk, gpu, servers, run, task or idle";
    if value == "inherit" {
        if !inherits {
            bail!(
                "{key}: `inherit` is for --group or --task; the limits for every task have nothing above them"
            );
        }
        let key = FIELDS
            .iter()
            .map(|f| f.key)
            .chain(SWITCHES.iter().map(|s| s.key))
            .find(|k| *k == key)
            .ok_or_else(|| anyhow!("{key}: a limit is {keys}"))?;
        return Ok((key, Said::Inherit));
    }
    if let Some(switch) = SWITCHES.iter().find(|s| s.key == key) {
        let on = match value {
            "on" => true,
            "off" => false,
            _ => bail!(
                "{key}: on, off{}",
                if inherits { " or inherit" } else { "" }
            ),
        };
        return Ok((switch.key, Said::Switch(on)));
    }
    let field = FIELDS
        .iter()
        .find(|f| f.key == key)
        .ok_or_else(|| anyhow!("{key}: a limit is {keys}"))?;
    if value == "none" {
        return Ok((field.key, Said::None));
    }
    let number: f64 = value
        .parse()
        .ok()
        .filter(|v: &f64| v.is_finite() && *v > 0.0)
        .ok_or_else(|| {
            anyhow!(
                "{key}={value}: a number of {} above 0, or none{}",
                field.unit,
                if inherits { " or inherit" } else { "" }
            )
        })?;
    Ok((
        field.key,
        Said::Value((number * field.scale).round() as u64),
    ))
}

/// Applies `settings` to the scope's own limits, field by field.
fn apply_own(own: &mut TaskLimits, settings: &[String]) -> Result<()> {
    for setting in settings {
        let (key, said) = parse(setting, true)?;
        let switch = SWITCHES.iter().any(|s| s.key == key);
        match said {
            Said::Inherit if switch => set_own_switch(own, key, None),
            Said::Switch(on) => set_own_switch(own, key, Some(on)),
            Said::Inherit => set_own_value(own, key, None)?,
            Said::None => set_own_value(own, key, Some(None))?,
            Said::Value(v) => set_own_value(own, key, Some(Some(v)))?,
        }
    }
    Ok(())
}

fn group_named(home: &Path, id: u64) -> Result<Group> {
    kule_engine::groups(home)?
        .into_iter()
        .find(|g| g.id == id)
        .ok_or_else(|| anyhow!("no group {id}"))
}

fn tasks_in(home: &Path, id: u64) -> Result<usize> {
    let mut n = 0;
    for info in Task::list(home)? {
        if Task::open(home, &info.id)?.group()? == Some(id) {
            n += 1;
        }
    }
    Ok(n)
}

fn tasks_word(n: usize) -> String {
    format!("{n} {}", if n == 1 { "task" } else { "tasks" })
}

/// `kule limits`: shows, or sets with `settings`, the limits for every
/// task, for each task in `group`, or for `task`.
pub fn limits_cmd(
    home: &Path,
    group: Option<u64>,
    task: Option<String>,
    settings: &[String],
) -> Result<()> {
    let every = kule_engine::limits(home)?;
    match (group, task) {
        (Some(_), Some(_)) => bail!("--group or --task, not both"),
        (None, None) => {
            let mut limits = every;
            for setting in settings {
                match parse(setting, false)? {
                    (key, Said::Switch(on)) => set_every_switch(&mut limits, key, on),
                    (key, Said::None) => set_every_value(&mut limits, key, None)?,
                    (key, Said::Value(v)) => set_every_value(&mut limits, key, Some(v))?,
                    _ => unreachable!("every task's limits take no inherit"),
                }
            }
            if !settings.is_empty() {
                kule_engine::set_limits(home, &limits)?;
            }
            println!("limits for EVERY TASK (where its group and the task set none of their own):");
            for f in &FIELDS {
                println!(
                    "  {:<22} {}",
                    f.name,
                    amount_text(f, every_value(&limits, f.key))
                );
            }
            for sw in &SWITCHES {
                println!(
                    "  {:<22} {}",
                    sw.name,
                    switch_text(sw, every_switch(&limits, sw.key))
                );
            }
        }
        (Some(id), None) => {
            let mut group = group_named(home, id)?;
            if !settings.is_empty() {
                let mut own = group.limits;
                apply_own(&mut own, settings)?;
                group = kule_engine::set_group_limits(home, id, &own)?;
            }
            println!(
                "limits for EACH TASK IN GROUP {id} {:?} ({}), where a task sets none of its own:",
                group.name,
                tasks_word(tasks_in(home, id)?)
            );
            let in_force = every.with(&group.limits);
            for f in &FIELDS {
                let from = if own_value(&group.limits, f.key).is_some() {
                    "the group's"
                } else {
                    "every task's"
                };
                println!(
                    "  {:<22} {} ({from})",
                    f.name,
                    amount_text(f, every_value(&in_force, f.key))
                );
            }
            for sw in &SWITCHES {
                let from = if own_switch(&group.limits, sw.key).is_some() {
                    "the group's"
                } else {
                    "every task's"
                };
                println!(
                    "  {:<22} {} ({from})",
                    sw.name,
                    switch_text(sw, every_switch(&in_force, sw.key))
                );
            }
        }
        (None, Some(id)) => {
            let task = Task::open(home, &id)?;
            if !settings.is_empty() {
                let mut own = task.limits()?;
                apply_own(&mut own, settings)?;
                task.set_limits(&own)?;
            }
            let group = task.group()?.map(|g| group_named(home, g)).transpose()?;
            let own = task.limits()?;
            let in_force = task.limits_in_force()?;
            match &group {
                Some(g) => println!(
                    "limits for TASK {} only (in group {} {:?}):",
                    task.id(),
                    g.id,
                    g.name
                ),
                None => println!("limits for TASK {} only (in no group):", task.id()),
            }
            let source = |own_set: bool, group_set: bool| match (&group, own_set, group_set) {
                (_, true, _) => "its own".to_owned(),
                (Some(g), false, true) => format!("group {:?}", g.name),
                _ => "every task's".to_owned(),
            };
            for f in &FIELDS {
                let group_set = group
                    .as_ref()
                    .is_some_and(|g| own_value(&g.limits, f.key).is_some());
                println!(
                    "  {:<22} {} ({})",
                    f.name,
                    amount_text(f, every_value(&in_force, f.key)),
                    source(own_value(&own, f.key).is_some(), group_set)
                );
            }
            for sw in &SWITCHES {
                let group_set = group
                    .as_ref()
                    .is_some_and(|g| own_switch(&g.limits, sw.key).is_some());
                println!(
                    "  {:<22} {} ({})",
                    sw.name,
                    switch_text(sw, every_switch(&in_force, sw.key)),
                    source(own_switch(&own, sw.key).is_some(), group_set)
                );
            }
        }
    }
    Ok(())
}

/// What is set aside for runs at work, or that did not end, said after
/// what was spent: ` (0.80 USD set aside for runs at work or that did not
/// end)`, or nothing.
pub fn held_words(held: &[Money]) -> String {
    if held.is_empty() {
        return String::new();
    }
    let held: Vec<String> = held.iter().map(money).collect();
    format!(
        " ({} set aside for runs at work or that did not end)",
        held.join(" + ")
    )
}

pub fn money(m: &Money) -> String {
    // An empty sum is -0.0, which reads as an odd minus.
    format!("{:.2} {}", m.amount + 0.0, m.currency)
}

/// An amount like 5 or 2.50 in `currency` (USD without one), or `off`.
pub fn parse_budget(amount: &str, currency: Option<String>) -> Result<Option<Money>> {
    if amount == "off" {
        return Ok(None);
    }
    Ok(Some(Money {
        amount: amount
            .parse()
            .map_err(|_| anyhow!("{amount}: an amount like 5 or 2.50, or off"))?,
        currency: currency.unwrap_or_else(|| "USD".to_owned()),
    }))
}

/// `kule group budget`: shows, or sets, what group `id`'s tasks may cost
/// together; `daily` picks the 24-hour budget over the one for all runs.
pub fn group_budget_cmd(
    home: &Path,
    id: u64,
    amount: Option<String>,
    currency: Option<String>,
    daily: bool,
) -> Result<()> {
    let mut group = group_named(home, id)?;
    if let Some(amount) = amount {
        let budget = parse_budget(&amount, currency)?;
        let (total, day) = if daily {
            (group.budget.clone(), budget)
        } else {
            (budget, group.daily_budget.clone())
        };
        group = kule_engine::set_group_budget(home, id, total, day)?;
    }
    let spent = kule_engine::groups_spent(home)?
        .remove(&id)
        .unwrap_or_default();
    println!(
        "budgets for GROUP {id} {:?}: its {} together, beside each task's own and the daily one for every task:",
        group.name,
        tasks_word(tasks_in(home, id)?)
    );
    let line = |what: &str, budget: &Option<Money>, spent: &kule_engine::Spent| {
        let used = |currency: &str| Money {
            amount: spent.in_currency(currency),
            currency: currency.to_owned(),
        };
        match budget {
            Some(b) => {
                let used = used(&b.currency);
                let left = Money {
                    amount: (b.amount - spent.counted_in(&b.currency)).max(0.0),
                    currency: b.currency.clone(),
                };
                println!(
                    "  {what:<16} {}, {} spent, {} left{}",
                    money(b),
                    money(&used),
                    money(&left),
                    held_words(&spent.held)
                );
            }
            None => {
                let cost = if spent.cost.is_empty() {
                    "nothing reported".to_owned()
                } else {
                    spent.cost.iter().map(money).collect::<Vec<_>>().join(" + ")
                };
                println!("  {what:<16} no budget ({cost} spent)");
            }
        }
    };
    line("all their runs", &group.budget, &spent.total);
    line("any 24 hours", &group.daily_budget, &spent.today);
    Ok(())
}

/// Every budget over `task`'s next run, each named, and the one that holds.
pub fn print_task_budgets(task: &Task) -> Result<()> {
    let budgets = task.budgets()?;
    if budgets.is_empty() {
        println!("no budget holds task {}", task.id());
        return Ok(());
    }
    println!("budgets over task {}'s next run:", task.id());
    for b in &budgets {
        let what = match &b.scope {
            BudgetScope::Task => "the task's own".to_owned(),
            BudgetScope::Group { name, .. } => format!("group {name:?}, all its runs"),
            BudgetScope::GroupDaily { name, .. } => format!("group {name:?}, any 24 hours"),
            BudgetScope::Daily => "every task, any 24 hours".to_owned(),
        };
        let left = Money {
            amount: b.left.amount.max(0.0),
            currency: b.left.currency.clone(),
        };
        let held = (b.held > 0.0).then(|| Money {
            amount: b.held,
            currency: b.budget.currency.clone(),
        });
        println!(
            "  {what:<32} {}, {} left{}",
            money(&b.budget),
            money(&left),
            held_words(held.as_slice())
        );
    }
    match task.run_budget() {
        Ok(Some(limit)) => println!("the next run may cost at most {}", money(&limit)),
        Ok(None) => {}
        Err(e) => println!("no run starts: {e}"),
    }
    Ok(())
}

/// A group's budgets in a few words, for `kule group`.
pub fn group_budget_words(group: &Group) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(b) = &group.budget {
        parts.push(format!("budget {}", money(b)));
    }
    if let Some(b) = &group.daily_budget {
        parts.push(format!("{} a day", money(b)));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// `kule group put`: moves the task, then says what changed of the limits
/// and group budgets over its runs, so none loosens unseen.
pub fn put_in_group(home: &Path, id: &str, group: Option<u64>) -> Result<()> {
    let task = Task::open(home, id)?;
    let (limits, budgets) = (task.limits_in_force()?, task.budgets()?);
    task.set_group(group)?;
    let (after, after_budgets) = (task.limits_in_force()?, task.budgets()?);
    match group {
        Some(g) => println!("task {id} is in group {g} {:?}", group_named(home, g)?.name),
        None => println!("task {id} is in no group"),
    }
    let mut changed = Vec::new();
    for f in &FIELDS {
        let (a, b) = (every_value(&limits, f.key), every_value(&after, f.key));
        if a != b {
            changed.push(format!(
                "{} {} → {}",
                f.name,
                amount_text(f, a),
                amount_text(f, b)
            ));
        }
    }
    for sw in &SWITCHES {
        let (a, b) = (every_switch(&limits, sw.key), every_switch(&after, sw.key));
        if a != b {
            changed.push(format!(
                "{} {} → {}",
                sw.name,
                switch_text(sw, a),
                switch_text(sw, b)
            ));
        }
    }
    if !changed.is_empty() {
        println!("  its limits: {}", changed.join(", "));
    }
    let group_budgets = |list: &[kule_engine::BudgetLeft]| {
        list.iter()
            .filter_map(|b| match &b.scope {
                BudgetScope::Group { name, .. } => {
                    Some(format!("{} in all ({name:?})", money(&b.budget)))
                }
                BudgetScope::GroupDaily { name, .. } => {
                    Some(format!("{} a day ({name:?})", money(&b.budget)))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (before, now) = (group_budgets(&budgets), group_budgets(&after_budgets));
    if before != now {
        match (before.is_empty(), now.is_empty()) {
            (_, false) => println!("  its group's budgets: {now}"),
            (false, true) => println!("  no group budget holds it any more (was {before})"),
            (true, true) => {}
        }
    }
    Ok(())
}
