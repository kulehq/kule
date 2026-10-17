//! A task's name, colour and place in the list, on real task folders.

use kule_engine::{Task, TaskColor, arranged, set_task_order, task_order};
use tempfile::TempDir;

fn ids(home: &std::path::Path) -> Vec<String> {
    arranged(home).unwrap().into_iter().map(|i| i.id).collect()
}

#[test]
fn a_task_keeps_the_name_and_colour_it_was_given() {
    let (home, folder) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let task = Task::create(home.path(), folder.path()).unwrap();
    assert_eq!(task.label().unwrap(), None);
    assert_eq!(task.color().unwrap(), None);

    task.set_label(Some("  march invoices ")).unwrap();
    task.set_color(Some(TaskColor::Teal)).unwrap();
    task.set_budget(Some(kule_engine::Money {
        amount: 2.0,
        currency: "USD".into(),
    }))
    .unwrap();
    let again = Task::open(home.path(), task.id()).unwrap();
    assert_eq!(again.label().unwrap().as_deref(), Some("march invoices"));
    assert_eq!(again.color().unwrap(), Some(TaskColor::Teal));
    // The other settings stay as they were.
    assert_eq!(again.budget().unwrap().unwrap().amount, 2.0);

    assert!(again.set_label(Some("evil\u{202E}txt")).is_err());
    assert_eq!(again.label().unwrap().as_deref(), Some("march invoices"));

    again.set_label(Some("")).unwrap();
    again.set_color(None).unwrap();
    assert_eq!(again.label().unwrap(), None);
    assert_eq!(again.color().unwrap(), None);
    assert_eq!(again.budget().unwrap().unwrap().amount, 2.0);
}

#[test]
fn tasks_come_in_the_order_they_were_put_in_and_new_ones_on_top() {
    let (home, folder) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let a = Task::create(home.path(), folder.path()).unwrap();
    let b = Task::create(home.path(), folder.path()).unwrap();
    let c = Task::create(home.path(), folder.path()).unwrap();
    assert_eq!(task_order(home.path()).unwrap(), Vec::<String>::new());

    let order = vec![b.id().to_owned(), c.id().to_owned(), a.id().to_owned()];
    set_task_order(home.path(), &order).unwrap();
    assert_eq!(ids(home.path()), order);

    // A task made since comes first, the rest stay in their order.
    let d = Task::create(home.path(), folder.path()).unwrap();
    let mut with_d = vec![d.id().to_owned()];
    with_d.extend(order.iter().cloned());
    assert_eq!(ids(home.path()), with_d);

    // A deleted task drops out of the list but leaves the others in place.
    let c_id = c.id().to_owned();
    c.remove().unwrap();
    assert_eq!(ids(home.path()), [d.id(), b.id(), a.id()]);

    // Only tasks there are, once each.
    let err = set_task_order(home.path(), &[c_id, a.id().to_owned()]).unwrap_err();
    assert!(err.to_string().contains("no task"), "{err}");
    let twice = vec![a.id().to_owned(), a.id().to_owned()];
    let err = set_task_order(home.path(), &twice).unwrap_err();
    assert!(err.to_string().contains("twice"), "{err}");
    assert_eq!(ids(home.path()), [d.id(), b.id(), a.id()]);
}
