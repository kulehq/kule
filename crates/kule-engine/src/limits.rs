//! What a task's runs may take: processor, memory, disk, the GPU and time.
//! One set for every task in `<home>/limits.json`, and each task's own in
//! its settings, field by field: a task that sets nothing takes them all
//! from the home.

use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fsops::{read_json, write_json};
use crate::task::Task;

const LIMITS_FILE: &str = "limits.json";

/// Limits in force; `None` is no limit. The default limits nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Processor time, in percent of one core: 250 is two and a half cores.
    /// Over it, the run is slowed down, not stopped.
    #[serde(default)]
    pub cpu_percent: Option<u32>,
    /// What the run's processes hold in memory together; over it, the run
    /// stops.
    #[serde(default)]
    pub memory_mb: Option<u64>,
    /// What the task's copy, temporary directory and home changes may grow
    /// by since the task was made; over it, the run stops.
    #[serde(default)]
    pub disk_mb: Option<u64>,
    /// Whether the run's processes may use the GPU.
    #[serde(default = "allowed")]
    pub gpu: bool,
    /// How long one run may take.
    #[serde(default)]
    pub run_minutes: Option<u64>,
    /// How long all of the task's runs may take together.
    #[serde(default)]
    pub task_minutes: Option<u64>,
    /// How long the agent may do nothing (no message, tool call, output or
    /// processor use) before the run stops. A question waiting on a person
    /// does not count.
    #[serde(default)]
    pub idle_minutes: Option<u64>,
}

fn allowed() -> bool {
    true
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            cpu_percent: None,
            memory_mb: None,
            disk_mb: None,
            gpu: true,
            run_minutes: None,
            task_minutes: None,
            idle_minutes: None,
        }
    }
}

/// What a task sets for itself, field by field: absent takes the home's,
/// `null` (`Some(None)`) is no limit for this task, a value is this task's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskLimits {
    #[serde(
        default,
        deserialize_with = "set",
        skip_serializing_if = "Option::is_none"
    )]
    pub cpu_percent: Option<Option<u32>>,
    #[serde(
        default,
        deserialize_with = "set",
        skip_serializing_if = "Option::is_none"
    )]
    pub memory_mb: Option<Option<u64>>,
    #[serde(
        default,
        deserialize_with = "set",
        skip_serializing_if = "Option::is_none"
    )]
    pub disk_mb: Option<Option<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<bool>,
    #[serde(
        default,
        deserialize_with = "set",
        skip_serializing_if = "Option::is_none"
    )]
    pub run_minutes: Option<Option<u64>>,
    #[serde(
        default,
        deserialize_with = "set",
        skip_serializing_if = "Option::is_none"
    )]
    pub task_minutes: Option<Option<u64>>,
    #[serde(
        default,
        deserialize_with = "set",
        skip_serializing_if = "Option::is_none"
    )]
    pub idle_minutes: Option<Option<u64>>,
}

/// A field that is present, `null` included, is set.
fn set<'de, T, D>(d: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// The ranges a limit can take, so a typo cannot stop every run at once
/// or mean nothing.
const CPU: (u64, u64) = (10, 25_600);
const MEMORY_MB: (u64, u64) = (64, 4 * 1024 * 1024);
const DISK_MB: (u64, u64) = (1, 64 * 1024 * 1024);
const RUN_MINUTES: (u64, u64) = (1, 7 * 24 * 60);
const TASK_MINUTES: (u64, u64) = (1, 365 * 24 * 60);
const IDLE_MINUTES: (u64, u64) = (1, 24 * 60);

fn within(name: &str, value: Option<u64>, (low, high): (u64, u64)) -> Result<()> {
    match value {
        Some(v) if !(low..=high).contains(&v) => Err(Error::Invalid(format!(
            "{name} can be from {low} to {high}, not {v}"
        ))),
        _ => Ok(()),
    }
}

impl Limits {
    /// These, with what `task` sets for itself in their place.
    pub fn with(&self, task: &TaskLimits) -> Limits {
        Limits {
            cpu_percent: task.cpu_percent.unwrap_or(self.cpu_percent),
            memory_mb: task.memory_mb.unwrap_or(self.memory_mb),
            disk_mb: task.disk_mb.unwrap_or(self.disk_mb),
            gpu: task.gpu.unwrap_or(self.gpu),
            run_minutes: task.run_minutes.unwrap_or(self.run_minutes),
            task_minutes: task.task_minutes.unwrap_or(self.task_minutes),
            idle_minutes: task.idle_minutes.unwrap_or(self.idle_minutes),
        }
    }

    /// Refused unless every limit is in its range.
    pub fn check(&self) -> Result<()> {
        within(
            "the processor limit (percent of a core)",
            self.cpu_percent.map(u64::from),
            CPU,
        )?;
        within("the memory limit (MB)", self.memory_mb, MEMORY_MB)?;
        within("the disk limit (MB)", self.disk_mb, DISK_MB)?;
        within(
            "a run's time limit (minutes)",
            self.run_minutes,
            RUN_MINUTES,
        )?;
        within(
            "a task's time limit (minutes)",
            self.task_minutes,
            TASK_MINUTES,
        )?;
        within("the idle limit (minutes)", self.idle_minutes, IDLE_MINUTES)
    }
}

impl TaskLimits {
    /// Refused unless every value the task sets is in its range.
    pub fn check(&self) -> Result<()> {
        Limits::default().with(self).check()
    }
}

