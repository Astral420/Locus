//! Turns raw catalog entries into the models the app shows and downloads.
//!
//! Only files that pass [`Quant::verify`] survive; an entry with no verifiable file is
//! dropped and counted, never shown as downloadable.

use super::types::{CatalogEntry, EmbeddingMeta, Quant, VerifiedFile};
use std::collections::BTreeMap;

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// Models the catalog lists first. Everything else is ordered by download count.
pub const RECOMMENDED_LLM_IDS: &[&str] = &[
    "unsloth/qwen3-4b-instruct-2507",
    "qwen/qwen3-8b",
    "bartowski/llama-3.2-3b-instruct",
    "unsloth/gemma-3-4b-it",
    "unsloth/gemma-3-12b-it",
    "ggml-org/gemma-3-1b-it",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModelRole {
    Whisper,
    Llm,
    Embedding,
}

impl ModelRole {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelRole::Whisper => "whisper",
            ModelRole::Llm => "llm",
            ModelRole::Embedding => "embedding",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogVariant {
    /// Lower-case quantization label, e.g. "q4_k_m"; "standard" for unquantized whisper.
    pub id: String,
    pub quantization: Option<String>,
    pub file: VerifiedFile,
    /// An estimate (weights plus runtime headroom), not a measurement.
    pub ram_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogModel {
    pub catalog_id: String,
    pub display_name: String,
    pub role: ModelRole,
    pub author: String,
    pub description: Option<String>,
    pub downloads: u64,
    pub tags: Vec<String>,
    pub revision: Option<String>,
    /// Smallest file first.
    pub variants: Vec<CatalogVariant>,
    pub embedding: Option<EmbeddingMeta>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Skipped {
    pub entries_without_verified_files: usize,
    pub files_rejected: usize,
}

// ---------- shared helpers ----------

fn is_quant_token(token: &str) -> bool {
    let token = token.to_ascii_lowercase();
    if matches!(token.as_str(), "f32" | "f16" | "bf16") {
        return true;
    }
    let rest = token
        .strip_prefix("iq")
        .or_else(|| token.strip_prefix('q'))
        .unwrap_or("");
    let mut chars = rest.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_digit())
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// "gemma-3-4b-it-Q4_K_M" -> "q4_k_m", "bge-small-en-v1.5-f16" -> "f16".
pub fn quant_label(model_id: &str) -> Option<String> {
    let stem = model_id.rsplit('/').next().unwrap_or(model_id);
    let idx = stem.rfind(['-', '.'])?;
    let token = &stem[idx + 1..];
    is_quant_token(token).then(|| token.to_ascii_lowercase())
}

/// "All-MiniLM-L6-v2-Embedding-GGUF" and "all-MiniLM-L6-v2-GGUF" both become
/// "all-minilm-l6-v2", so a model keeps one id whichever repo currently serves it.
pub fn base_id(model_name: &str) -> String {
    let mut id = model_name.to_ascii_lowercase();
    loop {
        let trimmed = ["-gguf", "-embedding"]
            .iter()
            .find_map(|suffix| id.strip_suffix(suffix));
        match trimmed {
            Some(shorter) if !shorter.is_empty() => id = shorter.to_string(),
            _ => return id,
        }
    }
}

fn verified_files<'a>(
    entry: &'a CatalogEntry,
    skipped: &mut Skipped,
) -> Vec<(&'a Quant, VerifiedFile)> {
    let mut files = Vec::new();
    for quant in &entry.quants {
        match quant.verify(entry.revision.as_deref()) {
            Ok(file) => files.push((quant, file)),
            Err(_) => skipped.files_rejected += 1,
        }
    }
    files
}

/// Files that live next to the weights but are not models: vision projectors and
/// importance matrices. Real catalogs list them as ordinary quants.
fn is_auxiliary(model_id: &str) -> bool {
    let stem = model_id.rsplit('/').next().unwrap_or(model_id).to_ascii_lowercase();
    stem.contains("mmproj") || stem.starts_with("imatrix")
}

/// Embedding models lose retrieval quality quickly below 5 bits, so the lower
/// quantizations the catalog lists are not offered.
fn embedding_quant_allowed(label: Option<&str>) -> bool {
    matches!(
        label,
        Some("q5_0" | "q5_1" | "q5_k_s" | "q5_k_m" | "q6_k" | "q8_0" | "f16" | "bf16" | "f32")
    )
}

fn llm_ram(size: u64) -> u64 {
    size / 10 * 12 + GIB * 3 / 2
}

// ---------- whisper ----------

const WHISPER_FAMILIES: &[(&str, &str, &str, u64)] = &[
    ("tiny", "Tiny", "Fastest and smallest; fine for quick drafts, weakest accuracy.", 200),
    ("base", "Base", "Fast with modest accuracy; good for clean single-speaker audio.", 250),
    ("small", "Small", "Balanced speed and accuracy; a sensible default for meetings.", 390),
    ("medium", "Medium", "Higher accuracy for accents and noisy rooms; needs more RAM and time.", 600),
    ("large-v1", "Large v1", "Original large model; superseded by v2 and v3.", 1000),
    ("large-v2", "Large v2", "High accuracy across many languages; heavy on RAM and compute.", 1000),
    ("large-v3", "Large v3", "Highest accuracy of the standard models; heavy on RAM and compute.", 1000),
    ("large-v3-turbo", "Large v3 Turbo", "Near large-v3 accuracy at much higher speed; multilingual only.", 800),
];
const ENGLISH_ONLY: &[&str] = &["tiny", "base", "small", "medium"];

/// "ggml-small.en-q5_1" -> ("small", english, Some("q5_1")).
pub fn parse_whisper_id(model_id: &str) -> Option<(&'static str, bool, Option<String>)> {
    let rest = model_id.strip_prefix("ggml-")?;
    let (base, quant) = match rest.rfind('-') {
        Some(i) if is_quant_token(&rest[i + 1..]) => {
            (&rest[..i], Some(rest[i + 1..].to_ascii_lowercase()))
        }
        _ => (rest, None),
    };
    let (family, english) = match base.strip_suffix(".en") {
        Some(family) => (family, true),
        None => (base, false),
    };
    let (name, ..) = WHISPER_FAMILIES.iter().find(|(name, ..)| *name == family)?;
    if english && !ENGLISH_ONLY.contains(name) {
        return None;
    }
    Some((name, english, quant))
}

pub fn build_whisper(entries: &[CatalogEntry]) -> (Vec<CatalogModel>, Skipped) {
    let mut skipped = Skipped::default();
    let mut grouped: BTreeMap<(usize, bool), CatalogModel> = BTreeMap::new();
    for entry in entries {
        let mut any = false;
        for (quant, file) in verified_files(entry, &mut skipped) {
            let Some((family, english, label)) = parse_whisper_id(&quant.model_id) else {
                skipped.files_rejected += 1;
                continue;
            };
            any = true;
            let index = WHISPER_FAMILIES.iter().position(|f| f.0 == family).unwrap();
            let (_, title, description, overhead) = WHISPER_FAMILIES[index];
            let model = grouped.entry((index, english)).or_insert_with(|| CatalogModel {
                catalog_id: format!("whisper-{family}{}", if english { "-en" } else { "" }),
                display_name: format!("Whisper {title}{}", if english { " (English)" } else { "" }),
                role: ModelRole::Whisper,
                author: entry.developer.clone(),
                description: Some(description.to_string()),
                downloads: entry.downloads.unwrap_or(0),
                tags: vec![if english { "English only" } else { "Multilingual" }.to_string()],
                revision: entry.revision.clone(),
                variants: Vec::new(),
                embedding: None,
            });
            let id = label.clone().unwrap_or_else(|| "standard".to_string());
            if model.variants.iter().all(|v| v.id != id) {
                model.variants.push(CatalogVariant {
                    id,
                    quantization: label,
                    ram_bytes: file.size_bytes + overhead * MIB,
                    file,
                });
            }
        }
        if !any {
            skipped.entries_without_verified_files += 1;
        }
    }
    let mut models: Vec<CatalogModel> = grouped.into_values().collect();
    for model in &mut models {
        model.variants.sort_by_key(|v| v.file.size_bytes);
    }
    (models, skipped)
}

// ---------- GGUF (LLM + embedding) ----------

fn gguf_variants(
    entry: &CatalogEntry,
    skipped: &mut Skipped,
    ram: impl Fn(u64) -> u64,
    keep: impl Fn(Option<&str>) -> bool,
) -> Vec<CatalogVariant> {
    let mut variants: Vec<CatalogVariant> = Vec::new();
    for (quant, file) in verified_files(entry, skipped) {
        if is_auxiliary(&quant.model_id) {
            skipped.files_rejected += 1;
            continue;
        }
        let label = quant_label(&quant.model_id);
        if !keep(label.as_deref()) {
            skipped.files_rejected += 1;
            continue;
        }
        let id = label.clone().unwrap_or_else(|| quant.model_id.to_ascii_lowercase());
        if variants.iter().any(|v| v.id == id) {
            continue;
        }
        variants.push(CatalogVariant {
            id,
            quantization: label,
            ram_bytes: ram(file.size_bytes),
            file,
        });
    }
    variants.sort_by_key(|v| v.file.size_bytes);
    variants
}

pub fn build_llm(entries: &[CatalogEntry]) -> (Vec<CatalogModel>, Skipped) {
    let mut skipped = Skipped::default();
    let mut by_id: BTreeMap<String, CatalogModel> = BTreeMap::new();
    for entry in entries {
        let variants = gguf_variants(entry, &mut skipped, llm_ram, |_| true);
        if variants.is_empty() {
            skipped.entries_without_verified_files += 1;
            continue;
        }
        let catalog_id = format!(
            "{}/{}",
            entry.developer.to_ascii_lowercase(),
            base_id(&entry.model_name)
        );
        let model = CatalogModel {
            catalog_id: catalog_id.clone(),
            display_name: entry.model_name.trim_end_matches("-GGUF").trim_end_matches("-gguf").to_string(),
            role: ModelRole::Llm,
            author: entry.developer.clone(),
            description: entry.description.clone().filter(|d| !d.trim().is_empty()),
            downloads: entry.downloads.unwrap_or(0),
            tags: if entry.tools.unwrap_or(false) { vec!["Tools".to_string()] } else { vec![] },
            revision: entry.revision.clone(),
            variants,
            embedding: None,
        };
        let keep_new = by_id.get(&catalog_id).map_or(true, |old| model.downloads > old.downloads);
        if keep_new {
            by_id.insert(catalog_id, model);
        }
    }
    let mut models: Vec<CatalogModel> = by_id.into_values().collect();
    models.sort_by(|a, b| rank(a).cmp(&rank(b)).then(b.downloads.cmp(&a.downloads)).then(a.catalog_id.cmp(&b.catalog_id)));
    (models, skipped)
}

fn rank(model: &CatalogModel) -> usize {
    RECOMMENDED_LLM_IDS
        .iter()
        .position(|id| *id == model.catalog_id)
        .unwrap_or(RECOMMENDED_LLM_IDS.len())
}

pub fn build_embedding(entries: &[CatalogEntry]) -> (Vec<CatalogModel>, Skipped) {
    let mut skipped = Skipped::default();
    let mut models: Vec<CatalogModel> = Vec::new();
    for entry in entries {
        // Without dimension and prefixes a client cannot build or query an index.
        let Some(meta) = entry.embedding.clone().filter(|m| m.dimension > 0) else {
            skipped.entries_without_verified_files += 1;
            continue;
        };
        let variants = gguf_variants(entry, &mut skipped, |size| size + 256 * MIB, embedding_quant_allowed);
        if variants.is_empty() {
            skipped.entries_without_verified_files += 1;
            continue;
        }
        let catalog_id = base_id(&entry.model_name);
        if models.iter().any(|m| m.catalog_id == catalog_id) {
            continue; // first repo wins; later fallbacks serve the same model
        }
        models.push(CatalogModel {
            catalog_id,
            display_name: entry.model_name.trim_end_matches("-GGUF").trim_end_matches("-gguf").to_string(),
            role: ModelRole::Embedding,
            author: entry.developer.clone(),
            description: entry.description.clone().filter(|d| !d.trim().is_empty()),
            downloads: entry.downloads.unwrap_or(0),
            tags: vec![format!("{} dims", meta.dimension)],
            revision: entry.revision.clone(),
            variants,
            embedding: Some(meta),
        });
    }
    (models, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REV: &str = "0123456789abcdef0123456789abcdef01234567";

    fn quant(repo: &str, file: &str, size: u64) -> Quant {
        Quant {
            model_id: file.rsplit_once('.').map_or(file, |(stem, _)| stem).to_string(),
            path: format!("https://huggingface.co/{repo}/resolve/{REV}/{file}"),
            file_size: None,
            size_bytes: Some(size),
            sha256: Some("cd".repeat(32)),
        }
    }

    fn entry(dev: &str, name: &str, quants: Vec<Quant>) -> CatalogEntry {
        CatalogEntry {
            model_name: name.into(),
            developer: dev.into(),
            downloads: Some(10),
            created_at: None,
            pipeline_tag: None,
            tools: None,
            quants,
            description: Some("desc".into()),
            revision: Some(REV.into()),
            embedding: None,
        }
    }

    #[test]
    fn quant_labels() {
        assert_eq!(quant_label("gemma-3-4b-it-Q4_K_M").as_deref(), Some("q4_k_m"));
        assert_eq!(quant_label("Phi-4-mini-instruct.Q8_0").as_deref(), Some("q8_0"));
        assert_eq!(quant_label("bge-small-en-v1.5-f16").as_deref(), Some("f16"));
        assert_eq!(quant_label("gemma-UD-Q6_K_XL").as_deref(), Some("q6_k_xl"));
        assert_eq!(quant_label("m-IQ4_XS").as_deref(), Some("iq4_xs"));
        assert_eq!(quant_label("Q4_K_M/m-Q8_0").as_deref(), Some("q8_0"));
        assert_eq!(quant_label("model"), None);
        assert_eq!(quant_label("Qwen3-4B"), None);
        assert_eq!(quant_label("llama-3.2-instruct"), None);
    }

    #[test]
    fn ids_are_stable_across_fallback_repos() {
        assert_eq!(base_id("All-MiniLM-L6-v2-Embedding-GGUF"), "all-minilm-l6-v2");
        assert_eq!(base_id("all-MiniLM-L6-v2-GGUF"), "all-minilm-l6-v2");
        assert_eq!(base_id("Qwen3-4B-Instruct-2507-GGUF"), "qwen3-4b-instruct-2507");
        assert_eq!(base_id("-gguf"), "-gguf"); // never collapses to an empty id
    }

    #[test]
    fn whisper_ids_parse() {
        assert_eq!(parse_whisper_id("ggml-small"), Some(("small", false, None)));
        assert_eq!(parse_whisper_id("ggml-small.en-q5_1"), Some(("small", true, Some("q5_1".into()))));
        assert_eq!(parse_whisper_id("ggml-large-v3-turbo"), Some(("large-v3-turbo", false, None)));
        assert_eq!(parse_whisper_id("ggml-large-v3-q5_0"), Some(("large-v3", false, Some("q5_0".into()))));
        assert_eq!(parse_whisper_id("ggml-large-v3.en"), None);
        assert_eq!(parse_whisper_id("ggml-small-encoder.mlmodelc"), None);
        assert_eq!(parse_whisper_id("ggml-silero-v5.1.2"), None);
        assert_eq!(parse_whisper_id("small"), None);
    }

    #[test]
    fn whisper_files_group_into_one_model_per_family_and_language() {
        let e = entry("ggerganov", "whisper.cpp", vec![
            quant("ggerganov/whisper.cpp", "ggml-small.bin", 488_000_000),
            quant("ggerganov/whisper.cpp", "ggml-small-q5_1.bin", 190_000_000),
            quant("ggerganov/whisper.cpp", "ggml-small.en.bin", 488_000_001),
            quant("ggerganov/whisper.cpp", "ggml-tiny.bin", 77_000_000),
            quant("ggerganov/whisper.cpp", "ggml-small-encoder.mlmodelc.zip", 5),
        ]);
        let (models, skipped) = build_whisper(&[e]);
        let ids: Vec<_> = models.iter().map(|m| m.catalog_id.as_str()).collect();
        assert_eq!(ids, ["whisper-tiny", "whisper-small", "whisper-small-en"]);
        let small = &models[1];
        assert_eq!(small.display_name, "Whisper Small");
        assert_eq!(small.author, "ggerganov");
        let variants: Vec<_> = small.variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(variants, ["q5_1", "standard"], "smallest file first");
        assert_eq!(small.variants[1].file.filename, "ggml-small.bin");
        assert_eq!(small.variants[0].ram_bytes, 190_000_000 + 390 * MIB);
        assert_eq!(skipped.files_rejected, 1);
    }

    #[test]
    fn llm_models_drop_unverifiable_files_and_entries() {
        let mut unverified = quant("o/a", "a-Q4_K_M.gguf", 1_000);
        unverified.sha256 = None;
        let entries = vec![
            entry("unsloth", "gemma-3-4b-it-GGUF", vec![
                quant("unsloth/gemma-3-4b-it-GGUF", "gemma-3-4b-it-Q8_0.gguf", 4_000_000_000),
                quant("unsloth/gemma-3-4b-it-GGUF", "gemma-3-4b-it-Q4_K_M.gguf", 2_500_000_000),
                unverified.clone(),
            ]),
            entry("someone", "legacy-GGUF", vec![unverified]),
        ];
        let (models, skipped) = build_llm(&entries);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].catalog_id, "unsloth/gemma-3-4b-it");
        assert_eq!(models[0].display_name, "gemma-3-4b-it");
        let ids: Vec<_> = models[0].variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["q4_k_m", "q8_0"]);
        assert_eq!(models[0].variants[0].ram_bytes, 2_500_000_000 / 10 * 12 + GIB * 3 / 2);
        assert_eq!(skipped.entries_without_verified_files, 1);
        assert_eq!(skipped.files_rejected, 2);
    }

    #[test]
    fn llm_list_puts_recommended_models_first_then_by_downloads() {
        let mk = |dev: &str, name: &str, downloads: u64| {
            let mut e = entry(dev, name, vec![quant(&format!("{dev}/{name}"), "m-Q4_K_M.gguf", 1_000)]);
            e.downloads = Some(downloads);
            e
        };
        let (models, _) = build_llm(&[
            mk("popular", "Huge-GGUF", 9_000_000),
            mk("ggml-org", "gemma-3-1b-it-GGUF", 5),
            mk("unsloth", "Qwen3-4B-Instruct-2507-GGUF", 1),
            mk("other", "Mid-GGUF", 500),
        ]);
        let ids: Vec<_> = models.iter().map(|m| m.catalog_id.as_str()).collect();
        assert_eq!(ids, ["unsloth/qwen3-4b-instruct-2507", "ggml-org/gemma-3-1b-it", "popular/huge", "other/mid"]);
    }

    #[test]
    fn duplicate_llm_ids_keep_the_more_downloaded_entry() {
        let mk = |downloads| {
            let mut e = entry("a", "X-GGUF", vec![quant("a/X-GGUF", "x-Q4_K_M.gguf", 1_000)]);
            e.downloads = Some(downloads);
            e
        };
        let (models, _) = build_llm(&[mk(1), mk(50), mk(7)]);
        assert_eq!((models.len(), models[0].downloads), (1, 50));
    }

    #[test]
    fn embedding_needs_metadata_and_the_first_repo_for_an_id_wins() {
        let meta = EmbeddingMeta { dimension: 384, max_sequence_tokens: 256, query_prefix: String::new(), document_prefix: String::new() };
        let mut first = entry("leliuga", "all-MiniLM-L6-v2-GGUF", vec![
            quant("leliuga/all-MiniLM-L6-v2-GGUF", "all-MiniLM-L6-v2.Q8_0.gguf", 30_000_000),
            quant("leliuga/all-MiniLM-L6-v2-GGUF", "all-MiniLM-L6-v2.F16.gguf", 45_000_000),
        ]);
        first.embedding = Some(meta.clone());
        let mut fallback = entry("second-state", "All-MiniLM-L6-v2-Embedding-GGUF", vec![
            quant("second-state/x", "all-MiniLM-L6-v2-Q8_0.gguf", 31_000_000),
        ]);
        fallback.embedding = Some(meta);
        let no_meta = entry("x", "bge-small-en-v1.5-gguf", vec![quant("x/y", "b-Q8_0.gguf", 1_000)]);

        let (models, skipped) = build_embedding(&[first, fallback, no_meta]);
        assert_eq!(models.len(), 1);
        let m = &models[0];
        assert_eq!((m.catalog_id.as_str(), m.author.as_str()), ("all-minilm-l6-v2", "leliuga"));
        assert_eq!(m.embedding.as_ref().unwrap().dimension, 384);
        assert_eq!(m.variants.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["q8_0", "f16"]);
        assert_eq!(m.variants[0].ram_bytes, 30_000_000 + 256 * MIB);
        assert_eq!(skipped.entries_without_verified_files, 1);
    }

    #[test]
    fn importance_matrices_and_projectors_are_not_model_variants() {
        let e = entry("unsloth", "m-GGUF", vec![
            quant("unsloth/m-GGUF", "imatrix_unsloth.gguf", 9_000_000),
            quant("unsloth/m-GGUF", "mmproj-F16.gguf", 800_000_000),
            quant("unsloth/m-GGUF", "m-Q4_K_M.gguf", 2_000_000_000),
            quant("unsloth/m-GGUF", "BF16/m-BF16.gguf", 5_000_000_000),
        ]);
        let (models, skipped) = build_llm(&[e]);
        let ids: Vec<_> = models[0].variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["q4_k_m", "bf16"]);
        assert_eq!(skipped.files_rejected, 2);
    }

    #[test]
    fn embedders_only_offer_five_bit_and_higher_quantizations() {
        let mut e = entry("o", "x-GGUF", vec![
            quant("o/x", "x.Q2_K.gguf", 19_000_000),
            quant("o/x", "x.Q4_K_M.gguf", 20_000_000),
            quant("o/x", "x.Q5_K_M.gguf", 21_000_000),
            quant("o/x", "x.Q8_0.gguf", 25_000_000),
            quant("o/x", "x.F16.gguf", 45_000_000),
        ]);
        e.embedding = Some(EmbeddingMeta { dimension: 384, max_sequence_tokens: 256, query_prefix: String::new(), document_prefix: String::new() });
        let ids: Vec<_> = build_embedding(&[e]).0.remove(0).variants.into_iter().map(|v| v.id).collect();
        assert_eq!(ids, ["q5_k_m", "q8_0", "f16"]);
    }

    #[test]
    fn a_zero_dimension_embedding_is_never_offered() {
        let mut e = entry("a", "e-GGUF", vec![quant("a/e", "e-Q8_0.gguf", 1_000)]);
        e.embedding = Some(EmbeddingMeta { dimension: 0, max_sequence_tokens: 1, query_prefix: String::new(), document_prefix: String::new() });
        assert!(build_embedding(&[e]).0.is_empty());
    }
}
