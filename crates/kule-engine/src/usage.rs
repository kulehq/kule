//! What a task's runs used, as their agents reported it: tokens, and what
//! it cost when an agent says (Claude Code does; one signed in with a
//! subscription may report tokens alone). One line per run in the task's
//! `usage.jsonl`, outside the copy, where nothing the task runs can reach.

use std::io::Write;

use serde::{Deserialize, Serialize};

use std::path::Path;

use crate::error::{Error, IoContext, Result};
use crate::fsops::{read_json, write_json};
use crate::task::Task;

/// An amount in an ISO 4217 currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Money {
    pub amount: f64,
    pub currency: String,
}

impl Money {
    /// Refused unless a positive amount in an ISO 4217 currency.
    pub(crate) fn check_budget(&self) -> Result<()> {
        let currency_ok =
            self.currency.len() == 3 && self.currency.chars().all(|c| c.is_ascii_uppercase());
        if !(self.amount.is_finite() && self.amount > 0.0 && currency_ok) {
            return Err(Error::Invalid(format!(
                "a budget is a positive amount in a currency like USD, not {} {}",
                self.amount, self.currency
            )));
        }
        Ok(())
    }

    /// The tighter of two limits; one in another currency than the first
    /// is not compared, and the first stands.
    pub fn tighter(a: Option<Money>, b: Option<Money>) -> Option<Money> {
        match (a, b) {
            (Some(a), Some(b)) if a.currency == b.currency && b.amount < a.amount => Some(b),
            (Some(a), _) => Some(a),
            (None, b) => b,
        }
    }
}

/// What one run used.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunUsage {
    pub agent: String,
    pub started_unix: u64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub thought_tokens: u64,
    #[serde(default)]
    pub cached_read_tokens: u64,
    #[serde(default)]
    pub cached_write_tokens: u64,
    /// The context window at the end, used of size.
    #[serde(default)]
    pub context_used: u64,
    #[serde(default)]
    pub context_size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Money>,
    /// How long the run took, start to end.
    #[serde(default)]
    pub duration_secs: u64,
    /// The group the task was in when the run was recorded, `Some(None)`
    /// for none, so a group's budget keeps what its tasks spent after one
    /// moves out. Absent in runs recorded before Kule kept it: those count
    /// for the group the task is in now.
    #[serde(
        default,
        deserialize_with = "crate::limits::set",
        skip_serializing_if = "Option::is_none"
    )]
    pub group: Option<Option<u64>>,
    /// The share set aside for this run before it started (see
    /// [`Task::reserve_run`]), which this record closes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reservation: Option<String>,
    /// Not a run: the person let go of the share a run that did not end
    /// still held (see [`Task::release_unfinished`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub released: bool,
}

/// What a run may spend, set aside before it starts, until the run's
/// usage is recorded (see [`Task::reserve_run`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Reservation {
    id: String,
    started_unix: u64,
    /// The group the task was in then; none for none.
    #[serde(default)]
    group: Option<u64>,
    amount: Money,
}

/// A run's share, set aside: the most it may spend, and what its usage
/// record names to close it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reserved {
    pub id: String,
    pub limit: Money,
}

/// What a task's records say: its runs (what the person let go of is
/// none), how many records cannot be read, and the shares still set aside
/// for runs whose usage has not come.
struct Ledger {
    runs: Vec<RunUsage>,
    unreadable: usize,
    open: Vec<Reservation>,
}

