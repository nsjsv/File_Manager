use std::fs;
use std::path::{Path, PathBuf};

use crate::model::SidebarLocation;

pub(crate) const SIDEBAR_WIDTH: f32 = 180.0;

pub(crate) fn home_sidebar_location(home: &Path) -> SidebarLocation {
    SidebarLocation {
        label: "Home".to_owned(),
        path: home.to_path_buf(),
    }
}

pub(crate) fn sidebar_locations(home: &Path) -> Vec<SidebarLocation> {
    let mut locations = vec![home_sidebar_location(home)];

    for label in [
        "Desktop",
        "Documents",
        "Downloads",
        "Pictures",
        "Music",
        "Videos",
    ] {
        let path = home.join(label);
        if path.is_dir() {
            push_sidebar_location(&mut locations, label, path);
        }
    }

    for location in gtk_bookmark_locations(home) {
        push_sidebar_location(&mut locations, &location.label, location.path);
    }

    locations
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
                push_sidebar_location(&mut locations, &location.label, location.path);
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

    Some(SidebarLocation { label, path })
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

fn push_sidebar_location(locations: &mut Vec<SidebarLocation>, label: &str, path: PathBuf) {
    if !locations.iter().any(|location| location.path == path) {
        locations.push(SidebarLocation {
            label: label.to_owned(),
            path,
        });
    }
}
