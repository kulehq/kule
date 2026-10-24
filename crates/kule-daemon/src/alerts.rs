//! What a window tells a person who is looking elsewhere: an agent waiting
//! for an answer, a run that is over, two tasks that start changing the same
//! thing. Each is told once.

use std::collections::HashSet;

use kule_engine::{Answer, BudgetScope, Money, Question};

use crate::service::{SharedBudget, TaskView};

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
    /// Each budget ("task:ID", "group:ID", "group-day:ID", "daily") with how
    /// much of it it was told was spent (80 or 100).
    spending: HashSet<(String, u8)>,
    /// Meetings that were there before the first task list are not news.
    primed: bool,
    /// Shared budgets spent before the first look at them are not news.
    shared_primed: bool,
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

    /// A connection of task `task`'s run that Kule refused without the
    /// person answering it: nobody did in time, the run ended first, or too
    /// many were waiting. `None` for the person's own answer.
    pub fn answered(&self, task: &str, host: &str, port: u16, answer: Answer) -> Option<Alert> {
        let why = match answer {
            Answer::Approved | Answer::Denied => return None,
            Answer::TimedOut => "nobody answered in time",
            Answer::Withdrawn => "the run ended before anyone answered",
            Answer::TooMany => "too many of its questions were waiting already",
        };
        Some(Alert {
            title: format!("Task {task}: Kule refused a connection"),
            body: format!("{}:{port}, as {why}.", plain(host)),
        })
    }

    /// A connection the person approved that did not go through as
    /// approved.
    pub fn after_approval(&self, task: &str, host: &str, port: u16, why: &str) -> Alert {
        Alert {
            title: format!("Task {task}: a connection you approved ended"),
            body: format!("{}:{port}: {}", plain(host), plain(why))
                .chars()
                .take(200)
                .collect(),
        }
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
                body: plain(error).chars().take(200).collect(),
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
                    // Paths and names the agents chose.
                    body: match what.as_str() {
                        "" => format!("Both change {}.", plain(&overlap.path)),
                        what => format!("Both change {} in {}.", plain(what), plain(&overlap.path)),
                    }
                    .chars()
                    .take(200)
                    .collect(),
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
            let Some(level) = self.news(format!("task:{}", view.id), spent, budget, self.primed)
            else {
                continue;
            };
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

    /// Budgets tasks share, a group's or the daily one for every task, that
    /// were 80% spent, or all of it, since last told. Those found so at the
    /// first task list are not news.
    pub fn shared(&mut self, budgets: &[SharedBudget]) -> Vec<Alert> {
        let mut alerts = Vec::new();
        for b in budgets {
            let key = match &b.scope {
                BudgetScope::Group { group, .. } => format!("group:{group}"),
                BudgetScope::GroupDaily { group, .. } => format!("group-day:{group}"),
                BudgetScope::Daily => "daily".to_owned(),
                BudgetScope::Task => continue,
            };
            let Some(level) = self.news(key, b.spent, &b.budget, self.shared_primed) else {
                continue;
            };
            let amounts = format!(
                "{:.2} of {:.2} {}",
                b.spent, b.budget.amount, b.budget.currency
            );
            let all = level == 100;
            alerts.push(match &b.scope {
                BudgetScope::Group { name, .. } => Alert {
                    title: if all {
                        format!("Group {} spent its budget", plain(name))
                    } else {
                        format!("Group {} spent 80% of its budget", plain(name))
                    },
                    body: if all {
                        format!("{amounts}, its tasks together; their next runs wait until you raise it.")
                    } else {
                        format!("{amounts}, its tasks together.")
                    },
                },
                BudgetScope::GroupDaily { name, .. } => Alert {
                    title: if all {
                        format!("Group {} spent its daily budget", plain(name))
                    } else {
                        format!("Group {} spent 80% of its daily budget", plain(name))
                    },
                    body: if all {
                        format!("{amounts} in the last 24 hours, its tasks together; their next runs wait until older runs fall out of the 24 hours or you raise it.")
                    } else {
                        format!("{amounts} in the last 24 hours, its tasks together.")
                    },
                },
                _ => Alert {
                    title: if all {
                        "The daily budget is spent".to_owned()
                    } else {
                        "80% of the daily budget is spent".to_owned()
                    },
                    body: if all {
                        format!("{amounts} in the last 24 hours, every task together; no run starts until older runs fall out of the 24 hours or you raise it.")
                    } else {
                        format!("{amounts} in the last 24 hours, every task together.")
                    },
                },
            });
        }
        self.shared_primed = true;
        alerts
    }

    /// The level (80 or 100) budget `key` reached, when it is news: not
    /// told at that level since it was last below 80%, and not found so at
    /// start. Below 80% (raised, or a day's runs fallen out of the 24
    /// hours) it is told again when it comes near.
    fn news(&mut self, key: String, spent: f64, budget: &Money, primed: bool) -> Option<u8> {
        let level = if spent >= budget.amount {
            100
        } else if spent >= budget.amount * 0.8 {
            80
        } else {
            self.spending.retain(|(k, _)| *k != key);
            return None;
        };
        let told = !self.spending.insert((key, level));
        (!told && primed).then_some(level)
    }
}

