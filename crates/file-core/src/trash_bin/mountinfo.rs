use std::path::PathBuf;

use crate::mount_table::{parse_mount_table, PROC_SELF_MOUNTINFO};
use crate::ScanWarning;

pub(super) const MOUNTINFO_PATH: &str = PROC_SELF_MOUNTINFO;

#[derive(Debug)]
pub(super) struct MountInfoSnapshot {
    pub mount_points: Vec<PathBuf>,
    pub warnings: Vec<ScanWarning>,
}

pub(super) fn parse_mountinfo(content: &[u8]) -> MountInfoSnapshot {
    let snapshot = parse_mount_table(content);
    MountInfoSnapshot {
        mount_points: snapshot.mount_points,
        warnings: snapshot
            .warnings
            .into_iter()
            .map(|warning| ScanWarning {
                path: PathBuf::from(MOUNTINFO_PATH),
                message: format!("line {}: {}", warning.line_number, warning.message),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn trash_snapshot_preserves_shared_parser_warnings() {
        let snapshot = parse_mountinfo(
            b"broken\n\
              2 1 8:2 / relative rw - ext4 /dev/sdb rw\n\
              3 1 8:3 / /bad\\999 rw - ext4 /dev/sdc rw\n\
              4 1 8:4 / /valid rw - ext4 /dev/sdd rw\n",
        );

        assert_eq!(snapshot.mount_points, vec![PathBuf::from("/valid")]);
        assert_eq!(snapshot.warnings.len(), 3);
        assert!(snapshot
            .warnings
            .iter()
            .all(|warning| warning.path == Path::new(MOUNTINFO_PATH)));
    }
}
