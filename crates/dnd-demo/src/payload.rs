use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub struct DragPayload {
    pub text_uri_list: String,
    pub gnome_copied_files: String,
    pub plain_text: String,
}

impl DragPayload {
    pub fn new(paths: &[PathBuf]) -> Self {
        let uris = paths.iter().map(|path| file_uri(path)).collect::<Vec<_>>();
        let text_uri_list = format!("{}\r\n", uris.join("\r\n"));

        Self {
            gnome_copied_files: format!("cut\n{}\n", uris.join("\n")),
            plain_text: text_uri_list.clone(),
            text_uri_list,
        }
    }

    pub fn for_mime(&self, mime: &str) -> Option<&str> {
        match mime {
            "text/uri-list" => Some(&self.text_uri_list),
            "x-special/gnome-copied-files" => Some(&self.gnome_copied_files),
            "text/plain;charset=utf-8" | "UTF8_STRING" | "text/plain" => Some(&self.plain_text),
            _ => None,
        }
    }
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");

    for byte in path.as_os_str().as_bytes() {
        if is_uri_path_byte(*byte) {
            uri.push(*byte as char);
        } else {
            uri.push('%');
            uri.push(hex_digit(byte >> 4));
            uri.push(hex_digit(byte & 0x0f));
        }
    }

    uri
}

fn is_uri_path_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~'
    )
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        _ => (b'A' + value - 10) as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_keeps_path_separators_and_encodes_other_bytes() {
        let uri = file_uri(Path::new("/tmp/a b/%/文件.txt"));

        assert_eq!(uri, "file:///tmp/a%20b/%25/%E6%96%87%E4%BB%B6.txt");
    }

    #[test]
    fn drag_payload_uses_uri_list_and_gnome_move_contract() {
        let paths = vec![
            PathBuf::from("/tmp/source one"),
            PathBuf::from("/tmp/source-two"),
        ];

        let payload = DragPayload::new(&paths);

        assert_eq!(
            payload.text_uri_list,
            "file:///tmp/source%20one\r\nfile:///tmp/source-two\r\n"
        );
        assert_eq!(
            payload.gnome_copied_files,
            "cut\nfile:///tmp/source%20one\nfile:///tmp/source-two\n"
        );
    }
}