/// What a task's runs used together.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spent {
    pub runs: usize,
    /// Every token the agents' models took in or gave out, reads from and
    /// writes to their prompt caches included: an agent may send only a few
    /// new input tokens a turn and the rest from its cache.
    pub tokens: u64,
    /// What they cost, one sum per currency; runs that did not say are not
    /// in it.
    pub cost: Vec<Money>,
    /// Runs that reported no cost.
    pub unpriced_runs: usize,
    /// Records of runs that cannot be read: what they spent is not known,
    /// so no budget they count for holds a run (see [`Task::run_budget`]).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unreadable: usize,
    /// Set aside for runs at work, or that did not end, one sum per
    /// currency: counted as spent until each run's usage comes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held: Vec<Money>,
    /// The tasks those runs are in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held_by: Vec<String>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Spent {
    fn add(&mut self, run: RunUsage) {
        self.runs += 1;
        self.tokens += run.input_tokens
            + run.output_tokens
            + run.thought_tokens
            + run.cached_read_tokens
            + run.cached_write_tokens;
        match run.cost {
            Some(cost) => match self.cost.iter_mut().find(|m| m.currency == cost.currency) {
                Some(sum) => sum.amount += cost.amount,
                None => self.cost.push(cost),
            },
            None => self.unpriced_runs += 1,
        }
    }

    fn hold(&mut self, share: &Reservation, task: &str) {
        match self
            .held
            .iter_mut()
            .find(|m| m.currency == share.amount.currency)
        {
            Some(sum) => sum.amount += share.amount.amount,
            None => self.held.push(share.amount.clone()),
        }
        if !self.held_by.iter().any(|t| t == task) {
            self.held_by.push(task.to_owned());
        }
    }

    /// What was spent in `currency`.
    pub fn in_currency(&self, currency: &str) -> f64 {
        self.cost
            .iter()
            .filter(|m| m.currency == currency)
            .map(|m| m.amount)
            .sum()
    }

    /// What counts against a budget in `currency`: what was spent, and
    /// what is set aside for runs whose usage has not come.
    pub fn counted_in(&self, currency: &str) -> f64 {
        self.in_currency(currency) + self.held_in(currency)
    }

    /// What is set aside in `currency`.
    pub fn held_in(&self, currency: &str) -> f64 {
        self.held
            .iter()
            .filter(|m| m.currency == currency)
            .map(|m| m.amount)
            .sum()
    }
}

impl Task {
    fn usage_path(&self) -> std::path::PathBuf {
        self.dir().join("usage.jsonl")
    }

    fn reserved_path(&self) -> std::path::PathBuf {
        self.dir().join("reserved.jsonl")
    }

    /// Adds one run's usage to the task's, with the group the task is in
    /// now unless the usage names one; it closes the share set aside for
    /// the run, if it names one.
    pub fn record_usage(&self, usage: &RunUsage) -> Result<()> {
        let mut usage = usage.clone();
        if usage.group.is_none() {
            usage.group = Some(self.group()?);
        }
        let _lock = budget_lock(&self.home())?;
        append_record(&self.usage_path(), &usage)
    }

    /// Before a run starts: sets aside what it may spend, the tightest of
    /// [`Task::budgets`] with what other runs set aside counted as spent,
    /// so runs started at once under one budget share what is left, and a
    /// run whose usage never comes counts at its whole share. `None`
    /// without a budget; refused, naming the budget, when nothing is
    /// left, and when the share cannot be set aside (the run does not
    /// start). Across Kule's processes, one run at a time is set aside.
    pub fn reserve_run(&self) -> Result<Option<Reserved>> {
        let _lock = budget_lock(&self.home())?;
        let Some(limit) = self.run_budget()? else {
            return Ok(None);
        };
        let share = Reservation {
            id: random_id()?,
            started_unix: now_unix(),
            group: self.group()?,
            amount: limit.clone(),
        };
        append_record(&self.reserved_path(), &share).map_err(|e| {
            Error::Invalid(format!(
                "Kule could not set aside what the run may spend ({e}), so it does not start"
            ))
        })?;
        Ok(Some(Reserved {
            id: share.id,
            limit,
        }))
    }

