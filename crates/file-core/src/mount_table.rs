use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

pub const PROC_SELF_MOUNTINFO: &str = "/proc/self/mountinfo";

#[derive(Debug, Eq, PartialEq)]
pub struct MountTableWarning {
    pub line_number: usize,
    pub message: &'static str,
}

#[derive(Debug, Eq, PartialEq)]
pub struct MountTableSnapshot {
    pub mount_points: Vec<PathBuf>,
    pub warnings: Vec<MountTableWarning>,
}

pub fn parse_mount_table(content: &[u8]) -> MountTableSnapshot {
    let mut mount_points = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();

    for (line_index, line) in content.split(|byte| *byte == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let fields = line
            .split(|byte| *byte == b' ')
            .filter(|field| !field.is_empty())
            .collect::<Vec<_>>();
        let Some(separator_index) = fields.iter().position(|field| *field == b"-") else {
            warnings.push(mount_table_warning(line_index, "invalid mountinfo record"));
            continue;
        };
        if fields.len() < 10 || separator_index < 6 || fields.len() < separator_index + 4 {
            warnings.push(mount_table_warning(line_index, "invalid mountinfo record"));
            continue;
        }

        let decoded = match decode_mount_path(fields[4]) {
            Ok(decoded) => decoded,
            Err(message) => {
                warnings.push(mount_table_warning(line_index, message));
                continue;
            }
        };
        let mount_point = path_from_bytes(decoded);
        if !mount_point.is_absolute() {
            warnings.push(mount_table_warning(
                line_index,
                "mount point is not an absolute path",
            ));
            continue;
        }
        if seen.insert(path_bytes(&mount_point)) {
            mount_points.push(mount_point);
        }
    }

    MountTableSnapshot {
        mount_points,
        warnings,
    }
}

fn decode_mount_path(encoded: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut index = 0;
    while index < encoded.len() {
        if encoded[index] != b'\\' {
            if encoded[index] == 0 {
                return Err("mount point contains a NUL byte");
            }
            decoded.push(encoded[index]);
            index += 1;
            continue;
        }
        let escape = encoded
            .get(index + 1..index + 4)
            .ok_or("mount point has a truncated escape")?;
        let value = match escape {
            b"040" => b' ',
            b"011" => b'\t',
            b"012" => b'\n',
            b"134" => b'\\',
            _ => return Err("mount point has an unknown escape"),
        };
        decoded.push(value);
        index += 4;
    }
    Ok(decoded)
}

fn mount_table_warning(line_index: usize, message: &'static str) -> MountTableWarning {
    MountTableWarning {
        line_number: line_index + 1,
        message,
    }
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().to_string_lossy().as_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_preserves_bytes_order_and_distinct_mounts() {
        let snapshot = parse_mount_table(
            b"1 0 8:1 / / rw - ext4 /dev/root rw\n\
              2 1 8:2 / /media/My\\040Disk rw - ext4 /dev/sdb rw\n\
              3 1 8:2 / /media/My\\040Disk rw - ext4 /dev/sdb rw\n\
              4 1 8:3 / /media/tab\\011line\\012slash\\134 rw - ext4 /dev/sdc rw\n",
        );

        assert_eq!(
            snapshot.mount_points,
            vec![
                PathBuf::from("/"),
                PathBuf::from("/media/My Disk"),
                PathBuf::from("/media/tab\tline\nslash\\"),
            ]
        );
        assert!(snapshot.warnings.is_empty());
    }

    #[test]
    fn malformed_records_are_reported_without_hiding_valid_mounts() {
        let snapshot = parse_mount_table(
            b"broken\n\
              2 1 8:2 / relative rw - ext4 /dev/sdb rw\n\
              3 1 8:3 / /bad\\999 rw - ext4 /dev/sdc rw\n\
              4 1 8:4 / /valid rw - ext4 /dev/sdd rw\n",
        );

        assert_eq!(snapshot.mount_points, vec![PathBuf::from("/valid")]);
        assert_eq!(snapshot.warnings.len(), 3);
        assert_eq!(snapshot.warnings[0].line_number, 1);
    }
}
