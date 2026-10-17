//! What a window tells a person who is looking elsewhere: an agent waiting
//! for an answer, a run that is over, two tasks that start changing the same
//! thing. Each is told once.

use std::collections::HashSet;

use kule_engine::Question;

use crate::service::TaskView;

/// One thing to tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub title: String,
    pub body: String,
}

/// What was told already.
#[derive(Debug, Default)]
pub struct Alerts {
    asked: HashSet<u64>,
    /// Each pair of tasks (in order), the file, and what in it.
    met: HashSet<(String, String, String, String)>,
    /// Each task with how much of its budget it was told it spent (80 or 100).
    spending: HashSet<(String, u8)>,
    /// Meetings that were there before the first task list are not news.
    primed: bool,
}

impl Alerts {
    pub fn new() -> Alerts {
        Alerts::default()
    }

    /// The questions among `waiting` not told yet.
    pub fn questions(&mut self, waiting: &[Question]) -> Vec<Alert> {
        waiting
            .iter()
            .filter(|q| self.asked.insert(q.id))
            .map(|q| Alert {
                title: format!("Task {} waits for you", q.task),
                body: format!("Its agent wants to reach {}:{}.", q.host, q.port),
            })
            .collect()
    }

    /// A run that is over.
    pub fn finished(&self, task: &str, error: Option<&str>) -> Alert {
        match error {
            None => Alert {
                title: format!("Task {task} is done"),
                body: "Review its changes before they reach the folder.".to_owned(),
            },
            Some(error) => Alert {
                title: format!("Task {task} failed"),
                body: error.chars().take(200).collect(),
            },
        }
    }

    /// The places where two tasks meet that were not told yet, one alert
    /// per pair of tasks and file.
    pub fn tasks(&mut self, views: &[TaskView]) -> Vec<Alert> {
        let mut alerts = Vec::new();
        for view in views {
            for overlap in &view.overlaps {
                // Each meeting shows on both tasks; tell it from one side.
                if view.id > overlap.task {
                    continue;
                }
                let what = overlap.entities.join(", ");
                let key = (
                    view.id.clone(),
                    overlap.task.clone(),
                    overlap.path.clone(),
                    what.clone(),
                );
                if !self.met.insert(key) || !self.primed {
                    continue;
                }
                alerts.push(Alert {
                    title: format!("Tasks {} and {} meet", view.id, overlap.task),
                    body: match what.as_str() {
                        "" => format!("Both change {}.", overlap.path),
                        what => format!("Both change {what} in {}.", overlap.path),
                    },
                });
            }
        }
        alerts.extend(self.spending(views));
        self.primed = true;
        alerts
    }

