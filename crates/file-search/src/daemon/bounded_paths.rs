use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const CONSERVATIVE_PATH_ALLOCATION_BYTES: usize = 192;

#[derive(Debug)]
pub(super) struct BoundedPathSet {
    paths: BTreeSet<PathBuf>,
    estimated_bytes: usize,
    max_entries: usize,
    max_estimated_bytes: usize,
}

impl BoundedPathSet {
    pub(super) fn new(max_entries: usize, max_estimated_bytes: usize) -> Self {
        Self {
            paths: BTreeSet::new(),
            estimated_bytes: 0,
            max_entries,
            max_estimated_bytes,
        }
    }

    pub(super) fn insert(&mut self, path: PathBuf) -> Result<(), ()> {
        if self.paths.contains(path.as_path()) {
            return Ok(());
        }

        let path_estimated_bytes = estimated_path_bytes(&path);
        let next_estimated_bytes = self.estimated_bytes.saturating_add(path_estimated_bytes);
        if self.paths.len() >= self.max_entries || next_estimated_bytes > self.max_estimated_bytes {
            return Err(());
        }

        self.paths.insert(path);
        self.estimated_bytes = next_estimated_bytes;
        Ok(())
    }

    pub(super) fn clear(&mut self) {
        self.paths.clear();
        self.estimated_bytes = 0;
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    pub(super) fn remove(&mut self, path: &Path) -> bool {
        if !self.paths.remove(path) {
            return false;
        }
        self.estimated_bytes = self
            .estimated_bytes
            .saturating_sub(estimated_path_bytes(path));
        true
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &PathBuf> {
        self.paths.iter()
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&Path) -> bool) {
        let removed = self
            .paths
            .iter()
            .filter(|path| !keep(path))
            .cloned()
            .collect::<Vec<_>>();
        for path in removed {
            self.remove(&path);
        }
    }

    pub(super) fn take_paths(&mut self) -> Vec<PathBuf> {
        self.estimated_bytes = 0;
        std::mem::take(&mut self.paths).into_iter().collect()
    }

    pub(super) fn len(&self) -> usize {
        self.paths.len()
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        self.estimated_bytes
    }
}

pub(super) fn estimated_path_bytes(path: &Path) -> usize {
    path.as_os_str()
        .as_encoded_bytes()
        .len()
        .saturating_add(CONSERVATIVE_PATH_ALLOCATION_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_paths_do_not_consume_capacity_twice() {
        let path = PathBuf::from("/tmp/repeated");
        let mut paths = BoundedPathSet::new(1, estimated_path_bytes(&path));

        paths.insert(path.clone()).unwrap();
        paths.insert(path).unwrap();

        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn count_and_byte_limits_reject_without_growing_state() {
        let first = PathBuf::from("/tmp/first");
        let second = PathBuf::from("/tmp/second");
        let mut count_limited = BoundedPathSet::new(1, usize::MAX);
        count_limited.insert(first.clone()).unwrap();
        assert!(count_limited.insert(second.clone()).is_err());
        assert_eq!(count_limited.len(), 1);

        let mut byte_limited = BoundedPathSet::new(2, estimated_path_bytes(&first));
        byte_limited.insert(first).unwrap();
        assert!(byte_limited.insert(second).is_err());
        assert_eq!(byte_limited.len(), 1);
    }

    #[test]
    fn observed_home_scale_fits_the_realtime_path_budget() {
        let mut paths = BoundedPathSet::new(96_000, 32_000_000);
        for position in 0..78_710 {
            paths
                .insert(PathBuf::from(format!(
                    "/home/yuanming/work/project-{position:05}/target/debug/build/dependency-{position:05}/out/generated"
                )))
                .unwrap();
        }

        assert_eq!(paths.len(), 78_710);
        assert!(paths.estimated_bytes() <= 32_000_000);
    }

    #[test]
    fn removal_releases_the_exact_estimated_capacity() {
        let first = PathBuf::from("/tmp/first");
        let second = PathBuf::from("/tmp/second");
        let capacity = estimated_path_bytes(&first).max(estimated_path_bytes(&second));
        let mut paths = BoundedPathSet::new(1, capacity);

        paths.insert(first.clone()).unwrap();
        assert!(paths.contains(&first));
        assert!(paths.remove(&first));
        paths.insert(second.clone()).unwrap();

        assert!(!paths.contains(&first));
        assert!(paths.contains(&second));
    }
}