/// What an agent said, fit for a system notification: no control
/// characters or marks that turn the text around.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            // Marks that turn text around, and characters that show nothing.
            !c.is_control()
                && !matches!(
                    c,
                    '\u{00AD}'
                        | '\u{061C}'
                        | '\u{200B}'..='\u{200F}'
                        | '\u{2028}'..='\u{202E}'
                        | '\u{2060}'..='\u{2069}'
                        | '\u{FEFF}'
                        | '\u{E0000}'..='\u{E007F}'
                )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::service::Overlap;

    #[test]
    fn what_an_agent_said_reaches_a_notification_without_hidden_characters() {
        let alerts = Alerts::default();
        let said = alerts.finished("t1", Some("failed\u{1b}[2K\u{202E}gnp.exe\nnow"));
        assert_eq!(said.body, "failed[2Kgnp.exenow");
    }

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
            label: None,
            color: None,
            group: None,
            limits: Default::default(),
            own_limits: Default::default(),
            time_used_secs: 0,
            behind: Default::default(),
            unreadable: None,
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
        // A path and a name the agent chose, with hidden characters in them.
        let hidden = [view(
            "a",
            &[(
                "d",
                "a\u{202E}gnp.exe\u{200B}",
                &["fn \u{1b}]0;x\u{7}\u{E0041}f"],
            )],
        )];
        assert_eq!(
            alerts.tasks(&hidden)[0].body,
            "Both change fn ]0;xf in agnp.exe."
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
            inside: None,
        };
        assert_eq!(alerts.questions(std::slice::from_ref(&q)).len(), 1);
        assert!(alerts.questions(&[q]).is_empty());
    }

    /// What Kule refused without the person's answer is told, saying why;
    /// the person's own answers are not news to them.
    #[test]
    fn a_connection_kule_refused_unanswered_is_told_with_why() {
        let alerts = Alerts::new();
        let told = |answer| alerts.answered("a", "example.com", 443, answer);
        assert_eq!(told(Answer::Approved), None);
        assert_eq!(told(Answer::Denied), None);
        for (answer, why) in [
            (Answer::TimedOut, "nobody answered in time"),
            (Answer::Withdrawn, "the run ended before anyone answered"),
            (Answer::TooMany, "too many of its questions were waiting"),
        ] {
            let alert = told(answer).unwrap();
            assert!(alert.title.contains("Kule refused"), "{alert:?}");
            assert!(
                alert.body.contains("example.com:443") && alert.body.contains(why),
                "{alert:?}"
            );
        }
        let cut = alerts.after_approval("a", "example.com", 443, "cut by Kule: another host");
        assert!(cut.body.contains("cut by Kule: another host"), "{cut:?}");
    }

    #[test]
    fn a_group_s_budgets_and_the_daily_one_are_told_near_and_past_once_each() {
        let usd = |amount| Money {
            amount,
            currency: "USD".into(),
        };
        let group = |spent, day_spent, daily_spent| {
            let name = "cli\u{202E}ents".to_owned();
            vec![
                SharedBudget {
                    scope: BudgetScope::Group {
                        group: 1,
                        name: name.clone(),
                    },
                    budget: usd(10.0),
                    spent,
                },
                SharedBudget {
                    scope: BudgetScope::GroupDaily { group: 1, name },
                    budget: usd(2.0),
                    spent: day_spent,
                },
                SharedBudget {
                    scope: BudgetScope::Daily,
                    budget: usd(5.0),
                    spent: daily_spent,
                },
            ]
        };
        let titles =
            |alerts: Vec<Alert>| -> Vec<String> { alerts.into_iter().map(|a| a.title).collect() };
        let mut alerts = Alerts::new();
        // The task list's first look does not prime these: their own does.
        assert!(alerts.tasks(&[]).is_empty());
        // Spent before the first look: not news.
        assert!(alerts.shared(&group(9.0, 0.0, 5.0)).is_empty());
        assert!(alerts.shared(&group(9.5, 1.0, 5.5)).is_empty());

        // The group's day nears, then is spent; its name shows nothing hidden.
        assert_eq!(
            titles(alerts.shared(&group(9.5, 1.7, 5.5))),
            ["Group clients spent 80% of its daily budget"]
        );
        let past = alerts.shared(&group(10.0, 2.0, 5.5));
        assert_eq!(
            titles(past.clone()),
            [
                "Group clients spent its budget",
                "Group clients spent its daily budget"
            ]
        );
        assert!(
            past[0]
                .body
                .starts_with("10.00 of 10.00 USD, its tasks together"),
            "{}",
            past[0].body
        );
        assert!(
            past[1].body.contains("in the last 24 hours"),
            "{}",
            past[1].body
        );
        assert!(alerts.shared(&group(10.5, 2.5, 5.5)).is_empty());

        // A day's runs fall out of the 24 hours, then it comes near again.
        assert!(alerts.shared(&group(10.5, 0.5, 1.0)).is_empty());
        assert_eq!(
            titles(alerts.shared(&group(10.5, 0.5, 4.2))),
            ["80% of the daily budget is spent"]
        );
        assert_eq!(
            titles(alerts.shared(&group(10.5, 1.8, 5.0))),
            [
                "Group clients spent 80% of its daily budget",
                "The daily budget is spent"
            ]
        );
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
