//! What a home's tasks may spend together in a day, on real task folders.

use kule_engine::{
    BudgetScope, Money, RunUsage, Task, create_group, daily_budget, daily_left, groups,
    groups_spent, set_daily_budget, set_group_budget, spent_today,
};
use tempfile::TempDir;

fn usd(amount: f64) -> Money {
    Money {
        amount,
        currency: "USD".into(),
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn run(started_unix: u64, cost: Option<Money>) -> RunUsage {
    RunUsage {
        agent: "claude-code".into(),
        started_unix,
        input_tokens: 100,
        cost,
        ..Default::default()
    }
}

#[test]
fn every_task_counts_toward_the_daily_budget_for_a_day() {
    let (home, a, b) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
    );
    assert_eq!(daily_budget(home.path()).unwrap(), None);
    assert_eq!(daily_left(home.path()).unwrap(), None);

    let one = Task::create(home.path(), a.path()).unwrap();
    let two = Task::create(home.path(), b.path()).unwrap();
    one.record_usage(&run(now() - 60, Some(usd(1.5)))).unwrap();
    two.record_usage(&run(now() - 60, Some(usd(2.0)))).unwrap();
    two.record_usage(&run(now() - 60, None)).unwrap();
    // Two days ago: not today's.
    one.record_usage(&run(now() - 2 * 24 * 3600, Some(usd(9.0))))
        .unwrap();

    let today = spent_today(home.path()).unwrap();
    assert_eq!((today.runs, today.unpriced_runs), (3, 1));
    assert!((today.in_currency("USD") - 3.5).abs() < 1e-9);

    set_daily_budget(home.path(), Some(usd(5.0))).unwrap();
    assert_eq!(daily_budget(home.path()).unwrap(), Some(usd(5.0)));
    let left = daily_left(home.path()).unwrap().unwrap();
    assert!((left.amount - 1.5).abs() < 1e-9, "{left:?}");

    assert!(set_daily_budget(home.path(), Some(usd(0.0))).is_err());
    assert!(
        set_daily_budget(
            home.path(),
            Some(Money {
                amount: 1.0,
                currency: "usd".into()
            })
        )
        .is_err()
    );
    set_daily_budget(home.path(), None).unwrap();
    assert_eq!(daily_left(home.path()).unwrap(), None);
}

#[test]
fn the_tighter_limit_stands() {
    assert_eq!(
        Money::tighter(Some(usd(2.0)), Some(usd(1.0))),
        Some(usd(1.0))
    );
    assert_eq!(
        Money::tighter(Some(usd(1.0)), Some(usd(2.0))),
        Some(usd(1.0))
    );
    assert_eq!(Money::tighter(None, Some(usd(2.0))), Some(usd(2.0)));
    let eur = Money {
        amount: 0.5,
        currency: "EUR".into(),
    };
    assert_eq!(Money::tighter(Some(usd(2.0)), Some(eur)), Some(usd(2.0)));
}

#[test]
fn a_group_s_budgets_hold_its_tasks_together() {
    let (home, a, b, c) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
    );
    let h = home.path();
    let group = create_group(h, "clients", None).unwrap();
    let one = Task::create(h, a.path()).unwrap();
    let two = Task::create(h, b.path()).unwrap();
    let alone = Task::create(h, c.path()).unwrap();
    one.set_group(Some(group.id)).unwrap();
    two.set_group(Some(group.id)).unwrap();

    // Refused unless a positive amount in a currency, and only for a group
    // there is.
    assert!(set_group_budget(h, group.id, Some(usd(0.0)), None).is_err());
    assert!(set_group_budget(h, group.id, None, Some(usd(-1.0))).is_err());
    assert!(set_group_budget(h, group.id + 1, Some(usd(5.0)), None).is_err());
    let kept = set_group_budget(h, group.id, Some(usd(5.0)), Some(usd(3.0))).unwrap();
    assert_eq!(
        (kept.budget, kept.daily_budget),
        (Some(usd(5.0)), Some(usd(3.0)))
    );
    assert_eq!(groups(h).unwrap()[0].budget, Some(usd(5.0)));

    one.record_usage(&run(now() - 60, Some(usd(1.0)))).unwrap();
    two.record_usage(&run(now() - 2 * 24 * 3600, Some(usd(2.5))))
        .unwrap();
    // A task in no group spends from no group's budget.
    alone
        .record_usage(&run(now() - 60, Some(usd(10.0))))
        .unwrap();
    let spent = groups_spent(h).unwrap();
    assert_eq!(spent.len(), 1);
    assert!((spent[&group.id].total.in_currency("USD") - 3.5).abs() < 1e-9);
    assert!((spent[&group.id].today.in_currency("USD") - 1.0).abs() < 1e-9);

    let budgets = one.budgets().unwrap();
    assert_eq!(budgets.len(), 2, "{budgets:?}");
    assert!(matches!(budgets[0].scope, BudgetScope::Group { group: g, .. } if g == group.id));
    assert!((budgets[0].left.amount - 1.5).abs() < 1e-9);
    assert!(matches!(budgets[1].scope, BudgetScope::GroupDaily { .. }));
    assert!((budgets[1].left.amount - 2.0).abs() < 1e-9);
    // The tightest holds the run.
    assert!((one.run_budget().unwrap().unwrap().amount - 1.5).abs() < 1e-9);
    assert_eq!(alone.run_budget().unwrap(), None);

    // Spent: every task in the group is refused, and says it is the
    // group's budget.
    two.record_usage(&run(now() - 60, Some(usd(1.5)))).unwrap();
    for task in [&one, &two] {
        let e = task.run_budget().unwrap_err().to_string();
        assert!(e.contains("group \"clients\" has spent its budget"), "{e}");
    }
    assert_eq!(alone.run_budget().unwrap(), None);

    // A task moved out keeps what it spent in the group's account.
    two.set_group(None).unwrap();
    assert!(one.run_budget().is_err());
    assert_eq!(two.run_budget().unwrap(), None);

    // The day's budget holds on its own: 2.5 spent today of 3.
    set_group_budget(h, group.id, Some(usd(50.0)), Some(usd(2.5))).unwrap();
    let e = one.run_budget().unwrap_err().to_string();
    assert!(
        e.contains("group \"clients\"'s daily budget is spent"),
        "{e}"
    );

    // A task's own budget and the daily one for every task hold beside the
    // group's, each named.
    set_group_budget(h, group.id, None, None).unwrap();
    assert_eq!(one.run_budget().unwrap(), None);
    one.set_budget(Some(usd(0.5))).unwrap();
    let e = one.run_budget().unwrap_err().to_string();
    assert!(
        e.contains(&format!("task {} has spent its budget", one.id())),
        "{e}"
    );
    one.set_budget(None).unwrap();
    set_daily_budget(h, Some(usd(4.0))).unwrap();
    let e = one.run_budget().unwrap_err().to_string();
    assert!(
        e.contains("the daily budget is spent: every task together"),
        "{e}"
    );
}

