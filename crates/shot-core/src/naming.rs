//! File names of saved captures and writing them (SPEC §3.4).

use std::fs::File;
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};

use crate::image::{BgrxView, Compression, encode_png};

/// Wall-clock time in the user's time zone, as the platform reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// `Shot 2026-10-22 17-03-15.png`; with `monitor = Some(2)` — `Shot 2026-10-22 17-03-15 (2).png`.
/// No colons: they are not allowed in Windows file names.
pub fn capture_file_name(t: LocalTime, monitor: Option<usize>) -> String {
    let stamp = format!(
        "Shot {:04}-{:02}-{:02} {:02}-{:02}-{:02}",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    );
    match monitor {
        Some(n) => format!("{stamp} ({n}).png"),
        None => format!("{stamp}.png"),
    }
}

/// Writes each image as a PNG into `dir`, creating it if needed, with its density in DPI (see
/// [`encode_png`]), and returns each file's path or error. One image gets the plain name; several
/// get `(1)`, `(2)`, … in order. A name already taken — two captures in one second — moves on to
/// the next free number. Files are created with `create_new`, so captures saved at once from
/// different threads never overwrite each other.
pub fn save_captures(
    dir: &Path,
    time: LocalTime,
    images: &[(BgrxView, f32)],
) -> Vec<io::Result<PathBuf>> {
    if let Err(e) = std::fs::create_dir_all(dir) {
        return images.iter().map(|_| Err(clone_error(&e))).collect();
    }
    // 0 stands for "no number": the plain name, then (2), (3), …
    let mut next = if images.len() == 1 { 0 } else { 1 };
    images
        .iter()
        .map(|&(image, dpi)| {
            for _ in 0..1000 {
                let path = dir.join(capture_file_name(time, (next > 0).then_some(next)));
                next = if next == 0 { 2 } else { next + 1 };
                match File::create_new(&path) {
                    Ok(file) => {
                        let out = BufWriter::new(file);
                        return match encode_png(image, out, Compression::Fast, Some(dpi)) {
                            Ok(()) => Ok(path),
                            Err(e) => {
                                let _ = std::fs::remove_file(&path);
                                Err(e)
                            }
                        };
                    }
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e),
                }
            }
            Err(io::Error::other("no free file name"))
        })
        .collect()
}

/// Writes `image` as a PNG over the file at `path`, the editor's save. The PNG goes into a
/// temporary file next to it first, which then replaces the file: a failed write leaves the file
/// as it was.
pub fn replace_png(path: &Path, image: BgrxView, dpi: f32) -> io::Result<()> {
    replace_file(path, |out| {
        encode_png(image, out, Compression::Fast, Some(dpi))
    })
}

fn replace_file(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(".shot-tmp");
    let temp = path.with_file_name(temp_name);
    let result = (|| {
        let mut out = BufWriter::new(File::create(&temp)?);
        write(&mut out)?;
        let file = out.into_inner().map_err(io::IntoInnerError::into_error)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn clone_error(e: &io::Error) -> io::Error {
    io::Error::new(e.kind(), e.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const T: LocalTime = LocalTime {
        year: 2026,
        month: 10,
        day: 2,
        hour: 7,
        minute: 3,
        second: 5,
    };

    #[test]
    fn single_capture() {
        assert_eq!(capture_file_name(T, None), "Shot 2026-10-02 07-03-05.png");
    }

    #[test]
    fn one_file_per_monitor() {
        assert_eq!(
            capture_file_name(T, Some(2)),
            "Shot 2026-10-02 07-03-05 (2).png"
        );
    }

    #[test]
    fn a_failed_overwrite_keeps_the_file() {
        let dir = std::env::temp_dir().join(format!("shot-replace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Shot.png");
        std::fs::write(&path, b"before").unwrap();

        let failed = replace_file(&path, |out| {
            out.write_all(b"half")?;
            Err(io::Error::from(io::ErrorKind::StorageFull))
        });
        assert_eq!(failed.unwrap_err().kind(), io::ErrorKind::StorageFull);
        assert_eq!(std::fs::read(&path).unwrap(), b"before");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "no temporary file left"
        );

        let data = [10u8, 20, 30, 0, 40, 50, 60, 0];
        let px = BgrxView {
            width: 2,
            height: 1,
            stride: 8,
            data: &data,
        };
        replace_png(&path, px, 120.0).unwrap();
        let decoded =
            crate::image::decode_png(std::io::BufReader::new(File::open(&path).unwrap())).unwrap();
        assert_eq!(decoded.data, [10, 20, 30, 255, 40, 50, 60, 255]);
        assert!((decoded.dpi.unwrap() - 120.0).abs() < 0.1);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn taken_names_move_to_the_next_number() {
        let dir = std::env::temp_dir().join(format!("shot-naming-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = [0u8; 4];
        let px = BgrxView {
            width: 1,
            height: 1,
            stride: 4,
            data: &data,
        };
        let names = |results: Vec<io::Result<PathBuf>>| {
            results
                .into_iter()
                .map(|r| {
                    r.unwrap()
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect::<Vec<_>>()
        };
        let stamp = "Shot 2026-10-02 07-03-05";
        assert_eq!(
            names(save_captures(&dir, T, &[(px, 96.0)])),
            [format!("{stamp}.png")]
        );
        assert_eq!(
            names(save_captures(&dir, T, &[(px, 96.0)])),
            [format!("{stamp} (2).png")]
        );
        assert_eq!(
            names(save_captures(&dir, T, &[(px, 96.0), (px, 96.0)])),
            [format!("{stamp} (1).png"), format!("{stamp} (3).png")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