    /// Lets go of the shares runs that did not end still hold in this task,
    /// as the person asked, knowingly: each is closed by a record that says
    /// it was let go, not spent. Refused while an agent is at work in the
    /// task. Returns what was let go.
    pub fn release_unfinished(&self) -> Result<Vec<Money>> {
        let _lock = budget_lock(&self.home())?;
        if let Some(pid) = self.agent_pid() {
            return Err(Error::Invalid(format!(
                "an agent is at work in task {} (process {pid}); its run holds its share until it ends",
                self.id()
            )));
        }
        let mut released = Vec::new();
        for share in self.ledger()?.open {
            append_record(
                &self.usage_path(),
                &RunUsage {
                    started_unix: share.started_unix,
                    group: Some(share.group),
                    reservation: Some(share.id),
                    released: true,
                    ..RunUsage::default()
                },
            )?;
            released.push(share.amount);
        }
        Ok(released)
    }

    /// Every run's usage, oldest first; a line that cannot be read is left
    /// out (and counted by [`Task::spent`]).
    pub fn usage(&self) -> Result<Vec<RunUsage>> {
        Ok(self.ledger()?.runs)
    }

    /// What the task's records say (see [`Ledger`]).
    fn ledger(&self) -> Result<Ledger> {
        let (records, mut unreadable) = read_records::<RunUsage>(&self.usage_path())?;
        let closed: std::collections::BTreeSet<String> = records
            .iter()
            .filter_map(|r| r.reservation.clone())
            .collect();
        let runs = records.into_iter().filter(|r| !r.released).collect();
        let (shares, unreadable_shares) = read_records::<Reservation>(&self.reserved_path())?;
        unreadable += unreadable_shares;
        let open = shares
            .into_iter()
            .filter(|s| !closed.contains(&s.id))
            .collect();
        Ok(Ledger {
            runs,
            unreadable,
            open,
        })
    }

    /// What the task's runs used together, with what is set aside for
    /// those whose usage has not come.
    pub fn spent(&self) -> Result<Spent> {
        let ledger = self.ledger()?;
        let mut spent = Spent {
            unreadable: ledger.unreadable,
            ..Spent::default()
        };
        for run in ledger.runs {
            spent.add(run);
        }
        for share in &ledger.open {
            spent.hold(share, self.id());
        }
        Ok(spent)
    }

    /// What is left of the budget for the next run: `None` without a
    /// budget, and an amount of zero or less when it is spent.
    pub fn budget_left(&self) -> Result<Option<Money>> {
        let Some(budget) = self.budget()? else {
            return Ok(None);
        };
        let spent = self.spent()?.counted_in(&budget.currency);
        Ok(Some(Money {
            amount: budget.amount - spent,
            currency: budget.currency,
        }))
    }

    /// Every budget the task's next run is held to, with what is left of
    /// each: its own, its group's (all its runs, and the last 24 hours') and
    /// the daily one for every task. Each holds; the tightest decides.
    pub fn budgets(&self) -> Result<Vec<BudgetLeft>> {
        self.budgets_in(self.group()?)
    }

    /// As [`Task::budgets`], were the task in group `group`.
    pub fn budgets_in(&self, group: Option<u64>) -> Result<Vec<BudgetLeft>> {
        let home = self.home();
        let mut out = Vec::new();
        if let Some(budget) = self.budget()? {
            let spent = self.spent()?;
            out.push(BudgetLeft::new(BudgetScope::Task, budget, &spent));
        }
        if let Some(group) = crate::groups::group_of(&home, group)?
            && (group.budget.is_some() || group.daily_budget.is_some())
        {
            let spent = groups_spent(&home)?.remove(&group.id).unwrap_or_default();
            if let Some(budget) = group.budget {
                let scope = BudgetScope::Group {
                    group: group.id,
                    name: group.name.clone(),
                };
                out.push(BudgetLeft::new(scope, budget, &spent.total));
            }
            if let Some(budget) = group.daily_budget {
                let scope = BudgetScope::GroupDaily {
                    group: group.id,
                    name: group.name,
                };
                out.push(BudgetLeft::new(scope, budget, &spent.today));
            }
        }
        if let Some(budget) = daily_budget(&home)? {
            out.push(BudgetLeft::new(
                BudgetScope::Daily,
                budget,
                &spent_today(&home)?,
            ));
        }
        Ok(out)
    }

