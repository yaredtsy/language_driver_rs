//! Paths, by text. Every path the book keeps is absolute and real (symlinks followed, as `bend` does with `realpathSync`), spelled without a Windows
//! verbatim prefix, so two of them compare as places with `Path::starts_with`.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// The real path of `path`, or `None` when there is nothing there.
pub(crate) fn real(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok().map(plain)
}

/// On Windows `canonicalize` answers `\\?\D:\repo` (and `\\?\UNC\server\share`): the ordinary spelling is given back. Any other path is unchanged.
fn plain(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    match text.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(share) => PathBuf::from(format!(r"\\{share}")),
            None => PathBuf::from(rest),
        },
        None => path,
    }
}

/// The path from the folder `from` to `to`, with `/` for a separator and `..` for each folder that is climbed out of: `../lib/x.bend`. Both are real
/// paths. With no place in common (two drives on Windows) it is `to` itself.
pub(crate) fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<Component> = from.components().collect();
    let to_parts: Vec<Component> = to.components().collect();
    let common = from
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    if common == 0 {
        return to.to_string_lossy().replace('\\', "/");
    }
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - common];
    parts.extend(
        to_parts[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_from_a_folder_climbs_with_dots_and_descends_by_name() {
        let dir = Path::new("/work/app");
        assert_eq!(relative(dir, Path::new("/work/app/main.bend")), "main.bend");
        assert_eq!(
            relative(dir, Path::new("/work/app/sub/x.bend")),
            "sub/x.bend"
        );
        assert_eq!(
            relative(dir, Path::new("/work/shared/lib.bend")),
            "../shared/lib.bend"
        );
        assert_eq!(relative(dir, Path::new("/lib.bend")), "../../lib.bend");
    }

    #[test]
    fn a_verbatim_prefix_is_taken_off() {
        assert_eq!(
            plain(PathBuf::from(r"\\?\D:\repo")),
            PathBuf::from(r"D:\repo")
        );
        assert_eq!(
            plain(PathBuf::from(r"\\?\UNC\server\share")),
            PathBuf::from(r"\\server\share")
        );
        assert_eq!(plain(PathBuf::from("/usr/lib")), PathBuf::from("/usr/lib"));
    }
}
