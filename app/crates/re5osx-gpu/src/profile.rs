//! GPU time per pass, for finding what to optimise (`RE5OSX_GPU_PROFILE=1`):
//! timestamps around every pass, resolved with each submit, summed by label
//! (a draw pass is named after the first shader it draws with) and logged
//! with the statistics.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Passes timed per submit, at most.
const PAIRS: u32 = 1024;

pub struct Profiler {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    /// The label of each pass timed in the commands being recorded.
    labels: Vec<Arc<str>>,
    /// Nanoseconds per timestamp tick.
    period: f32,
    pub totals: Arc<Mutex<Totals>>,
}

#[derive(Default)]
pub struct Totals {
    /// Label: (passes, nanoseconds).
    pub by_label: HashMap<Arc<str>, (u64, f64)>,
    pub ns: f64,
}

impl Profiler {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Profiler {
        Profiler {
            set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("pass timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: PAIRS * 2,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pass timestamps"),
                size: PAIRS as u64 * 16,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            labels: Vec::new(),
            period: queue.get_timestamp_period(),
            totals: Arc::default(),
        }
    }

    /// A pair of timestamps for the next pass (`None`: none left until the
    /// next submit).
    pub fn next(&mut self, label: impl Into<Arc<str>>) -> Option<u32> {
        let i = self.labels.len() as u32;
        if i >= PAIRS {
            return None;
        }
        self.labels.push(label.into());
        Some(i)
    }

    /// Bytes the passes timed so far resolve to.
    pub fn pending_bytes(&self) -> u64 {
        self.labels.len() as u64 * 16
    }

    /// Names the pass timed last (a draw pass, after its first draw).
    pub fn rename(&mut self, i: u32, label: &str) {
        if let Some(l) = self.labels.get_mut(i as usize) {
            *l = label.into();
        }
    }

    pub fn render(&self, i: u32) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(i * 2),
            end_of_pass_write_index: Some(i * 2 + 1),
        }
    }

    pub fn compute(&self, i: u32) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(i * 2),
            end_of_pass_write_index: Some(i * 2 + 1),
        }
    }

    /// Resolves the passes timed so far into `staging`; the returned work
    /// adds them up once the GPU is done.
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder, staging: &wgpu::Buffer) -> Option<Resolved> {
        if self.labels.is_empty() {
            return None;
        }
        let n = self.labels.len() as u32;
        encoder.resolve_query_set(&self.set, 0..n * 2, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, staging, 0, n as u64 * 16);
        Some(Resolved { labels: std::mem::take(&mut self.labels), period: self.period, totals: self.totals.clone() })
    }
}

/// Timestamps on their way back.
pub struct Resolved {
    labels: Vec<Arc<str>>,
    period: f32,
    totals: Arc<Mutex<Totals>>,
}

impl Resolved {
    pub fn add(self, data: &[u8]) {
        let mut totals = self.totals.lock().unwrap();
        for (i, label) in self.labels.into_iter().enumerate() {
            let at = |k: usize| u64::from_le_bytes(data[k * 8..k * 8 + 8].try_into().unwrap());
            let (begin, end) = (at(i * 2), at(i * 2 + 1));
            if end <= begin || begin == 0 {
                continue;
            }
            let ns = (end - begin) as f64 * self.period as f64;
            totals.ns += ns;
            let e = totals.by_label.entry(label).or_default();
            e.0 += 1;
            e.1 += ns;
        }
    }
}

/// The heaviest labels: "total ms/frame: label ms (passes), ...".
pub fn report(totals: &Arc<Mutex<Totals>>, frames: u64) -> String {
    let t = std::mem::take(&mut *totals.lock().unwrap());
    let frames = frames.max(1) as f64;
    let mut by: Vec<_> = t.by_label.into_iter().collect();
    by.sort_by(|a, b| b.1.1.total_cmp(&a.1.1));
    let top: Vec<String> = by
        .iter()
        .take(16)
        .map(|(l, (n, ns))| format!("{l} {:.2} ({:.1})", ns / 1e6 / frames, *n as f64 / frames))
        .collect();
    format!("gpu time {:.2} ms/frame: {}", t.ns / 1e6 / frames, top.join(", "))
}