    /// What the task's next run may cost: the tightest of [`Task::budgets`],
    /// `None` without any. Refused, naming the budget, when one is spent.
    pub fn run_budget(&self) -> Result<Option<Money>> {
        let mut limit = None;
        for budget in self.budgets()? {
            // What it spent is not known: the check cannot decide, and
            // refuses.
            if budget.unreadable > 0 {
                return Err(Error::Invalid(budget.unreadable_reason(self.id())));
            }
            if budget.left.amount <= 0.0 && budget.held > 0.0 {
                return Err(Error::Invalid(budget.held_reason(self.id())));
            }
            if budget.left.amount <= 0.0 {
                return Err(Error::Invalid(budget.spent_reason(self.id())));
            }
            limit = Money::tighter(limit, Some(budget.left));
        }
        Ok(limit)
    }
}

/// Where a budget over a run is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum BudgetScope {
    /// The task's own: all of its runs together.
    Task,
    /// Its group's: all of the group's tasks' runs together.
    Group { group: u64, name: String },
    /// Its group's for a day: the group's tasks' runs started in the last
    /// 24 hours.
    GroupDaily { group: u64, name: String },
    /// Every task's runs started in the last 24 hours.
    Daily,
}

/// A budget over a run, and what is left of it: zero or less when spent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetLeft {
    #[serde(flatten)]
    pub scope: BudgetScope,
    pub budget: Money,
    pub left: Money,
    /// Records it counts that cannot be read: it holds no run while there
    /// are any.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unreadable: usize,
    /// Of what it counts, what is set aside for runs at work, or that did
    /// not end, in its currency (see [`Task::reserve_run`]).
    #[serde(default, skip_serializing_if = "is_none_held")]
    pub held: f64,
    /// The tasks those runs are in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held_by: Vec<String>,
}

fn is_none_held(held: &f64) -> bool {
    *held == 0.0
}

impl BudgetLeft {
    fn new(scope: BudgetScope, budget: Money, spent: &Spent) -> BudgetLeft {
        let left = Money {
            amount: budget.amount - spent.counted_in(&budget.currency),
            currency: budget.currency.clone(),
        };
        let held = spent.held_in(&budget.currency);
        BudgetLeft {
            scope,
            held_by: if held > 0.0 {
                spent.held_by.clone()
            } else {
                Vec::new()
            },
            held,
            budget,
            left,
            unreadable: spent.unreadable,
        }
    }

    /// The budget's name in a sentence about task `task`.
    fn whose(&self, task: &str) -> String {
        match &self.scope {
            BudgetScope::Task => format!("task {task}'s budget"),
            BudgetScope::Group { name, .. } => format!("group {name:?}'s budget"),
            BudgetScope::GroupDaily { name, .. } => format!("group {name:?}'s daily budget"),
            BudgetScope::Daily => "the daily budget".to_owned(),
        }
    }

    /// Why task `task` cannot run while what is left of this budget is set
    /// aside for runs whose usage has not come.
    fn held_reason(&self, task: &str) -> String {
        let tasks = match self.held_by.as_slice() {
            [one] => format!("task {one}"),
            many => format!("tasks {}", many.join(", ")),
        };
        format!(
            "what is left of {} ({:.2} {}) is set aside for runs at work, or that did not end, in {tasks}: \
             once a run ends, what it spent counts instead; `kule budget TASK --release` lets go of one that did not end",
            self.whose(task),
            self.held,
            self.budget.currency
        )
    }

    /// Why task `task` cannot run while records this budget counts cannot
    /// be read.
    fn unreadable_reason(&self, task: &str) -> String {
        let whose = self.whose(task);
        let records = match self.unreadable {
            1 => "1 record of what runs spent (a task's usage.jsonl) cannot be read".to_owned(),
            n => format!("{n} records of what runs spent (in tasks' usage.jsonl) cannot be read"),
        };
        format!(
            "Kule cannot tell what {whose} has left: {records}; mend or remove them, or turn the budget off, to run again"
        )
    }

