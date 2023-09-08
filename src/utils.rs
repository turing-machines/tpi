use tokio::time::Instant;

pub const PROGRESS_REPORT_PERCENT: u64 = 5;
pub struct ProgressPrinter {
    total_size: u64,
    total_read: u64,
    start_time: Instant,
    last_printed: u64,
    threshold: u64,
}

impl ProgressPrinter {
    pub fn new(total_size: u64, start_time: Instant, print_threshold_percent: u64) -> Self {
        Self {
            total_size,
            total_read: 0,
            start_time,
            last_printed: 0,
            threshold: print_threshold_percent * total_size / 100,
        }
    }

    pub fn update_progress(&mut self, chunk: usize) {
        self.total_read += chunk as u64;
        self.try_print_progress()
    }

    fn try_print_progress(&mut self) {
        if self.total_read - self.last_printed < self.threshold {
            return;
        }

        #[allow(clippy::cast_precision_loss)] // This affects files > 4 exabytes long
        let read_proportion = (self.total_read as f64) / (self.total_size as f64);
        let read_percent = 100 * self.total_read / self.total_size;

        let duration = self.start_time.elapsed();
        let estimated_end = duration.div_f64(read_proportion);
        let estimated_left = estimated_end - duration;

        let est_seconds = estimated_left.as_secs() % 60;
        let est_minutes = (estimated_left.as_secs() / 60) % 60;

        println!(
            "Progress: {:>2}%, estimated time left: {:02}:{:02}",
            read_percent, est_minutes, est_seconds,
        );

        self.last_printed = self.total_read;
    }
}
