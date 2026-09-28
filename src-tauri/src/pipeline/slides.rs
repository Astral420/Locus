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

pub fn persist(
    db: &Database,
    client: &mut SidecarClient,
    meeting_id: &str,
    slides: &[DetectedSlide],
) -> Result<(), String> {
    let mut ocr_texts = Vec::with_capacity(slides.len());
    for slide in slides {
        let result: serde_json::Value = client
            .call(
                "ocr_slide",
                serde_json::json!({ "image_path": slide.image_path }),
            )
            .map_err(|e| e.to_string())?;
        ocr_texts.push(
            result
                .get("text")
                .and_then(|text| text.as_str())
                .unwrap_or_default()
                .to_owned(),
        );
    }
    let meeting_id = meeting_id.to_owned();
    let slides = slides
        .iter()
        .map(|slide| (slide.ordinal, slide.timestamp, slide.image_path.clone()))
        .collect::<Vec<_>>();
    db.run(move |connection| {
        let tx = connection.transaction().map_err(|e| e.to_string())?;
        for ((ordinal, timestamp, image_path), ocr_text) in slides.iter().zip(ocr_texts.iter()) {
            let bytes = fs::read(image_path).map_err(|e| e.to_string())?;
            let mut hash = Sha256::new(); hash.update(bytes); let image_hash = format!("{:x}", hash.finalize());
            tx.execute("INSERT OR IGNORE INTO slides(id, meeting_id, image_hash, relative_path, ocr_text) VALUES (?1, ?2, ?3, ?4, ?5)", (Uuid::new_v4().to_string(), &meeting_id, &image_hash, Path::new(image_path).to_string_lossy().to_string(), ocr_text)).map_err(|e| e.to_string())?;
            let slide_id: String = tx.query_row("SELECT id FROM slides WHERE meeting_id = ?1 AND image_hash = ?2", (&meeting_id, &image_hash), |row| row.get(0)).map_err(|e| e.to_string())?;
            tx.execute("INSERT OR REPLACE INTO slide_occurrences(id, slide_id, meeting_id, ordinal, timestamp_seconds) VALUES ((SELECT COALESCE((SELECT id FROM slide_occurrences WHERE meeting_id = ?1 AND ordinal = ?2), ?3)), ?4, ?1, ?2, ?5)", (&meeting_id, ordinal, Uuid::new_v4().to_string(), &slide_id, timestamp)).map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?; Ok(())
    }).map_err(|e| e.to_string())
}
