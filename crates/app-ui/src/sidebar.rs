use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::{SidebarLocation, SidebarLocationKind};

pub(crate) const SIDEBAR_WIDTH: f32 = 180.0;

pub(crate) fn home_sidebar_location(home: &Path) -> SidebarLocation {
    SidebarLocation {
        label: "Home".to_owned(),
        path: home.to_path_buf(),
        kind: SidebarLocationKind::Home,
    }
}

pub(crate) fn sidebar_locations(home: &Path) -> Vec<SidebarLocation> {
    let mut locations = vec![home_sidebar_location(home)];

    for (label, kind, path) in default_user_directory_locations(home) {
        if path.is_dir() {
            push_sidebar_location(&mut locations, label, path, kind);
        }
    }

    for location in gtk_bookmark_locations(home) {
        push_sidebar_location(
            &mut locations,
            &location.label,
            location.path,
            SidebarLocationKind::Bookmark,
        );
    }

    locations
}

pub(crate) fn save_gtk_bookmark_locations(
    home: &Path,
    locations: &[SidebarLocation],
) -> io::Result<()> {
    let content = gtk_bookmarks_content(locations);
    for version in ["gtk-3.0", "gtk-4.0"] {
        let directory = home.join(".config").join(version);
        fs::create_dir_all(&directory)?;
        fs::write(directory.join("bookmarks"), &content)?;
    }
    Ok(())
}

fn default_user_directory_locations(
    home: &Path,
) -> [(&'static str, SidebarLocationKind, PathBuf); 6] {
    [
        (
            "Desktop",
            SidebarLocationKind::Desktop,
            dirs::desktop_dir().unwrap_or_else(|| home.join("Desktop")),
        ),
        (
            "Documents",
            SidebarLocationKind::Documents,
            dirs::document_dir().unwrap_or_else(|| home.join("Documents")),
        ),
        (
            "Downloads",
            SidebarLocationKind::Downloads,
            dirs::download_dir().unwrap_or_else(|| home.join("Downloads")),
        ),
        (
            "Pictures",
            SidebarLocationKind::Pictures,
            dirs::picture_dir().unwrap_or_else(|| home.join("Pictures")),
        ),
        (
            "Music",
            SidebarLocationKind::Music,
            dirs::audio_dir().unwrap_or_else(|| home.join("Music")),
        ),
        (
            "Videos",
            SidebarLocationKind::Videos,
            dirs::video_dir().unwrap_or_else(|| home.join("Videos")),
        ),
    ]
}

fn gtk_bookmark_locations(home: &Path) -> Vec<SidebarLocation> {
    let mut locations = Vec::new();

    for version in ["gtk-3.0", "gtk-4.0"] {
        let path = home.join(".config").join(version).join("bookmarks");
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };

        for line in content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            if let Some(location) = parse_gtk_bookmark(line) {
                push_sidebar_location(
                    &mut locations,
                    &location.label,
                    location.path,
                    SidebarLocationKind::Bookmark,
                );
            }
        }
    }

    locations
}

fn parse_gtk_bookmark(line: &str) -> Option<SidebarLocation> {
    let (uri, label) = line.split_once(' ').map_or((line, None), |(uri, label)| {
        (uri, Some(label.trim()).filter(|label| !label.is_empty()))
    });
    let raw_path = uri.strip_prefix("file://")?;
    let local_path = if raw_path.starts_with('/') {
        raw_path
    } else {
        raw_path.find('/').map(|index| &raw_path[index..])?
    };
    let path = PathBuf::from(percent_decode(local_path));
    let label = label
        .map(ToOwned::to_owned)
        .or_else(|| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    Some(SidebarLocation {
        label,
        path,
        kind: SidebarLocationKind::Bookmark,
    })
}

fn gtk_bookmarks_content(locations: &[SidebarLocation]) -> String {
    let mut content = locations
        .iter()
        .map(gtk_bookmark_line)
        .collect::<Vec<_>>()
        .join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    content
}

fn gtk_bookmark_line(location: &SidebarLocation) -> String {
    let uri = format!(
        "file://{}",
        percent_encode_path(&location.path.to_string_lossy())
    );
    let label = location.label.replace(['\n', '\r'], " ");
    if label.trim().is_empty() {
        uri
    } else {
        format!("{uri} {label}")
    }
}

fn percent_encode_path(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                output.push(*byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                output.push(high * 16 + low);
                index += 3;
                continue;
            }
        }

        output.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&output).into_owned()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn push_sidebar_location(
    locations: &mut Vec<SidebarLocation>,
    label: &str,
    path: PathBuf,
    kind: SidebarLocationKind,
) {
    if !locations.iter().any(|location| location.path == path) {
        locations.push(SidebarLocation {
            label: label.to_owned(),
            path,
            kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gtk_bookmark_label_and_encoded_path() {
        let bookmark = parse_gtk_bookmark("file:///home/user/My%20Folder Project Folder").unwrap();

        assert_eq!(bookmark.path, PathBuf::from("/home/user/My Folder"));
        assert_eq!(bookmark.label, "Project Folder");
        assert_eq!(bookmark.kind, SidebarLocationKind::Bookmark);
    }

    #[test]
    fn writes_gtk_bookmarks_to_both_versions() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let bookmarks = vec![SidebarLocation {
            label: "Project Folder".to_owned(),
            path: PathBuf::from("/home/user/My Folder"),
            kind: SidebarLocationKind::Bookmark,
        }];

        save_gtk_bookmark_locations(home, &bookmarks).unwrap();

        let expected = "file:///home/user/My%20Folder Project Folder\n";
        assert_eq!(
            fs::read_to_string(home.join(".config/gtk-3.0/bookmarks")).unwrap(),
            expected
        );
        assert_eq!(
            fs::read_to_string(home.join(".config/gtk-4.0/bookmarks")).unwrap(),
            expected
        );
    }
}
