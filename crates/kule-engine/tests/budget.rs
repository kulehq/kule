//! What a home's tasks may spend together in a day, on real task folders.

use kule_engine::{Money, RunUsage, Task, daily_budget, daily_left, set_daily_budget, spent_today};
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