/// What a run may spend is set aside before it starts: until its usage is
/// recorded, the whole of it counts, so a run whose usage never comes
/// (Kule could not write it, a crash) is not taken for nothing spent.
/// Once recorded, only what the run spent counts.
#[test]
fn a_run_whose_cost_is_not_recorded_stays_counted_at_its_reservation() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let task = Task::create(home.path(), ws.path()).unwrap();
    assert!(
        task.reserve_run().unwrap().is_none(),
        "no budget, nothing to set aside"
    );
    task.set_budget(Some(usd(2.0))).unwrap();
    let reserved = task.reserve_run().unwrap().unwrap();
    assert_eq!(reserved.limit, usd(2.0));
    // Nothing recorded yet: all of it is held.
    let e = task.run_budget().unwrap_err().to_string();
    assert!(e.contains("set aside"), "{e}");
    assert_eq!(task.spent().unwrap().held, [usd(2.0)]);
    let mut spent = run(now(), Some(usd(0.5)));
    spent.reservation = Some(reserved.id.clone());
    task.record_usage(&spent).unwrap();
    assert_eq!(task.run_budget().unwrap(), Some(usd(1.5)));
    assert!(task.spent().unwrap().held.is_empty());
}

/// Runs started at once under a budget they share see each other: what
/// one set aside, the next cannot have.
#[test]
fn runs_started_together_share_what_is_left() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let tasks: Vec<Task> = (0..8)
        .map(|_| Task::create(home.path(), ws.path()).unwrap())
        .collect();
    set_daily_budget(home.path(), Some(usd(10.0))).unwrap();
    let got: Vec<bool> = std::thread::scope(|scope| {
        let runs: Vec<_> = tasks
            .iter()
            .map(|t| scope.spawn(|| t.reserve_run()))
            .collect();
        runs.into_iter()
            .map(|r| match r.join().unwrap() {
                Ok(Some(r)) => {
                    assert_eq!(r.limit, usd(10.0));
                    true
                }
                Ok(None) => panic!("a budget holds"),
                Err(e) => {
                    assert!(e.to_string().contains("set aside"), "{e}");
                    false
                }
            })
            .collect()
    });
    assert_eq!(got.iter().filter(|g| **g).count(), 1, "{got:?}");
    assert_eq!(spent_today(home.path()).unwrap().held, [usd(10.0)]);
}

