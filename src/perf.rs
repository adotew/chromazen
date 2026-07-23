use std::time::{Duration, Instant};

const REPORT_INTERVAL: Duration = Duration::from_secs(5);
const MAX_SAMPLES: usize = 2_048;

#[derive(Debug)]
pub(crate) struct PaintPerf {
    enabled: bool,
    report_started: Instant,
    latest_input: Option<Instant>,
    input_events: u64,
    pressure_samples: u64,
    queued_stamps: u64,
    input_to_stamp: Vec<Duration>,
    input_to_submit: Vec<Duration>,
    input_to_present: Vec<Duration>,
}

impl Default for PaintPerf {
    fn default() -> Self {
        let enabled = std::env::var_os("CHROMAZEN_PERF").is_some_and(|value| value != "0");
        if enabled {
            log::info!(
                "paint performance probes enabled; reports are emitted every {} seconds",
                REPORT_INTERVAL.as_secs()
            );
        }
        Self {
            enabled,
            report_started: Instant::now(),
            latest_input: None,
            input_events: 0,
            pressure_samples: 0,
            queued_stamps: 0,
            input_to_stamp: Vec::with_capacity(MAX_SAMPLES),
            input_to_submit: Vec::with_capacity(MAX_SAMPLES),
            input_to_present: Vec::with_capacity(MAX_SAMPLES),
        }
    }
}

impl PaintPerf {
    pub(crate) fn input_received(&mut self) -> Option<Instant> {
        if !self.enabled {
            return None;
        }
        let now = Instant::now();
        self.latest_input = Some(now);
        self.input_events += 1;
        Some(now)
    }

    pub(crate) fn stamps_queued(
        &mut self,
        received_at: Option<Instant>,
        count: usize,
        pressure_sampled: bool,
    ) {
        if !self.enabled || count == 0 {
            return;
        }
        self.queued_stamps += count as u64;
        self.pressure_samples += u64::from(pressure_sampled);
        if let Some(received_at) = received_at {
            push_sample(&mut self.input_to_stamp, received_at.elapsed());
        }
    }

    pub(crate) fn submitted(&mut self) {
        if !self.enabled {
            return;
        }
        if let Some(received_at) = self.latest_input {
            push_sample(&mut self.input_to_submit, received_at.elapsed());
        }
    }

    pub(crate) fn presented(&mut self) {
        if !self.enabled {
            return;
        }
        if let Some(received_at) = self.latest_input.take() {
            push_sample(&mut self.input_to_present, received_at.elapsed());
        }
        self.report_if_due();
    }

    fn report_if_due(&mut self) {
        if self.report_started.elapsed() < REPORT_INTERVAL {
            return;
        }

        log::info!(
            "paint perf: inputs={} pressure_samples={} stamps={} input→stamp={} input→submit={} input→present={}",
            self.input_events,
            self.pressure_samples,
            self.queued_stamps,
            summary(&mut self.input_to_stamp),
            summary(&mut self.input_to_submit),
            summary(&mut self.input_to_present),
        );
        self.report_started = Instant::now();
        self.input_events = 0;
        self.pressure_samples = 0;
        self.queued_stamps = 0;
        self.input_to_stamp.clear();
        self.input_to_submit.clear();
        self.input_to_present.clear();
    }
}

fn push_sample(samples: &mut Vec<Duration>, sample: Duration) {
    if samples.len() == MAX_SAMPLES {
        samples.remove(0);
    }
    samples.push(sample);
}

fn summary(samples: &mut [Duration]) -> String {
    if samples.is_empty() {
        return "n/a".to_owned();
    }
    samples.sort_unstable();
    format!(
        "p50={:.2}ms p95={:.2}ms p99={:.2}ms n={}",
        percentile(samples, 50).as_secs_f64() * 1_000.0,
        percentile(samples, 95).as_secs_f64() * 1_000.0,
        percentile(samples, 99).as_secs_f64() * 1_000.0,
        samples.len(),
    )
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    let index = (samples.len() - 1) * percentile / 100;
    samples[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_uses_a_stable_nearest_rank() {
        let samples = [
            Duration::from_millis(1),
            Duration::from_millis(2),
            Duration::from_millis(3),
            Duration::from_millis(4),
        ];
        assert_eq!(percentile(&samples, 50), Duration::from_millis(2));
        assert_eq!(percentile(&samples, 95), Duration::from_millis(3));
        assert_eq!(percentile(&samples, 99), Duration::from_millis(3));
    }
}
