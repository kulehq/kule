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

    /// What was spent in `currency`.
    pub fn in_currency(&self, currency: &str) -> f64 {
        self.cost
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

    /// Adds one run's usage to the task's, with the group the task is in
    /// now unless the usage names one.
    pub fn record_usage(&self, usage: &RunUsage) -> Result<()> {
        let path = self.usage_path();
        let mut usage = usage.clone();
        if usage.group.is_none() {
            usage.group = Some(self.group()?);
        }
        let mut line = serde_json::to_vec(&usage).map_err(|source| Error::State {
            path: path.clone(),
            source,
        })?;
        line.push(b'\n');
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(&line))
            .at(&path)
    }

    /// Every run's usage, oldest first; a line that cannot be read is left
    /// out.
    pub fn usage(&self) -> Result<Vec<RunUsage>> {
        let path = self.usage_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).at(&path),
        };
        Ok(text
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect())
    }

    /// What the task's runs used together.
    pub fn spent(&self) -> Result<Spent> {
        let mut spent = Spent::default();
        for run in self.usage()? {
            spent.add(run);
        }
        Ok(spent)
    }

    /// What is left of the budget for the next run: `None` without a
    /// budget, and an amount of zero or less when it is spent.
    pub fn budget_left(&self) -> Result<Option<Money>> {
        let Some(budget) = self.budget()? else {
            return Ok(None);
        };
        let spent = self.spent()?.in_currency(&budget.currency);
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
            let spent = self.spent()?.in_currency(&budget.currency);
            out.push(BudgetLeft::new(BudgetScope::Task, budget, spent));
        }
        if let Some(group) = crate::groups::group_of(&home, group)?
            && (group.budget.is_some() || group.daily_budget.is_some())
        {
            let spent = groups_spent(&home)?.remove(&group.id).unwrap_or_default();
            if let Some(budget) = group.budget {
                let used = spent.total.in_currency(&budget.currency);
                let scope = BudgetScope::Group {
                    group: group.id,
                    name: group.name.clone(),
                };
                out.push(BudgetLeft::new(scope, budget, used));
            }
            if let Some(budget) = group.daily_budget {
                let used = spent.today.in_currency(&budget.currency);
                let scope = BudgetScope::GroupDaily {
                    group: group.id,
                    name: group.name,
                };
                out.push(BudgetLeft::new(scope, budget, used));
            }
        }
        if let Some(budget) = daily_budget(&home)? {
            let used = spent_today(&home)?.in_currency(&budget.currency);
            out.push(BudgetLeft::new(BudgetScope::Daily, budget, used));
        }
        Ok(out)
    }

    /// What the task's next run may cost: the tightest of [`Task::budgets`],
    /// `None` without any. Refused, naming the budget, when one is spent.
    pub fn run_budget(&self) -> Result<Option<Money>> {
        let mut limit = None;
        for budget in self.budgets()? {
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
}

impl BudgetLeft {
    fn new(scope: BudgetScope, budget: Money, spent: f64) -> BudgetLeft {
        let left = Money {
            amount: budget.amount - spent,
            currency: budget.currency.clone(),
        };
        BudgetLeft {
            scope,
            budget,
            left,
        }
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
        for run in task.usage()? {
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
/// started then.
pub fn spent_today(home: &Path) -> Result<Spent> {
    let since = now_unix().saturating_sub(DAY_SECS);
    let mut spent = Spent::default();
    for info in Task::list(home)? {
        let task = Task::open(home, &info.id)?;
        for run in task.usage()? {
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
    let spent = spent_today(home)?.in_currency(&budget.currency);
    Ok(Some(Money {
        amount: budget.amount - spent,
        currency: budget.currency,
    }))
}
