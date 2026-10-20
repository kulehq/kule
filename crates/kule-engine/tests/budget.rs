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