    /// Tasks that spent 80% of their budget, or all of it, since last told.
    fn spending(&mut self, views: &[TaskView]) -> Vec<Alert> {
        let mut alerts = Vec::new();
        for view in views {
            let Some(budget) = &view.budget else {
                continue;
            };
            let spent = view.spent.in_currency(&budget.currency);
            let level = if spent >= budget.amount {
                100
            } else if spent >= budget.amount * 0.8 {
                80
            } else {
                // A budget raised since: told again when it comes near.
                self.spending.retain(|(task, _)| *task != view.id);
                continue;
            };
            let told = !self.spending.insert((view.id.clone(), level));
            // Told already, or known at start: not news.
            if told || !self.primed {
                continue;
            }
            let amounts = format!("{spent:.2} of {:.2} {}", budget.amount, budget.currency);
            alerts.push(if level == 100 {
                Alert {
                    title: format!("Task {} spent its budget", view.id),
                    body: format!("{amounts}; its next run waits until you raise it."),
                }
            } else {
                Alert {
                    title: format!("Task {} spent 80% of its budget", view.id),
                    body: format!("{amounts}."),
                }
            });
        }
        alerts
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::service::Overlap;

    fn view(id: &str, overlaps: &[(&str, &str, &[&str])]) -> TaskView {
        TaskView {
            id: id.into(),
            workspace: PathBuf::from("/w"),
            created_unix: 0,
            changes: 1,
            ignored: 0,
            verdict: None,
            running: false,
            waiting: 0,
            approval_timeout_secs: 900,
            workspace_gone: false,
            scanning: false,
            overlaps: overlaps
                .iter()
                .map(|(task, path, entities)| Overlap {
                    task: (*task).into(),
                    path: (*path).into(),
                    entities: entities.iter().map(|e| (*e).into()).collect(),
                })
                .collect(),
            spent: Default::default(),
            budget: None,
            home_changes: 0,
        }
    }

    #[test]
    fn a_meeting_is_told_once_and_not_the_ones_found_at_start() {
        let mut alerts = Alerts::new();
        let at_start = [
            view("a", &[("b", "x.rs", &["function f"])]),
            view("b", &[("a", "x.rs", &["function f"])]),
        ];
        assert!(alerts.tasks(&at_start).is_empty());
        let later = [
            view("a", &[("b", "x.rs", &["function f"]), ("c", "y.md", &[])]),
            view("b", &[("a", "x.rs", &["function f"])]),
            view("c", &[("a", "y.md", &[])]),
        ];
        assert_eq!(
            alerts.tasks(&later),
            [Alert {
                title: "Tasks a and c meet".into(),
                body: "Both change y.md.".into()
            }]
        );
        assert!(alerts.tasks(&later).is_empty());
        // More in the same file is news again.
        let more = [view("a", &[("b", "x.rs", &["function f", "function g"])])];
        assert_eq!(
            alerts.tasks(&more)[0].body,
            "Both change function f, function g in x.rs."
        );
    }

    #[test]
    fn each_question_is_told_once() {
        let mut alerts = Alerts::new();
        let q = Question {
            id: 7,
            task: "a".into(),
            host: "example.com".into(),
            port: 443,
            rules: vec![],
            asked_unix: 0,
            deadline_unix: 0,
        };
        assert_eq!(alerts.questions(std::slice::from_ref(&q)).len(), 1);
        assert!(alerts.questions(&[q]).is_empty());
    }

    #[test]
    fn a_task_near_or_past_its_budget_is_told_once_each() {
        let spending = |id: &str, spent: f64, budget: f64| {
            let mut v = view(id, &[]);
            v.budget = Some(kule_engine::Money {
                amount: budget,
                currency: "USD".into(),
            });
            v.spent.cost = vec![kule_engine::Money {
                amount: spent,
                currency: "USD".into(),
            }];
            v
        };
        let titles =
            |alerts: Vec<Alert>| -> Vec<String> { alerts.into_iter().map(|a| a.title).collect() };
        let mut alerts = Alerts::new();
        // Spent before the window opened: not news.
        assert!(alerts.tasks(&[spending("a", 9.0, 10.0)]).is_empty());
        assert!(alerts.tasks(&[spending("b", 7.0, 10.0)]).is_empty());
        assert_eq!(
            titles(alerts.tasks(&[spending("b", 8.5, 10.0)])),
            ["Task b spent 80% of its budget"]
        );
        assert!(alerts.tasks(&[spending("b", 9.0, 10.0)]).is_empty());
        let past = alerts.tasks(&[spending("b", 10.5, 10.0)]);
        assert_eq!(past[0].title, "Task b spent its budget");
        assert!(
            past[0].body.starts_with("10.50 of 10.00 USD"),
            "{}",
            past[0].body
        );
        assert!(alerts.tasks(&[spending("b", 11.0, 10.0)]).is_empty());
        // Raised: near the new one, told again.
        assert!(alerts.tasks(&[spending("b", 11.0, 20.0)]).is_empty());
        assert_eq!(
            titles(alerts.tasks(&[spending("b", 16.0, 20.0)])),
            ["Task b spent 80% of its budget"]
        );
    }
}
