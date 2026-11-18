//! Images the page sends a session with a follow-up: pasted into its box
//! or picked from a phone's photos. Each is kept as a file in the DATA
//! DIR's `attachments/` ([`nebula_core::attachments`]), outside every
//! checkout, under a name no one can guess, readable by the user alone,
//! and the follow-up names it by its path on a line of its own, which
//! Claude Code takes for the image itself ([`nebula_core::turn::is_image_path`]).
//!
//! Only what the agent CLIs read is kept: PNG, JPEG, GIF and WebP, told by
//! their first bytes, never by the type the browser names. A HEIC or HEIF
//! photo, which an iPhone hands over when asked for the photo as it is, is
//! kept as the JPEG macOS's own `sips` makes of it.

use crate::Refusal;
use axum::http::StatusCode;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The largest image taken. A phone's photo is 2 to 8 MB as a JPEG, up to
/// about 15 MB at 48 megapixels, and a screenshot a few MB as a PNG; Claude
/// Code scales whatever it reads down to what the model takes anyway. This
/// holds every one of them and bounds what one request holds in memory.
pub const MAX_BYTES: usize = 20 * 1024 * 1024;

/// How long `sips` may take to turn a HEIC into a JPEG.
const CONVERT_TIMEOUT: Duration = Duration::from_secs(30);

/// What the first bytes of an image say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Png,
    Jpeg,
    Gif,
    Webp,
    /// HEIC or HEIF, which no agent CLI reads.
    Heif,
}

impl Kind {
    /// The extension its file is kept with, which Claude Code reads a
    /// pasted path's type by.
    fn ext(self) -> &'static str {
        match self {
            Kind::Png => "png",
            Kind::Jpeg => "jpg",
            Kind::Gif => "gif",
            Kind::Webp => "webp",
            Kind::Heif => "heic",
        }
    }
}

fn sniff(bytes: &[u8]) -> Option<Kind> {
    const HEIF_BRANDS: [&[u8]; 8] = [
        b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"mif1", b"msf1",
    ];
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Kind::Png)
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(Kind::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(Kind::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(Kind::Webp)
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && HEIF_BRANDS.contains(&&bytes[8..12]) {
        Some(Kind::Heif)
    } else {
        None
    }
}

fn unsupported(why: &str) -> Refusal {
    Refusal::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, why)
}

/// Keep `bytes` in `dir` as an image the agent CLIs read, and answer the
/// id a follow-up names it by. Files there older than a week go first.
pub async fn keep(dir: &Path, bytes: &[u8]) -> Result<String, Refusal> {
    if bytes.len() > MAX_BYTES {
        return Err(too_large());
    }
    let kind = sniff(bytes).ok_or_else(|| {
        unsupported("That is not an image nebula can send: a PNG, JPEG, GIF, WebP or HEIC.")
    })?;
    nebula_core::attachments::prune(dir);
    let (kind, bytes) = match kind {
        Kind::Heif => (Kind::Jpeg, to_jpeg(dir, bytes).await?),
        kind => (kind, bytes.to_vec()),
    };
    let id = format!(
        "web-{}.{}",
        crate::devices::hex(&crate::devices::random(16)),
        kind.ext()
    );
    nebula_forge::push::write_private(&dir.join(&id), &bytes).map_err(|err| {
        Refusal::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("The image was not kept: {err}."),
        )
    })?;
    Ok(id)
}

pub fn too_large() -> Refusal {
    Refusal::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        format!(
            "The image is too large: it can be {} MB at most.",
            MAX_BYTES / 1024 / 1024
        ),
    )
}

