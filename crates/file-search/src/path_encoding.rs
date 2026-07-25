use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
#[serde(tag = "encoding", rename_all = "snake_case")]
enum WirePath {
    #[cfg(unix)]
    UnixBytes { bytes: Vec<u8> },
    #[cfg(windows)]
    WindowsWide { units: Vec<u16> },
    #[cfg(not(any(unix, windows)))]
    EncodedBytes { bytes: Vec<u8> },
}

pub(crate) fn storage_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    #[cfg(not(any(unix, windows)))]
    {
        path.as_os_str().as_encoded_bytes().to_vec()
    }
}

pub(crate) fn path_from_storage(bytes: Vec<u8>) -> PathBuf {
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(OsString::from_vec(bytes))
    }

    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        assert!(bytes.len().is_multiple_of(2));
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        PathBuf::from(OsString::from_wide(&units))
    }

    #[cfg(not(any(unix, windows)))]
    {
        // 数据库与严格版本握手的客户端只在同一目标读取该平台编码。
        let value = unsafe { std::ffi::OsString::from_encoded_bytes_unchecked(bytes) };
        PathBuf::from(value)
    }
}

pub(crate) mod serde_path {
    use super::*;

    pub(crate) fn serialize<S>(path: &Path, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[cfg(unix)]
        let encoded = WirePath::UnixBytes {
            bytes: storage_bytes(path),
        };
        #[cfg(windows)]
        let encoded = {
            use std::os::windows::ffi::OsStrExt;
            WirePath::WindowsWide {
                units: path.as_os_str().encode_wide().collect(),
            }
        };
        #[cfg(not(any(unix, windows)))]
        let encoded = WirePath::EncodedBytes {
            bytes: storage_bytes(path),
        };
        encoded.serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
    where
        D: Deserializer<'de>,
    {
        match WirePath::deserialize(deserializer)? {
            #[cfg(unix)]
            WirePath::UnixBytes { bytes } => Ok(path_from_storage(bytes)),
            #[cfg(windows)]
            WirePath::WindowsWide { units } => {
                use std::ffi::OsString;
                use std::os::windows::ffi::OsStringExt;
                Ok(PathBuf::from(OsString::from_wide(&units)))
            }
            #[cfg(not(any(unix, windows)))]
            WirePath::EncodedBytes { bytes } => Ok(path_from_storage(bytes)),
        }
    }
}
