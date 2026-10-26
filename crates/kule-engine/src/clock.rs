//! The filesystem's clock, and stamps that cannot be trusted yet.
//!
//! A stamp notices a later write only when that write moves the change time.
//! Filesystems keep time in ticks (a few milliseconds on Linux, two seconds
//! on FAT), so a write of the same size within the tick the stamp was read
//! in leaves the stamp as it was, and Kule would overwrite or miss it. A
//! stamp is safe once the filesystem's clock has passed its change time:
//! every write after that gets a later one. Until then, the entry has to be
//! checked by content.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::{IoContext, Result};

/// The tick assumed for a filesystem Kule cannot probe: FAT's two seconds.
const FOREIGN_TICK: Duration = Duration::from_secs(2);

/// How long [`Clock::wait_past`] waits for a clock that does not move.
const PATIENCE: Duration = Duration::from_secs(5);

pub(crate) struct Clock {
    /// A probe file whose change time tells the filesystem's time, or `None`
    /// when the probe would sit on another filesystem than the entries.
    probe: Option<PathBuf>,
}

impl Clock {
    /// A clock for entries under `watched`, read from a probe file in `dir`.
    /// A probe tells the time of its own filesystem only, so when `dir` is on
    /// another device the clock goes by wall time, a [`FOREIGN_TICK`] behind.
    pub fn new(dir: &Path, watched: &Path) -> Result<Clock> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let same =
            crate::osfs::device_of(dir).at(dir)? == crate::osfs::device_of(watched).at(watched)?;
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        Ok(Clock {
            probe: same.then(|| dir.join(format!(".kule-clock-{}-{n}", std::process::id()))),
        })
    }

    /// A time such that an entry whose change time is earlier was last
    /// changed in a tick that is over.
    pub fn now(&self) -> Result<(i64, i64)> {
        let Some(probe) = &self.probe else {
            let wall = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .saturating_sub(FOREIGN_TICK);
            return Ok((wall.as_secs() as i64, i64::from(wall.subsec_nanos())));
        };
        // A new file each time: rewriting an empty one may not move its time.
        let _ = fs::remove_file(probe);
        let file = File::create(probe).at(probe)?;
        let changed = crate::osfs::change_time(&file).at(probe);
        drop(file);
        let _ = fs::remove_file(probe);
        changed
    }

    /// Waits until every write from now on gets a change time later than
    /// `t`. Returns false when the clock did not get there in [`PATIENCE`].
    pub fn wait_past(&self, t: (i64, i64)) -> Result<bool> {
        let start = Instant::now();
        loop {
            if self.now()? > t {
                return Ok(true);
            }
            if start.elapsed() > PATIENCE {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
