//! Memory sampling.
//!
//! Samples are advisory: they inform warnings and restore pacing but cannot
//! predict allocation spikes. On Linux, pressure stall information (PSI) adds a
//! direct signal of time lost waiting for memory.

use std::sync::{Mutex, PoisonError};

use reopen_core::checkpoint::ProviderError;

/// Linux memory pressure: share of wall time some or all tasks stalled on memory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemoryPressure {
    /// Percent over the last 10 seconds that at least one task stalled.
    pub some_avg10: f32,
    /// Percent over the last 10 seconds that all non-idle tasks stalled.
    pub full_avg10: f32,
    pub some_avg60: f32,
    pub full_avg60: f32,
}

/// One reading of system memory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemorySample {
    pub total_bytes: u64,
    /// Memory available for new allocations without swapping.
    pub available_bytes: u64,
    /// `None` where PSI is unsupported or unreadable.
    pub pressure: Option<MemoryPressure>,
}

impl MemorySample {
    /// Available memory as a fraction of total, in `0.0..=1.0`.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // A ratio; precision loss above 2^52 bytes is irrelevant.
    pub fn available_ratio(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.available_bytes as f64 / self.total_bytes as f64
        }
    }
}

/// Reads system memory. Implementations may block briefly.
pub trait MemorySampler: Send + Sync {
    /// # Errors
    ///
    /// [`ProviderError::Unavailable`] when memory cannot be read.
    fn sample(&self) -> Result<MemorySample, ProviderError>;
}

/// Parses `/proc/pressure/memory`. Returns `None` for any unexpected format.
#[must_use]
pub fn parse_psi(text: &str) -> Option<MemoryPressure> {
    let field = |line: &str, key: &str| -> Option<f32> {
        line.split_whitespace()
            .find_map(|part| part.strip_prefix(key)?.strip_prefix('=')?.parse().ok())
    };
    let mut some = None;
    let mut full = None;
    for line in text.lines() {
        let values = (field(line, "avg10")?, field(line, "avg60")?);
        match line.split_whitespace().next()? {
            "some" => some = Some(values),
            "full" => full = Some(values),
            _ => {}
        }
    }
    let (some_avg10, some_avg60) = some?;
    let (full_avg10, full_avg60) = full?;
    Some(MemoryPressure {
        some_avg10,
        full_avg10,
        some_avg60,
        full_avg60,
    })
}

/// Samples with `sysinfo`, plus PSI on Linux.
#[derive(Debug)]
pub struct SystemMemorySampler {
    system: Mutex<sysinfo::System>,
}

impl SystemMemorySampler {
    #[must_use]
    pub fn new() -> Self {
        Self {
            system: Mutex::new(sysinfo::System::new()),
        }
    }
}

impl Default for SystemMemorySampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
fn read_pressure() -> Option<MemoryPressure> {
    std::fs::read_to_string("/proc/pressure/memory")
        .ok()
        .as_deref()
        .and_then(parse_psi)
}

#[cfg(not(target_os = "linux"))]
const fn read_pressure() -> Option<MemoryPressure> {
    None
}

impl MemorySampler for SystemMemorySampler {
    fn sample(&self) -> Result<MemorySample, ProviderError> {
        let mut system = self.system.lock().unwrap_or_else(PoisonError::into_inner);
        // Refreshes memory only, not processes: cheap enough for a 5-second cadence.
        system.refresh_memory();
        let total_bytes = system.total_memory();
        if total_bytes == 0 {
            return Err(ProviderError::Unavailable);
        }
        Ok(MemorySample {
            total_bytes,
            available_bytes: system.available_memory(),
            pressure: read_pressure(),
        })
    }
}

/// Plays back scripted samples, then repeats the last one.
#[derive(Debug)]
pub struct FakeMemorySampler {
    samples: Mutex<Vec<Result<MemorySample, ProviderError>>>,
}

impl FakeMemorySampler {
    #[must_use]
    pub fn new(samples: Vec<Result<MemorySample, ProviderError>>) -> Self {
        let mut samples = samples;
        samples.reverse();
        Self {
            samples: Mutex::new(samples),
        }
    }
}

impl MemorySampler for FakeMemorySampler {
    fn sample(&self) -> Result<MemorySample, ProviderError> {
        let mut samples = self.samples.lock().unwrap_or_else(PoisonError::into_inner);
        if samples.len() > 1 {
            samples.pop().unwrap_or(Err(ProviderError::Unavailable))
        } else {
            samples
                .last()
                .copied()
                .unwrap_or(Err(ProviderError::Unavailable))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PSI: &str = "some avg10=1.50 avg60=0.75 avg300=0.02 total=18935112\n\
                       full avg10=0.40 avg60=0.10 avg300=0.02 total=18436797\n";

    #[test]
    fn parses_psi() {
        let pressure = parse_psi(PSI).unwrap();
        assert!((pressure.some_avg10 - 1.5).abs() < f32::EPSILON);
        assert!((pressure.full_avg10 - 0.4).abs() < f32::EPSILON);
        assert!((pressure.some_avg60 - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn malformed_psi_is_none() {
        assert_eq!(parse_psi(""), None);
        assert_eq!(parse_psi("some avg10=1.0 avg60=x\n"), None);
        assert_eq!(
            parse_psi("some avg10=1.0 avg60=1.0 avg300=0 total=0\n"),
            None,
            "full line missing"
        );
    }

    #[test]
    fn ratio_handles_zero_total() {
        let sample = MemorySample {
            total_bytes: 0,
            available_bytes: 0,
            pressure: None,
        };
        assert!(sample.available_ratio().abs() < f64::EPSILON);
        let half = MemorySample {
            total_bytes: 8,
            available_bytes: 4,
            pressure: None,
        };
        assert!((half.available_ratio() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn system_sampler_reads_real_memory() {
        let sample = SystemMemorySampler::new().sample().unwrap();
        assert!(sample.total_bytes > 0);
        assert!(sample.available_bytes <= sample.total_bytes);
        #[cfg(target_os = "linux")]
        if std::path::Path::new("/proc/pressure/memory").exists() {
            assert!(sample.pressure.is_some());
        }
    }

    #[test]
    fn fake_plays_script_then_repeats_last() {
        let low = MemorySample {
            total_bytes: 10,
            available_bytes: 1,
            pressure: None,
        };
        let fake = FakeMemorySampler::new(vec![Err(ProviderError::Unavailable), Ok(low)]);
        assert_eq!(fake.sample(), Err(ProviderError::Unavailable));
        assert_eq!(fake.sample(), Ok(low));
        assert_eq!(fake.sample(), Ok(low));
    }
}
