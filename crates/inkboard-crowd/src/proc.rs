//! CPU and memory of the current process (and the load of the whole machine), sampled about once a second. Windows and Linux; on other
//! systems every number is zero (the tool still runs).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcSample {
    /// Process CPU use since the previous sample, in cores (1.0 = one core fully busy; can exceed 1).
    pub cpu_cores: f64,
    /// Resident memory, MiB.
    pub rss_mib: f64,
    /// CPU use of the whole machine, percent of all cores (the load others put on the computer).
    pub machine_cpu_pct: f64,
    /// Total CPU time of the process so far, seconds.
    pub cpu_seconds: f64,
}

#[cfg(any(windows, target_os = "linux"))]
mod imp {
    use super::ProcSample;
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    #[derive(Debug)]
    pub struct ProcSampler {
        sys: System,
        pid: Pid,
    }

    impl ProcSampler {
        #[must_use]
        pub fn new() -> Self {
            let mut s = Self { sys: System::new(), pid: Pid::from_u32(std::process::id()) };
            s.refresh();
            s
        }

        fn refresh(&mut self) {
            self.sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[self.pid]), true, ProcessRefreshKind::nothing().with_cpu().with_memory());
            self.sys.refresh_cpu_usage();
        }

        pub fn sample(&mut self) -> ProcSample {
            self.refresh();
            let Some(p) = self.sys.process(self.pid) else { return ProcSample::default() };
            ProcSample {
                cpu_cores: f64::from(p.cpu_usage()) / 100.0,
                rss_mib: p.memory() as f64 / (1024.0 * 1024.0),
                machine_cpu_pct: f64::from(self.sys.global_cpu_usage()),
                cpu_seconds: p.accumulated_cpu_time() as f64 / 1000.0,
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::ProcSample;

    #[derive(Debug)]
    pub struct ProcSampler;

    impl ProcSampler {
        pub fn new() -> Self {
            Self
        }

        pub fn sample(&mut self) -> ProcSample {
            ProcSample::default()
        }
    }
}

pub use imp::ProcSampler;

impl Default for ProcSampler {
    fn default() -> Self {
        Self::new()
    }
}
