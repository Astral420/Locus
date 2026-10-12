use crate::{
    db::Database,
    sidecar::client::{SidecarClient, SidecarError},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct DetectedSlide {
    pub ordinal: i64,
    pub timestamp: f32,
    pub image_path: String,
    #[serde(default)]
    pub transition: bool,
}
#[derive(Debug, Deserialize)]
struct SlideResponse {
    slides: Vec<DetectedSlide>,
}

pub fn detect(
    client: &mut SidecarClient,
    video_path: &str,
    output_dir: Option<&str>,
) -> Result<Vec<DetectedSlide>, SidecarError> {
    let response: SlideResponse = client.call(
        "detect_slides",
        serde_json::json!({ "video_path": video_path, "output_dir": output_dir }),
    )?;
    Ok(response.slides)
}

/// Path stored in the database: relative to the media root so the library can
/// be relocated (FR12 / storage migration) without rewriting rows.
pub fn relative_to_media(media_root: &Path, image_path: &str) -> String {
    let path = Path::new(image_path);
    let canonical_root = media_root.canonicalize().ok();
    let relative = path.strip_prefix(media_root).ok().or_else(|| {
        canonical_root
            .as_deref()
            .and_then(|root| path.strip_prefix(root).ok())
    });
    match relative {
        Some(relative) => relative.to_string_lossy().replace('\\', "/"),
        None => image_path.to_owned(),
    }
}

/// Stores detected slides (without OCR). Identical images collapse to one slide
/// with several occurrences (SPEC: repeated slides keep every timestamp). A
/// re-run replaces the previous detection rather than stacking duplicates.
pub fn persist_detected(
    db: &Database,
    media_root: &Path,
    meeting_id: &str,
    slides: &[DetectedSlide],
) -> Result<usize, String> {
    let meeting_id = meeting_id.to_owned();
    let rows = slides
        .iter()
        .map(|slide| {
            let bytes = fs::read(&slide.image_path)
                .map_err(|e| format!("cannot read slide image {}: {e}", slide.image_path))?;
            let mut hash = Sha256::new();
            hash.update(bytes);
            Ok((
                slide.ordinal,
                slide.timestamp,
                relative_to_media(media_root, &slide.image_path),
                format!("{:x}", hash.finalize()),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    db.run(move |connection| {
        let tx = connection.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM slides WHERE meeting_id = ?1", [&meeting_id])
            .map_err(|e| e.to_string())?;
        let mut unique = 0_usize;
        for (ordinal, timestamp, relative_path, image_hash) in &rows {
            let inserted = tx
                .execute(
                    "INSERT OR IGNORE INTO slides(id, meeting_id, image_hash, relative_path, ocr_text) VALUES (?1, ?2, ?3, ?4, NULL)",
                    (Uuid::new_v4().to_string(), &meeting_id, image_hash, relative_path),
                )
                .map_err(|e| e.to_string())?;
            unique += inserted;
            let slide_id: String = tx
                .query_row(
                    "SELECT id FROM slides WHERE meeting_id = ?1 AND image_hash = ?2",
                    (&meeting_id, image_hash),
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO slide_occurrences(id, slide_id, meeting_id, ordinal, timestamp_seconds) VALUES (?1, ?2, ?3, ?4, ?5)",
                (Uuid::new_v4().to_string(), &slide_id, &meeting_id, ordinal, timestamp),
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(unique)
    })
    .map_err(|e| e.to_string())
}

/// Slides that still need OCR as `(slide_id, absolute image path)`.
pub fn pending_ocr(
    db: &Database,
    media_root: &Path,
    meeting_id: &str,
) -> Result<Vec<(String, std::path::PathBuf)>, String> {
    let meeting_id = meeting_id.to_owned();
    let media_root = media_root.to_path_buf();
    db.run(move |connection| {
        let mut statement = connection
            .prepare("SELECT id, relative_path FROM slides WHERE meeting_id = ?1 AND ocr_text IS NULL ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([&meeting_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|(id, relative)| {
                let path = Path::new(&relative);
                let absolute = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    media_root.join(path)
                };
                (id, absolute)
            })
            .collect())
    })
    .map_err(|e| e.to_string())
}

pub fn ocr_image(client: &mut SidecarClient, image_path: &str) -> Result<String, SidecarError> {
    let result: serde_json::Value = client.call(
        "ocr_slide",
        serde_json::json!({ "image_path": image_path }),
    )?;
    Ok(result
        .get("text")
        .and_then(|text| text.as_str())
        .unwrap_or_default()
        .to_owned())
}

pub fn store_ocr(db: &Database, slide_id: &str, text: &str) -> Result<(), String> {
    let slide_id = slide_id.to_owned();
    let text = text.to_owned();
    db.run(move |connection| {
        connection
            .execute("UPDATE slides SET ocr_text = ?1 WHERE id = ?2", (&text, &slide_id))
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting(db: &Database) {
        db.run(|c| {
            c.execute("INSERT INTO meetings(id,title,title_origin,recorded_at,timezone) VALUES ('m1','T','placeholder','now','UTC')", [])
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .unwrap();
    }

    #[test]
    fn identical_slides_collapse_and_reruns_replace() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().to_path_buf();
        std::fs::create_dir_all(media.join("m1/slides")).unwrap();
        let (a, b, c) = (
            media.join("m1/slides/a.png"),
            media.join("m1/slides/b.png"),
            media.join("m1/slides/c.png"),
        );
        std::fs::write(&a, b"one").unwrap();
        std::fs::write(&b, b"two").unwrap();
        std::fs::write(&c, b"one").unwrap(); // same image as `a`, shown again later
        let db = Database::in_memory().unwrap();
        meeting(&db);
        let detected = |path: &std::path::PathBuf, ordinal, timestamp| DetectedSlide {
            ordinal,
            timestamp,
            image_path: path.to_string_lossy().to_string(),
            transition: false,
        };
        let slides = vec![detected(&a, 0, 1.0), detected(&b, 1, 5.0), detected(&c, 2, 9.0)];
        assert_eq!(persist_detected(&db, &media, "m1", &slides).unwrap(), 2);
        assert_eq!(persist_detected(&db, &media, "m1", &slides).unwrap(), 2);
        let (slides_n, occurrences): (i64, i64) = db
            .run(|c| {
                Ok((
                    c.query_row("SELECT COUNT(*) FROM slides", [], |r| r.get(0)).unwrap(),
                    c.query_row("SELECT COUNT(*) FROM slide_occurrences", [], |r| r.get(0)).unwrap(),
                ))
            })
            .unwrap();
        assert_eq!((slides_n, occurrences), (2, 3));
        let pending = pending_ocr(&db, &media, "m1").unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|(_, path)| path.starts_with(&media) && path.is_file()));
        store_ocr(&db, &pending[0].0, "Agenda").unwrap();
        assert_eq!(pending_ocr(&db, &media, "m1").unwrap().len(), 1);
    }

    #[test]
    fn relative_paths_are_stored_forward_slashed() {
        let root = Path::new("/data/media");
        assert_eq!(
            relative_to_media(root, "/data/media/m1/slides/a.png"),
            "m1/slides/a.png"
        );
        assert_eq!(relative_to_media(root, "/elsewhere/a.png"), "/elsewhere/a.png");
    }
}
