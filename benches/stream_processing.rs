use std::hint::black_box;
use std::io::Read;

use criterion::{criterion_group, criterion_main, Criterion};

use tapcue::notifier::{FailureNotification, Notifier};
use tapcue::{process_stream, AppConfig};

#[derive(Default)]
struct NullBenchNotifier;

impl Notifier for NullBenchNotifier {
    fn notify_failure(&mut self, _failure: &FailureNotification) {}

    fn notify_bailout(&mut self, _reason: &str) {}

    fn notify_summary(&mut self, _state: &tapcue::processor::RunState) {}
}

struct TinyChunkReader {
    data: Vec<u8>,
    cursor: usize,
    max_chunk: usize,
}

impl TinyChunkReader {
    fn new(data: Vec<u8>, max_chunk: usize) -> Self {
        Self { data, cursor: 0, max_chunk }
    }
}

impl Read for TinyChunkReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cursor >= self.data.len() {
            return Ok(0);
        }

        let count = self.max_chunk.min(buf.len()).min(self.data.len().saturating_sub(self.cursor));
        let next = self.cursor + count;
        buf[..count].copy_from_slice(&self.data[self.cursor..next]);
        self.cursor = next;
        Ok(count)
    }
}

fn large_tap_document(count: usize) -> Vec<u8> {
    let mut input = String::with_capacity(32 * count);
    input.push_str("TAP version 14\n");
    input.push_str(&format!("1..{count}\n"));
    for index in 1..=count {
        input.push_str(&format!("ok {index} - benchmark {index}\n"));
    }
    input.into_bytes()
}

fn bench_stream_processing(c: &mut Criterion) {
    let payload = large_tap_document(10_000);

    c.bench_function("stream_processing_10k_tiny_chunks", |b| {
        b.iter(|| {
            let reader = TinyChunkReader::new(payload.clone(), 1);
            let mut notifier = NullBenchNotifier;
            let state = process_stream(reader, &mut notifier, AppConfig::default())
                .expect("benchmark stream should parse");
            black_box(state.total)
        });
    });
}

criterion_group!(benches, bench_stream_processing);
criterion_main!(benches);