    /// Why task `task` cannot run while this budget is spent.
    fn spent_reason(&self, task: &str) -> String {
        match &self.scope {
            BudgetScope::Task => format!(
                "task {task} has spent its budget; raise it or turn it off to run again"
            ),
            BudgetScope::Group { name, .. } => format!(
                "group {name:?} has spent its budget: its tasks together; raise it or turn it off to run again"
            ),
            BudgetScope::GroupDaily { name, .. } => format!(
                "group {name:?}'s daily budget is spent: its tasks together, in the last 24 hours; raise it or turn it off to run again"
            ),
            BudgetScope::Daily => "the daily budget is spent: every task together, in the last 24 hours; raise it or turn it off to run again".to_owned(),
        }
    }
}

/// What a group's tasks spent: all of their runs, and those started in the
/// last 24 hours.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupSpent {
    pub total: Spent,
    pub today: Spent,
}

/// What each group's tasks spent, by group id. A run counts for the group
/// its task was in when it was recorded, or, recorded before Kule kept
/// that, for the group the task is in now.
pub fn groups_spent(home: &Path) -> Result<std::collections::BTreeMap<u64, GroupSpent>> {
    let since = now_unix().saturating_sub(DAY_SECS);
    let mut spent = std::collections::BTreeMap::<u64, GroupSpent>::new();
    for info in Task::list(home)? {
        let task = Task::open(home, &info.id)?;
        let now_in = task.group()?;
        let Ledger {
            runs,
            unreadable,
            open,
        } = task.ledger()?;
        // Not known when, nor for which group: it counts for the group the
        // task is in now, today too.
        if let Some(group) = now_in
            && unreadable > 0
        {
            let entry = spent.entry(group).or_default();
            entry.total.unreadable += unreadable;
            entry.today.unreadable += unreadable;
        }
        // A share counts for the group the task was in when it was set
        // aside.
        for share in &open {
            let Some(group) = share.group else {
                continue;
            };
            let entry = spent.entry(group).or_default();
            if share.started_unix >= since {
                entry.today.hold(share, task.id());
            }
            entry.total.hold(share, task.id());
        }
        for run in runs {
            let Some(group) = run.group.unwrap_or(now_in) else {
                continue;
            };
            let entry = spent.entry(group).or_default();
            if run.started_unix >= since {
                entry.today.add(run.clone());
            }
            entry.total.add(run);
        }
    }
    Ok(spent)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn usd(amount: f64) -> Option<Money> {
        Some(Money {
            amount,
            currency: "USD".into(),
        })
    }

    #[test]
    fn runs_add_up_and_the_budget_shrinks() {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let task = Task::create(home.path(), ws.path()).unwrap();
        assert_eq!(task.spent().unwrap(), Spent::default());
        assert_eq!(task.budget_left().unwrap(), None);

        task.set_budget(usd(2.0)).unwrap();
        let run = |input, cost| RunUsage {
            agent: "claude-code".into(),
            input_tokens: input,
            output_tokens: 10,
            cached_read_tokens: 1000,
            cached_write_tokens: 5,
            cost,
            ..RunUsage::default()
        };
        task.record_usage(&run(100, usd(0.5))).unwrap();
        task.record_usage(&run(200, None)).unwrap();
        task.record_usage(&run(300, usd(0.75))).unwrap();
        let spent = task.spent().unwrap();
        assert_eq!(
            (spent.runs, spent.tokens, spent.unpriced_runs),
            (3, 3645, 1)
        );
        assert_eq!(spent.cost, usd(1.25).into_iter().collect::<Vec<_>>());
        assert_eq!(task.budget_left().unwrap(), usd(0.75));

        // The approval timeout and the budget are kept apart.
        task.set_approval_timeout(std::time::Duration::from_secs(120))
            .unwrap();
        assert_eq!(task.budget().unwrap(), usd(2.0));
        assert_eq!(task.approval_timeout().unwrap().as_secs(), 120);
        task.set_budget(None).unwrap();
        assert_eq!(task.approval_timeout().unwrap().as_secs(), 120);
    }

    #[test]
    fn a_run_is_kept_with_its_group_and_older_runs_count_for_the_group_now() {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let task = Task::create(home.path(), ws.path()).unwrap();
        let group = crate::groups::create_group(home.path(), "g", None).unwrap();
        // Recorded before Kule kept a run's group: no `group` in the line.
        std::fs::write(
            task.usage_path(),
            "{\"agent\":\"a\",\"started_unix\":1,\"cost\":{\"amount\":1.0,\"currency\":\"USD\"}}\n",
        )
        .unwrap();
        task.record_usage(&RunUsage {
            agent: "a".into(),
            cost: usd(2.0),
            ..RunUsage::default()
        })
        .unwrap();
        task.set_group(Some(group.id)).unwrap();
        task.record_usage(&RunUsage {
            agent: "a".into(),
            cost: usd(4.0),
            ..RunUsage::default()
        })
        .unwrap();
        let groups = |kept: &[Option<Option<u64>>]| {
            assert_eq!(
                task.usage()
                    .unwrap()
                    .iter()
                    .map(|r| r.group)
                    .collect::<Vec<_>>(),
                kept
            );
        };
        groups(&[None, Some(None), Some(Some(group.id))]);
        // The old run counts for the group the task is in now; the one
        // recorded in no group does not.
        let spent = groups_spent(home.path()).unwrap();
        assert!((spent[&group.id].total.in_currency("USD") - 5.0).abs() < 1e-9);
    }

    /// A record of a run that cannot be read is counted, not taken for
    /// nothing spent: every budget it could count for holds the next run,
    /// until it is mended or the budget turned off. A record cut short
    /// does not take the next one with it.
    #[test]
    fn an_unreadable_usage_record_holds_every_budget_it_counts_for() {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let task = Task::create(home.path(), ws.path()).unwrap();
        let other = Task::create(home.path(), ws.path()).unwrap();
        let run = RunUsage {
            agent: "a".into(),
            started_unix: now_unix(),
            cost: usd(0.5),
            ..RunUsage::default()
        };
        task.record_usage(&run).unwrap();
        // Cut short as it was written: no line end.
        std::fs::OpenOptions::new()
            .append(true)
            .open(task.usage_path())
            .and_then(|mut f| f.write_all(b"{\"agent\":\"a\",\"cost\":{\"amo"))
            .unwrap();
        task.record_usage(&run).unwrap();
        let spent = task.spent().unwrap();
        assert_eq!((spent.runs, spent.unreadable), (2, 1));

        // No budget: nothing to hold.
        assert_eq!(task.run_budget().unwrap(), None);
        task.set_budget(usd(10.0)).unwrap();
        let e = task.run_budget().unwrap_err().to_string();
        assert!(e.contains("cannot be read"), "{e}");
        assert!(e.contains("turn the budget off"), "{e}");
        task.set_budget(None).unwrap();
        assert_eq!(task.run_budget().unwrap(), None);

        // The daily budget counts every task's runs: it holds the others too.
        set_daily_budget(home.path(), usd(10.0)).unwrap();
        let e = other.run_budget().unwrap_err().to_string();
        assert!(e.contains("the daily budget"), "{e}");
    }

    #[test]
    fn a_budget_is_a_positive_amount_in_a_currency() {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let task = Task::create(home.path(), ws.path()).unwrap();
        for (amount, currency) in [
            (0.0, "USD"),
            (-1.0, "USD"),
            (f64::NAN, "USD"),
            (5.0, "usd"),
            (5.0, "DOLLAR"),
        ] {
            let budget = Some(Money {
                amount,
                currency: currency.into(),
            });
            assert!(task.set_budget(budget).is_err(), "{amount} {currency}");
        }
    }
}