/// A HEIC as a JPEG, by macOS's `sips`, through two files of the user's
/// alone in `dir` that go once it has read them.
async fn to_jpeg(dir: &Path, heic: &[u8]) -> Result<Vec<u8>, Refusal> {
    if !cfg!(target_os = "macos") {
        return Err(unsupported(
            "A HEIC photo is made a JPEG only on a Mac: share it as a JPEG (on an iPhone, \
             Most Compatible).",
        ));
    }
    let stem = format!(".heic-{}", crate::devices::hex(&crate::devices::random(16)));
    let (input, output) = (
        dir.join(format!("{stem}.heic")),
        dir.join(format!("{stem}.jpg")),
    );
    let unread = || unsupported("The HEIC photo could not be read: share it as a JPEG.");
    nebula_forge::push::write_private(&input, heic).map_err(|_| unread())?;
    let run = tokio::process::Command::new("/usr/bin/sips")
        .args(["-s", "format", "jpeg"])
        .arg(&input)
        .arg("--out")
        .arg(&output)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .status();
    let ran = tokio::time::timeout(CONVERT_TIMEOUT, run).await;
    let jpeg = std::fs::read(&output);
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
    match (ran, jpeg) {
        (Ok(Ok(status)), Ok(jpeg)) if status.success() && sniff(&jpeg) == Some(Kind::Jpeg) => {
            Ok(jpeg)
        }
        _ => Err(unread()),
    }
}

/// The path of the image `id` in `dir`, which [`keep`] answered: refused
/// for an id it never gives, so no other file can be named, and for one
/// whose file is gone.
pub fn path_of(dir: &Path, id: &str) -> Result<PathBuf, Refusal> {
    let ours = id
        .strip_prefix("web-")
        .and_then(|rest| rest.split_once('.'))
        .is_some_and(|(hex, ext)| {
            hex.len() == 32
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                && ["png", "jpg", "gif", "webp"].contains(&ext)
        });
    if !ours {
        return Err(Refusal::new(
            StatusCode::BAD_REQUEST,
            format!("There is no image {id}."),
        ));
    }
    let path = dir.join(id);
    if !path.is_file() {
        return Err(Refusal::new(
            StatusCode::NOT_FOUND,
            "An image is gone from the Mac: add it again.",
        ));
    }
    Ok(path)
}

