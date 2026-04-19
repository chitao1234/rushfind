use crate::walker::PendingPath;

#[allow(dead_code)]
pub(crate) const DEFAULT_SPLIT_CHILD_THRESHOLD: usize = 32;
#[allow(dead_code)]
pub(crate) const DEFAULT_SPILL_CHUNK_SIZE: usize = 32;

#[derive(Debug)]
pub(crate) struct ChunkPlan {
    pub(crate) local_stack: Vec<PendingPath>,
    pub(crate) spilled_chunks: Vec<Vec<PendingPath>>,
}

#[derive(Debug)]
pub(crate) struct ChunkAccumulator {
    split_threshold: usize,
    chunk_size: usize,
    local_stack: Vec<PendingPath>,
    spill_buffer: Vec<PendingPath>,
    spilled_chunks: Vec<Vec<PendingPath>>,
}

impl ChunkAccumulator {
    pub(crate) fn new(split_threshold: usize, chunk_size: usize) -> Self {
        Self {
            split_threshold,
            chunk_size: chunk_size.max(1),
            local_stack: Vec::new(),
            spill_buffer: Vec::new(),
            spilled_chunks: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, child: PendingPath, may_split: bool) {
        if !may_split || self.local_stack.len() < self.split_threshold {
            self.local_stack.push(child);
            return;
        }

        self.spill_buffer.push(child);
        if self.spill_buffer.len() >= self.chunk_size {
            self.spilled_chunks
                .push(std::mem::take(&mut self.spill_buffer));
        }
    }

    pub(crate) fn observe_quit(&mut self) {
        for chunk in self.spilled_chunks.drain(..) {
            self.local_stack.extend(chunk);
        }
        self.local_stack.append(&mut self.spill_buffer);
    }

    pub(crate) fn take_spilled_chunks(&mut self) -> Vec<Vec<PendingPath>> {
        std::mem::take(&mut self.spilled_chunks)
    }

    pub(crate) fn finish(mut self) -> ChunkPlan {
        if !self.spill_buffer.is_empty() {
            self.spilled_chunks
                .push(std::mem::take(&mut self.spill_buffer));
        }

        ChunkPlan {
            local_stack: self.local_stack,
            spilled_chunks: self.spilled_chunks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ChunkAccumulator, ChunkPlan};
    use crate::walker::PendingPath;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn pending_child(index: usize) -> PendingPath {
        let path = PathBuf::from(format!("child-{index:02}"));
        PendingPath {
            path,
            root_path: Arc::new(PathBuf::from("root")),
            depth: 1,
            is_command_line_root: false,
            physical_file_type_hint: None,
            ancestry: Vec::new(),
            ancestor_barriers: Vec::new(),
            root_device: None,
            parent_completion: None,
        }
    }

    #[test]
    fn accumulator_keeps_small_directories_local() {
        let mut accumulator = ChunkAccumulator::new(4, 2);
        for index in 0..3 {
            accumulator.push(pending_child(index), true);
        }

        let ChunkPlan {
            local_stack,
            spilled_chunks,
        } = accumulator.finish();
        assert_eq!(local_stack.len(), 3);
        assert!(spilled_chunks.is_empty());
    }

    #[test]
    fn accumulator_emits_fixed_size_chunks_after_split_threshold() {
        let mut accumulator = ChunkAccumulator::new(4, 2);
        for index in 0..7 {
            accumulator.push(pending_child(index), true);
        }

        let ChunkPlan {
            local_stack,
            spilled_chunks,
        } = accumulator.finish();
        assert_eq!(local_stack.len(), 4);
        assert_eq!(
            spilled_chunks
                .iter()
                .map(|chunk| chunk.len())
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
    }

    #[test]
    fn accumulator_drains_buffer_locally_after_quit() {
        let mut accumulator = ChunkAccumulator::new(4, 2);
        for index in 0..6 {
            accumulator.push(pending_child(index), true);
        }

        accumulator.observe_quit();
        accumulator.push(pending_child(6), false);

        let ChunkPlan {
            local_stack,
            spilled_chunks,
        } = accumulator.finish();
        assert!(spilled_chunks.is_empty());
        assert_eq!(local_stack.len(), 7);
    }
}