/// The limits of every task that sets none of its own.
pub fn limits(home: &Path) -> Result<Limits> {
    let path = home.join(LIMITS_FILE);
    if !path.is_file() {
        return Ok(Limits::default());
    }
    read_json(&path)
}

/// Sets [`limits`], each in its range.
pub fn set_limits(home: &Path, limits: &Limits) -> Result<()> {
    limits.check()?;
    std::fs::create_dir_all(home).at(home)?;
    write_json(&home.join(LIMITS_FILE), limits)
}

impl Task {
    /// The limits in force for the task's runs: its own, and the home's
    /// where it sets none.
    pub fn limits_in_force(&self) -> Result<Limits> {
        Ok(limits(&self.home())?.with(&self.limits()?))
    }

    /// How long the task's runs took together, as recorded.
    pub fn time_used(&self) -> Result<std::time::Duration> {
        let secs = self.usage()?.iter().map(|r| r.duration_secs).sum();
        Ok(std::time::Duration::from_secs(secs))
    }

    /// What is left of the task's time for the next run: `None` without a
    /// limit, zero when it is used up.
    pub fn time_left(&self) -> Result<Option<std::time::Duration>> {
        let Some(minutes) = self.limits_in_force()?.task_minutes else {
            return Ok(None);
        };
        let limit = std::time::Duration::from_secs(minutes.saturating_mul(60));
        Ok(Some(limit.saturating_sub(self.time_used()?)))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::usage::RunUsage;

    #[test]
    fn a_task_takes_the_home_s_limits_where_it_sets_none() {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let task = Task::create(home.path(), ws.path()).unwrap();
        assert_eq!(task.limits_in_force().unwrap(), Limits::default());

        let every = Limits {
            cpu_percent: Some(200),
            memory_mb: Some(4096),
            gpu: false,
            idle_minutes: Some(10),
            ..Limits::default()
        };
        set_limits(home.path(), &every).unwrap();
        assert_eq!(limits(home.path()).unwrap(), every);
        assert_eq!(task.limits_in_force().unwrap(), every);

        // Its own value, no limit of its own, and the home's.
        task.set_limits(&TaskLimits {
            cpu_percent: Some(Some(50)),
            memory_mb: Some(None),
            gpu: Some(true),
            ..TaskLimits::default()
        })
        .unwrap();
        let mine = task.limits_in_force().unwrap();
        assert_eq!(
            (
                mine.cpu_percent,
                mine.memory_mb,
                mine.gpu,
                mine.idle_minutes
            ),
            (Some(50), None, true, Some(10))
        );

        // Kept as it was said: absent, null and a value stay apart.
        let text = std::fs::read_to_string(task.dir().join("settings.json")).unwrap();
        let kept: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            kept["limits"],
            serde_json::json!({"cpu_percent": 50, "memory_mb": null, "gpu": true})
        );
        assert_eq!(task.limits().unwrap().memory_mb, Some(None));

        // The other settings stay as they were.
        task.set_budget(Some(crate::usage::Money {
            amount: 1.0,
            currency: "USD".into(),
        }))
        .unwrap();
        assert_eq!(task.limits().unwrap().cpu_percent, Some(Some(50)));
    }

    #[test]
    fn a_limit_out_of_its_range_is_refused() {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let task = Task::create(home.path(), ws.path()).unwrap();
        for wrong in [
            Limits {
                cpu_percent: Some(0),
                ..Limits::default()
            },
            Limits {
                memory_mb: Some(1),
                ..Limits::default()
            },
            Limits {
                disk_mb: Some(0),
                ..Limits::default()
            },
            Limits {
                run_minutes: Some(0),
                ..Limits::default()
            },
            Limits {
                task_minutes: Some(u64::MAX),
                ..Limits::default()
            },
            Limits {
                idle_minutes: Some(0),
                ..Limits::default()
            },
        ] {
            assert!(set_limits(home.path(), &wrong).is_err(), "{wrong:?}");
        }
        assert!(!home.path().join(LIMITS_FILE).exists());
        let wrong = TaskLimits {
            run_minutes: Some(Some(0)),
            ..TaskLimits::default()
        };
        assert!(task.set_limits(&wrong).is_err());
        assert_eq!(task.limits().unwrap(), TaskLimits::default());
    }

    #[test]
    fn the_task_s_time_is_what_its_runs_took() {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let task = Task::create(home.path(), ws.path()).unwrap();
        assert_eq!(task.time_left().unwrap(), None);
        task.set_limits(&TaskLimits {
            task_minutes: Some(Some(2)),
            ..TaskLimits::default()
        })
        .unwrap();
        let run = |secs| RunUsage {
            duration_secs: secs,
            ..RunUsage::default()
        };
        task.record_usage(&run(50)).unwrap();
        task.record_usage(&run(40)).unwrap();
        assert_eq!(task.time_used().unwrap().as_secs(), 90);
        assert_eq!(task.time_left().unwrap().unwrap().as_secs(), 30);
        task.record_usage(&run(60)).unwrap();
        assert_eq!(task.time_left().unwrap().unwrap().as_secs(), 0);
    }

    #[test]
    fn an_older_settings_file_reads_as_no_limits_of_its_own() {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let task = Task::create(home.path(), ws.path()).unwrap();
        std::fs::write(
            task.dir().join("settings.json"),
            r#"{"approval_timeout_secs": 300, "label": "old"}"#,
        )
        .unwrap();
        assert_eq!(task.limits().unwrap(), TaskLimits::default());
        assert_eq!(task.label().unwrap().as_deref(), Some("old"));
    }
}
