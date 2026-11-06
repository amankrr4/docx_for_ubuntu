use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_ZIP_ENTRIES: usize = 10_000;
pub const MAX_COMPRESSION_RATIO: u64 = 200;
pub const MAX_XML_PART_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_TOTAL_DECOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_XML_NODES: u64 = 2_000_000;
pub const MAX_DEPTH: u32 = 128;
pub const MAX_TABLE_NESTING: u32 = 5;
pub const MAX_LIST_LEVELS: u8 = 9;
pub const MAX_STYLE_CHAIN: u32 = 32;
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_TOTAL_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_IMAGE_PIXELS: u64 = 32_000_000;
pub const MAX_TOTAL_IMAGE_PIXELS: u64 = 128_000_000;
pub const MAX_IMAGE_DIMENSION: u32 = 4096;
pub const MAX_RENDERED_IMAGES: u32 = 500;
pub const MAX_TABLE_CELLS: u64 = 20_000;
pub const MAX_WIDGETS: u64 = 5_000;
pub const MAX_TABULAR_CELL_CHARS: usize = 80;
pub const PARSE_BUDGET: Duration = Duration::from_secs(10);
const DEADLINE_CHECK_INTERVAL: u32 = 1024;

#[derive(Clone, Debug)]
pub struct Budget {
    pub cancel: Arc<AtomicBool>,
    start: Instant,
    deadline: Duration,
    checks: u32,
    pub nodes: u64,
    pub decompressed_bytes: u64,
    pub image_bytes: u64,
    pub image_pixels: u64,
    pub table_cells: u64,
    pub widgets: u64,
    pub truncated: bool,
    exhausted: bool,
}

impl Budget {
    pub fn new(cancel: Arc<AtomicBool>) -> Self {
        Self {
            cancel,
            start: Instant::now(),
            deadline: PARSE_BUDGET,
            checks: 0,
            nodes: 0,
            decompressed_bytes: 0,
            image_bytes: 0,
            image_pixels: 0,
            table_cells: 0,
            widgets: 0,
            truncated: false,
            exhausted: false,
        }
    }

    pub fn tick(&mut self) -> bool {
        if self.exhausted {
            return false;
        }
        if self.cancel.load(Ordering::Relaxed) {
            self.exhausted = true;
            self.truncated = true;
            return false;
        }
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > MAX_XML_NODES {
            self.exhausted = true;
            self.truncated = true;
            return false;
        }
        self.checks = self.checks.saturating_add(1);
        if self.checks >= DEADLINE_CHECK_INTERVAL {
            self.checks = 0;
            if self.start.elapsed() > self.deadline {
                self.exhausted = true;
                self.truncated = true;
                return false;
            }
        }
        true
    }

    pub fn add_decompressed(&mut self, bytes: u64) -> bool {
        self.decompressed_bytes = self.decompressed_bytes.saturating_add(bytes);
        if self.decompressed_bytes > MAX_TOTAL_DECOMPRESSED_BYTES {
            self.exhausted = true;
            return false;
        }
        true
    }

    pub fn add_image(&mut self, bytes: u64) -> bool {
        self.image_bytes = self.image_bytes.saturating_add(bytes);
        if self.image_bytes > MAX_TOTAL_IMAGE_BYTES {
            self.exhausted = true;
            return false;
        }
        true
    }

    pub fn add_image_pixels(&mut self, pixels: u64) -> bool {
        self.image_pixels = self.image_pixels.saturating_add(pixels);
        if self.image_pixels > MAX_TOTAL_IMAGE_PIXELS {
            self.exhausted = true;
            return false;
        }
        true
    }

    pub fn add_table_cell(&mut self) -> bool {
        self.table_cells = self.table_cells.saturating_add(1);
        if self.table_cells > MAX_TABLE_CELLS {
            self.exhausted = true;
            self.truncated = true;
            return false;
        }
        true
    }

    pub fn add_widget(&mut self) -> bool {
        self.widgets = self.widgets.saturating_add(1);
        if self.widgets > MAX_WIDGETS {
            self.exhausted = true;
            self.truncated = true;
            return false;
        }
        true
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}