/// The lock under which shares are set aside and usage recorded, across
/// Kule's processes: `<home>/budget.lock`.
fn budget_lock(home: &Path) -> Result<crate::fsops::Flock> {
    std::fs::create_dir_all(home).at(home)?;
    crate::fsops::Flock::take(&home.join("budget.lock"))
}

/// Adds `record` as a line at the end of `path`, on disk before it
/// returns; a line cut short before it (a crash as it was written) stays
/// one that cannot be read, and does not take this one with it.
fn append_record(path: &Path, record: &impl Serialize) -> Result<()> {
    let mut line = serde_json::to_vec(record).map_err(|source| Error::State {
        path: path.to_owned(),
        source,
    })?;
    line.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .at(path)?;
    if ends_unfinished(&mut file).at(path)? {
        line.insert(0, b'\n');
    }
    file.write_all(&line).at(path)?;
    file.sync_data().at(path)
}

/// The records at `path` that can be read, and how many cannot; none when
/// it is missing.
fn read_records<T: serde::de::DeserializeOwned>(path: &Path) -> Result<(Vec<T>, usize)> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(e).at(path),
    };
    let mut records = Vec::new();
    let mut unreadable = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str(line) {
            Ok(record) => records.push(record),
            Err(_) => unreadable += 1,
        }
    }
    Ok((records, unreadable))
}

