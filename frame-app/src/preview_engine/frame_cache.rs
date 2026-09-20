use std::collections::VecDeque;

use super::PreviewRenderedFrame;

pub const DEFAULT_PREVIEW_FRAME_CACHE_CAPACITY: usize = 24;

/// A cached preview frame rendered for a given request timestamp.
#[derive(Clone, Debug)]
pub struct CachedPreviewFrame {
    pub request_seconds: f64,
    pub precise: bool,
    pub frame: PreviewRenderedFrame,
}

/// An in-memory LRU cache for rendered preview frames.
///
/// Reduces repetitive `FFmpeg` process invocations when scrubbing or trimming video.
#[derive(Debug)]
pub struct PreviewFrameCache {
    capacity: usize,
    entries: VecDeque<CachedPreviewFrame>,
}

impl Default for PreviewFrameCache {
    fn default() -> Self {
        Self::new(DEFAULT_PREVIEW_FRAME_CACHE_CAPACITY)
    }
}

impl PreviewFrameCache {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::with_capacity(capacity.max(1)),
        }
    }

    /// Looks up a rendered frame matching the target `seconds`.
    ///
    /// - If `precise` is true, only entries rendered with `precise = true` will match,
    ///   and the timestamp difference must be within the frame tolerance (`0.45 / fps`).
    /// - If `precise` is false, any entry within tolerance can match, preferring the closest.
    ///
    /// If found, the entry is promoted to the front of the LRU queue.
    pub fn get(&mut self, seconds: f64, precise: bool, fps: u32) -> Option<PreviewRenderedFrame> {
        let tolerance = if fps > 0 {
            (0.45 / f64::from(fps)).max(0.005)
        } else {
            0.015
        };

        let mut best_idx: Option<usize> = None;
        let mut best_distance = f64::MAX;
        let mut best_is_precise = false;

        for (idx, entry) in self.entries.iter().enumerate() {
            if precise && !entry.precise {
                continue;
            }
            let dist = (entry.request_seconds - seconds).abs();
            if dist <= tolerance {
                let is_better = match best_idx {
                    None => true,
                    Some(_) => {
                        if !precise && entry.precise && !best_is_precise {
                            true
                        } else if !precise && !entry.precise && best_is_precise {
                            false
                        } else {
                            dist < best_distance
                        }
                    }
                };

                if is_better {
                    best_idx = Some(idx);
                    best_distance = dist;
                    best_is_precise = entry.precise;
                }
            }
        }

        if let Some(idx) = best_idx {
            let entry = self.entries.remove(idx)?;
            let frame = entry.frame.clone();
            self.entries.push_front(entry);
            Some(frame)
        } else {
            None
        }
    }

    /// Inserts a new rendered frame into the cache.
    pub fn insert(&mut self, seconds: f64, precise: bool, frame: PreviewRenderedFrame) {
        if let Some(existing_idx) = self
            .entries
            .iter()
            .position(|e| (e.request_seconds - seconds).abs() < 0.001)
        {
            let existing = &mut self.entries[existing_idx];
            if precise || !existing.precise {
                existing.precise = precise;
                existing.frame = frame;
            }
            let entry = self.entries.remove(existing_idx).expect("entry must exist");
            self.entries.push_front(entry);
            return;
        }

        if self.entries.len() >= self.capacity {
            self.entries.pop_back();
        }

        self.entries.push_front(CachedPreviewFrame {
            request_seconds: seconds,
            precise,
            frame,
        });
    }

    /// Clears all cached frames.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::RenderImage;
    use smallvec::SmallVec;

    use super::*;

    fn dummy_rendered_frame(timestamp_us: u64) -> PreviewRenderedFrame {
        let image = image::RgbaImage::new(640, 360);
        let mut frames = SmallVec::<[image::Frame; 1]>::new();
        frames.push(image::Frame::new(image));
        PreviewRenderedFrame::new(
            640,
            360,
            timestamp_us,
            640 * 360 * 4,
            Arc::new(RenderImage::new(frames)),
        )
    }

    #[test]
    fn test_cache_hit_and_lru_ordering() {
        let mut cache = PreviewFrameCache::new(3);
        let frame1 = dummy_rendered_frame(1_000_000);
        let frame2 = dummy_rendered_frame(2_000_000);
        let frame3 = dummy_rendered_frame(3_000_000);

        cache.insert(1.0, true, frame1);
        cache.insert(2.0, true, frame2);
        cache.insert(3.0, true, frame3);

        assert_eq!(cache.len(), 3);

        // Access 1.0, promoting it to front
        let hit = cache.get(1.0, true, 30);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().timestamp_us, 1_000_000);

        // Insert a 4th frame, which should evict the least recently used (2.0)
        let frame4 = dummy_rendered_frame(4_000_000);
        cache.insert(4.0, true, frame4);

        assert_eq!(cache.len(), 3);
        // 2.0 should have been evicted
        assert!(cache.get(2.0, true, 30).is_none());
        // 1.0, 3.0, 4.0 should still exist
        assert!(cache.get(1.0, true, 30).is_some());
        assert!(cache.get(3.0, true, 30).is_some());
        assert!(cache.get(4.0, true, 30).is_some());
    }

    #[test]
    fn test_tolerance_and_fps() {
        let mut cache = PreviewFrameCache::new(5);
        let frame = dummy_rendered_frame(1_000_000);
        cache.insert(1.0, false, frame);

        // At 30fps, tolerance is 0.45 / 30 = 0.015s (15ms)
        // 1.010 is within 15ms -> HIT
        assert!(cache.get(1.010, false, 30).is_some());
        // 1.020 is outside 15ms -> MISS
        assert!(cache.get(1.020, false, 30).is_none());
    }

    #[test]
    fn test_precise_vs_fast_matching() {
        let mut cache = PreviewFrameCache::new(5);
        let fast_frame = dummy_rendered_frame(1_000_000);
        cache.insert(1.0, false, fast_frame);

        // Fast request matches fast frame
        assert!(cache.get(1.0, false, 30).is_some());
        // Precise request does NOT match fast frame
        assert!(cache.get(1.0, true, 30).is_none());

        // Now insert a precise frame at 2.0
        let precise_frame = dummy_rendered_frame(2_000_000);
        cache.insert(2.0, true, precise_frame);

        // Precise request matches precise frame
        assert!(cache.get(2.0, true, 30).is_some());
        // Fast request can also match precise frame
        assert!(cache.get(2.0, false, 30).is_some());
    }

    #[test]
    fn test_clear() {
        let mut cache = PreviewFrameCache::new(5);
        cache.insert(1.0, true, dummy_rendered_frame(1_000_000));
        assert_eq!(cache.len(), 1);
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }
}
