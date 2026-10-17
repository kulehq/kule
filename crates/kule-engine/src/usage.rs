//! What a task's runs used, as their agents reported it: tokens, and what
//! it cost when an agent says (Claude Code does; one signed in with a
//! subscription may report tokens alone). One line per run in the task's
//! `usage.jsonl`, outside the copy, where nothing the task runs can reach.

use std::io::Write;

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::task::Task;

/// An amount in an ISO 4217 currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Money {
    pub amount: f64,
    pub currency: String,
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

    /// Adds one run's usage to the task's.
    pub fn record_usage(&self, usage: &RunUsage) -> Result<()> {
        let path = self.usage_path();
        let mut line = serde_json::to_vec(usage).map_err(|source| Error::State {
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
            spent.runs += 1;
            spent.tokens += run.input_tokens
                + run.output_tokens
                + run.thought_tokens
                + run.cached_read_tokens
                + run.cached_write_tokens;
            match run.cost {
                Some(cost) => match spent.cost.iter_mut().find(|m| m.currency == cost.currency) {
                    Some(sum) => sum.amount += cost.amount,
                    None => spent.cost.push(cost),
                },
                None => spent.unpriced_runs += 1,
            }
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