/// A follow-up's `text` with each image's path on a line of its own under
/// it, as the session is sent it.
pub fn with_paths(text: &str, paths: &[PathBuf]) -> String {
    let text = text.trim();
    let mut lines: Vec<String> = (!text.is_empty())
        .then(|| text.to_string())
        .into_iter()
        .collect();
    lines.extend(paths.iter().map(|p| p.to_string_lossy().into_owned()));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR pixels";

    /// A whole 2x2 PNG, which `sips` reads.
    #[cfg(target_os = "macos")]
    const TINY_PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x02\x00\x00\x00\x02\x08\x02\x00\x00\x00\xfd\xd4\x9a\x73\x00\x00\x00\x10\x49\x44\x41\x54\x78\x9c\x63\xf8\xcf\xc0\x00\x44\x0c\x10\x0a\x00\x1f\xee\x03\xfd\x8b\x5f\x14\xd4\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

    #[test]
    fn an_image_is_told_by_its_first_bytes() {
        assert_eq!(sniff(PNG), Some(Kind::Png));
        assert_eq!(sniff(b"\xff\xd8\xff\xe0\0\x10JFIF"), Some(Kind::Jpeg));
        assert_eq!(sniff(b"GIF89a\x01\0"), Some(Kind::Gif));
        assert_eq!(sniff(b"RIFF\x24\0\0\0WEBPVP8 "), Some(Kind::Webp));
        assert_eq!(sniff(b"\0\0\0\x18ftypheic\0\0\0\0"), Some(Kind::Heif));
        assert_eq!(sniff(b"\0\0\0\x1cftypmif1\0\0\0\0"), Some(Kind::Heif));
        for other in [
            &b"\0\0\0\x1cftypavif\0\0\0\0"[..],
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
            b"BM\x36\0\0\0",
            b"%PDF-1.7",
            b"\x89PNG",
            b"RIFF\x24\0\0\0WAVE",
            b"",
        ] {
            assert_eq!(sniff(other), None, "{other:?}");
        }
    }

    #[tokio::test]
    async fn an_image_is_kept_for_the_user_alone_under_a_name_no_one_guesses() {
        let dir = tempfile::tempdir().unwrap();
        let a = keep(dir.path(), PNG).await.unwrap();
        let b = keep(dir.path(), PNG).await.unwrap();
        assert_ne!(a, b, "the same image twice is two files");
        assert_eq!(a.len(), "web-".len() + 32 + ".png".len(), "{a}");
        let path = path_of(dir.path(), &a).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), PNG);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let listed = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(listed, 2, "no file is left behind but the two");
    }

    #[tokio::test]
    async fn only_an_image_under_the_limit_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let refused = keep(dir.path(), b"<svg/>").await.unwrap_err();
        assert_eq!(refused.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let mut big = PNG.to_vec();
        big.resize(MAX_BYTES + 1, 0);
        assert_eq!(
            keep(dir.path(), &big).await.unwrap_err().status,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        big.truncate(MAX_BYTES);
        assert!(
            keep(dir.path(), &big).await.is_ok(),
            "the limit itself is taken"
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_heic_photo_is_kept_as_a_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("shot.png");
        // Made a HEIC by the same sips the server runs.
        std::fs::write(&png, TINY_PNG).unwrap();
        let heic = dir.path().join("shot.heic");
        let made = std::process::Command::new("/usr/bin/sips")
            .args(["-s", "format", "heic"])
            .arg(&png)
            .arg("--out")
            .arg(&heic)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        let kept = dir.path().join("kept");
        let id = keep(&kept, &std::fs::read(&heic).unwrap()).await.unwrap();
        assert!(id.ends_with(".jpg"), "{id}");
        let jpeg = std::fs::read(path_of(&kept, &id).unwrap()).unwrap();
        assert_eq!(sniff(&jpeg), Some(Kind::Jpeg));
        let left: Vec<_> = std::fs::read_dir(&kept).unwrap().flatten().collect();
        assert_eq!(left.len(), 1, "sips's files go: {left:?}");
        let bad = keep(&kept, b"\0\0\0\x18ftypheic\0\0\0\0 not a photo").await;
        assert_eq!(bad.unwrap_err().status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[test]
    fn only_an_id_keep_gave_names_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let id = format!("web-{}.png", "ab".repeat(16));
        std::fs::write(dir.path().join(&id), PNG).unwrap();
        std::fs::write(dir.path().join("notes.png"), PNG).unwrap();
        assert!(path_of(dir.path(), &id).is_ok());
        for other in [
            "notes.png",
            "../notes.png",
            &format!("web-{}.png", "AB".repeat(16)),
            &format!("web-{}.svg", "ab".repeat(16)),
            &format!("web-{}/../../x.png", "ab".repeat(16)),
            &format!("web-{}.png", "ab".repeat(15)),
        ] {
            assert_eq!(
                path_of(dir.path(), other).unwrap_err().status,
                StatusCode::BAD_REQUEST,
                "{other}"
            );
        }
        let gone = format!("web-{}.jpg", "cd".repeat(16));
        assert_eq!(
            path_of(dir.path(), &gone).unwrap_err().status,
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn each_path_goes_on_a_line_of_its_own_under_the_text() {
        let paths = [PathBuf::from("/d/web-a.png"), PathBuf::from("/d/web-b.jpg")];
        assert_eq!(
            with_paths("  What does this show?\n", &paths),
            "What does this show?\n/d/web-a.png\n/d/web-b.jpg"
        );
        assert_eq!(with_paths(" ", &paths[..1]), "/d/web-a.png");
        assert_eq!(with_paths("carry on", &[]), "carry on");
        for line in with_paths("look", &paths).lines().skip(1) {
            assert!(nebula_core::turn::is_image_path(line), "{line}");
        }
    }

    #[tokio::test]
    async fn a_week_old_image_goes_when_another_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join(format!("web-{}.png", "ef".repeat(16)));
        std::fs::write(&old, PNG).unwrap();
        let week = nebula_core::attachments::KEEP + Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - week)
            .unwrap();
        let new = keep(dir.path(), PNG).await.unwrap();
        assert!(!old.exists(), "a week-old image goes");
        assert!(dir.path().join(new).exists());
    }
}
