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
        self.primed = true;
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
}