/// A share's id: at random, as a task's is.
fn random_id() -> Result<String> {
    let random = Path::new("/dev/urandom");
    let mut bytes = [0u8; 8];
    std::fs::File::open(random)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes))
        .at(random)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Whether `file` has something in it and its last byte ends no line.
fn ends_unfinished(file: &mut std::fs::File) -> std::io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8];
    file.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

/// What a home's tasks may spend together, kept in `<home>/budget.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct HomeBudget {
    #[serde(default)]
    daily: Option<Money>,
}

const BUDGET_FILE: &str = "budget.json";
const DAY_SECS: u64 = 24 * 60 * 60;

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What every task in `home` may spend together in any 24 hours; `None`
/// for no limit.
pub fn daily_budget(home: &Path) -> Result<Option<Money>> {
    let path = home.join(BUDGET_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    Ok(read_json::<HomeBudget>(&path)?.daily)
}

/// Sets [`daily_budget`]: a positive amount in an ISO 4217 currency, or
/// none.
pub fn set_daily_budget(home: &Path, budget: Option<Money>) -> Result<()> {
    if let Some(b) = &budget {
        b.check_budget()?;
    }
    std::fs::create_dir_all(home).at(home)?;
    write_json(&home.join(BUDGET_FILE), &HomeBudget { daily: budget })
}

/// What every task in `home` used in the last 24 hours: the runs that
/// started then, and the shares set aside then for runs whose usage has
/// not come.
pub fn spent_today(home: &Path) -> Result<Spent> {
    let since = now_unix().saturating_sub(DAY_SECS);
    let mut spent = Spent::default();
    for info in Task::list(home)? {
        let task = Task::open(home, &info.id)?;
        let Ledger {
            runs,
            unreadable,
            open,
        } = task.ledger()?;
        // Not known when: it may be today's.
        spent.unreadable += unreadable;
        for share in open.iter().filter(|s| s.started_unix >= since) {
            spent.hold(share, task.id());
        }
        for run in runs {
            if run.started_unix >= since {
                spent.add(run);
            }
        }
    }
    Ok(spent)
}

/// What is left of the daily budget for the next run of any task: `None`
/// without one, and an amount of zero or less when it is spent.
pub fn daily_left(home: &Path) -> Result<Option<Money>> {
    let Some(budget) = daily_budget(home)? else {
        return Ok(None);
    };
    let spent = spent_today(home)?.counted_in(&budget.currency);
    Ok(Some(Money {
        amount: budget.amount - spent,
        currency: budget.currency,
    }))
}
