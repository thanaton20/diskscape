//! Coarse file categories, used to colour the treemap.

use std::ffi::OsStr;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Category {
    Video,
    Audio,
    Image,
    Archive,
    Document,
    Code,
    App,
    Other,
}

impl Category {
    /// Stable lowercase name, used by the interface.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Video => "video",
            Category::Audio => "audio",
            Category::Image => "image",
            Category::Archive => "archive",
            Category::Document => "document",
            Category::Code => "code",
            Category::App => "app",
            Category::Other => "other",
        }
    }
}

const VIDEO: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "m2ts", "3gp", "vob",
];
const AUDIO: &[&str] = &[
    "mp3", "flac", "wav", "aac", "ogg", "opus", "m4a", "wma", "aiff", "alac", "mid",
];
const IMAGE: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "tiff", "tif", "webp", "heic", "heif", "svg", "raw", "cr2",
    "nef", "arw", "dng", "psd", "ico", "avif",
];
const ARCHIVE: &[&str] = &[
    "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "tgz", "iso", "dmg", "pkg", "cab", "deb",
    "rpm", "jar",
];
const DOCUMENT: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "txt", "rtf", "md",
    "epub", "mobi", "djvu", "csv", "pages", "numbers", "key",
];
const CODE: &[&str] = &[
    "rs", "c", "h", "cpp", "hpp", "cc", "py", "js", "ts", "jsx", "tsx", "java", "kt", "swift", "go",
    "rb", "php", "cs", "html", "css", "scss", "json", "xml", "yaml", "yml", "toml", "sh", "sql",
    "lua",
];
const APP: &[&str] = &[
    "exe", "dll", "so", "dylib", "app", "msi", "bin", "apk", "ipa", "appimage", "sys", "o", "a",
    "lib", "class", "wasm",
];

/// Category by file extension, case-insensitively. Names without an extension
/// (including dotfiles such as `.bashrc`) are `Other`.
pub fn categorize(name: &OsStr) -> Category {
    let name = name.to_string_lossy();
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return Category::Other;
    };
    if stem.is_empty() || ext.is_empty() {
        return Category::Other;
    }
    let ext = ext.to_ascii_lowercase();
    let ext = ext.as_str();
    [
        (VIDEO, Category::Video),
        (AUDIO, Category::Audio),
        (IMAGE, Category::Image),
        (ARCHIVE, Category::Archive),
        (DOCUMENT, Category::Document),
        (CODE, Category::Code),
        (APP, Category::App),
    ]
    .into_iter()
    .find(|(list, _)| list.contains(&ext))
    .map_or(Category::Other, |(_, category)| category)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn cat(name: &str) -> Category {
        categorize(OsStr::new(name))
    }

    #[test]
    fn categorize_known_extensions() {
        assert_eq!(cat("a.MKV"), Category::Video);
        assert_eq!(cat("a.flac"), Category::Audio);
        assert_eq!(cat("a.png"), Category::Image);
        assert_eq!(cat("a.7z"), Category::Archive);
        assert_eq!(cat("a.pdf"), Category::Document);
        assert_eq!(cat("a.rs"), Category::Code);
        assert_eq!(cat("a.exe"), Category::App);
        assert_eq!(cat("noext"), Category::Other);
    }

    #[test]
    fn categorize_edge_cases() {
        assert_eq!(cat("archive.tar.GZ"), Category::Archive);
        assert_eq!(cat(".bashrc"), Category::Other);
        assert_eq!(cat("trailing."), Category::Other);
        assert_eq!(cat(""), Category::Other);
        assert_eq!(cat("Safari.app"), Category::App);
        assert_eq!(cat("weird.ФЫВ"), Category::Other);
    }

    #[test]
    fn every_category_has_a_distinct_name() {
        let all = [
            Category::Video, Category::Audio, Category::Image, Category::Archive,
            Category::Document, Category::Code, Category::App, Category::Other,
        ];
        let names: std::collections::HashSet<_> = all.iter().map(|c| c.as_str()).collect();
        assert_eq!(names.len(), all.len());
        assert_eq!(Category::Video.as_str(), "video");
    }
}
