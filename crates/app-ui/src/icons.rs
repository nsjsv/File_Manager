use std::ffi::OsStr;
use std::path::Path;

use file_core::FileKind;
use iced::widget::{svg, Svg};
use iced::{Length, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IconSymbol {
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ChevronRight,
    Close,
    Copy,
    File,
    FileArchive,
    FileCode,
    FileImage,
    FileText,
    Folder,
    Link,
    Pencil,
    Settings,
    Trash,
    TriangleAlert,
}

pub(crate) fn file_entry_icon_symbol(kind: FileKind, name: &OsStr) -> IconSymbol {
    file_kind_icon_symbol(kind, Path::new(name).extension().and_then(OsStr::to_str))
}

pub(crate) fn preview_entry_icon_symbol(kind: FileKind, name: &str) -> IconSymbol {
    file_kind_icon_symbol(kind, Path::new(name).extension().and_then(OsStr::to_str))
}

fn file_kind_icon_symbol(kind: FileKind, extension: Option<&str>) -> IconSymbol {
    match kind {
        FileKind::Directory => IconSymbol::Folder,
        FileKind::Symlink => IconSymbol::Link,
        FileKind::Other => IconSymbol::File,
        FileKind::File => file_extension_icon_symbol(extension),
    }
}

fn file_extension_icon_symbol(extension: Option<&str>) -> IconSymbol {
    let Some(extension) = extension else {
        return IconSymbol::File;
    };

    if is_image_extension(extension) {
        IconSymbol::FileImage
    } else if is_archive_extension(extension) {
        IconSymbol::FileArchive
    } else if is_code_extension(extension) {
        IconSymbol::FileCode
    } else if is_text_extension(extension) {
        IconSymbol::FileText
    } else {
        IconSymbol::File
    }
}

fn is_image_extension(extension: &str) -> bool {
    [
        "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "tif", "tiff", "avif",
    ]
    .iter()
    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

fn is_archive_extension(extension: &str) -> bool {
    [
        "zip", "tar", "gz", "tgz", "xz", "bz2", "7z", "rar", "zst", "deb", "rpm",
    ]
    .iter()
    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

fn is_code_extension(extension: &str) -> bool {
    [
        "rs", "py", "js", "ts", "tsx", "jsx", "c", "h", "cpp", "hpp", "go", "java", "kt", "swift",
        "sh", "bash", "zsh", "fish", "html", "css", "scss", "xml",
    ]
    .iter()
    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

fn is_text_extension(extension: &str) -> bool {
    [
        "txt", "md", "markdown", "rst", "log", "toml", "yaml", "yml", "json", "csv",
    ]
    .iter()
    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

impl IconSymbol {
    pub(crate) fn view(self, size: f32) -> Svg<Theme> {
        Svg::new(svg::Handle::from_memory(self.bytes()))
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::ArrowLeft => include_bytes!("../assets/icons/lucide/arrow-left.svg"),
            Self::ArrowRight => include_bytes!("../assets/icons/lucide/arrow-right.svg"),
            Self::ArrowUp => include_bytes!("../assets/icons/lucide/arrow-up.svg"),
            Self::ChevronRight => include_bytes!("../assets/icons/lucide/chevron-right.svg"),
            Self::Close => CLOSE_ICON,
            Self::Copy => include_bytes!("../assets/icons/lucide/copy.svg"),
            Self::File => include_bytes!("../assets/icons/lucide/file.svg"),
            Self::FileArchive => include_bytes!("../assets/icons/lucide/file-archive.svg"),
            Self::FileCode => include_bytes!("../assets/icons/lucide/file-code.svg"),
            Self::FileImage => include_bytes!("../assets/icons/lucide/file-image.svg"),
            Self::FileText => include_bytes!("../assets/icons/lucide/file-text.svg"),
            Self::Folder => include_bytes!("../assets/icons/lucide/folder.svg"),
            Self::Link => include_bytes!("../assets/icons/lucide/link.svg"),
            Self::Pencil => include_bytes!("../assets/icons/lucide/pencil.svg"),
            Self::Settings => SETTINGS_ICON,
            Self::Trash => include_bytes!("../assets/icons/lucide/trash-2.svg"),
            Self::TriangleAlert => include_bytes!("../assets/icons/lucide/triangle-alert.svg"),
        }
    }
}

const CLOSE_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>"#;
const SETTINGS_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.32-1.915"/><circle cx="12" cy="12" r="3"/></svg>"#;