/// A run whose share cannot be set aside does not start.
#[test]
fn a_reservation_that_cannot_be_written_starts_no_run() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let task = Task::create(home.path(), ws.path()).unwrap();
    task.set_budget(Some(usd(2.0))).unwrap();
    // Read, but not written to.
    use std::os::unix::fs::PermissionsExt;
    let path = home
        .path()
        .join("tasks")
        .join(task.id())
        .join("reserved.jsonl");
    std::fs::write(&path, "").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
    let e = task.reserve_run().unwrap_err().to_string();
    assert!(e.contains("does not start"), "{e}");
}

/// A share set aside for a day's budget leaves it with the day; the
/// task's own budget, over all of its runs, keeps counting it.
#[test]
fn a_daily_reservation_ages_out() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let task = Task::create(home.path(), ws.path()).unwrap();
    std::fs::write(
        home.path().join("tasks").join(task.id()).join("reserved.jsonl"),
        format!(
            "{{\"id\":\"old\",\"started_unix\":{},\"amount\":{{\"amount\":3.0,\"currency\":\"USD\"}}}}\n",
            now() - 2 * 24 * 3600
        ),
    )
    .unwrap();
    assert!(spent_today(home.path()).unwrap().held.is_empty());
    assert_eq!(task.spent().unwrap().held, [usd(3.0)]);
}

/// A share a run that did not end still holds can be let go of by the
/// person, knowingly: never while an agent is at work in the task, and
/// what was let go stays on record.
#[test]
fn a_reservation_of_a_run_at_work_is_not_released() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let task = Task::create(home.path(), ws.path()).unwrap();
    task.set_budget(Some(usd(2.0))).unwrap();
    task.reserve_run().unwrap().unwrap();
    {
        let _at_work = task.agent_at_work().unwrap();
        let e = task.release_unfinished().unwrap_err().to_string();
        assert!(e.contains("at work"), "{e}");
    }
    assert_eq!(task.release_unfinished().unwrap(), [usd(2.0)]);
    assert_eq!(task.run_budget().unwrap(), Some(usd(2.0)));
    assert_eq!(task.spent().unwrap().runs, 0, "a release is no run");
    assert!(task.release_unfinished().unwrap().is_empty());
    let kept = std::fs::read_to_string(
        home.path()
            .join("tasks")
            .join(task.id())
            .join("usage.jsonl"),
    )
    .unwrap();
    assert!(kept.contains("\"released\":true"), "{kept}");
}

/// A share counts for the group the task was in when it was set aside:
/// another of the group's tasks sees it.
#[test]
fn a_share_counts_for_the_group() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let (one, other) = (
        Task::create(home.path(), ws.path()).unwrap(),
        Task::create(home.path(), ws.path()).unwrap(),
    );
    let group = create_group(home.path(), "clients", None).unwrap();
    set_group_budget(home.path(), group.id, Some(usd(4.0)), None).unwrap();
    one.set_group(Some(group.id)).unwrap();
    other.set_group(Some(group.id)).unwrap();
    assert_eq!(one.reserve_run().unwrap().unwrap().limit, usd(4.0));
    let e = other.run_budget().unwrap_err().to_string();
    assert!(
        e.contains("group \"clients\"'s budget") && e.contains("set aside"),
        "{e}"
    );
}
