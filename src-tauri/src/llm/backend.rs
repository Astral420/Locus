//! Chooses the llama backend for the resolved `llama-server`.
//!
//! This module only decides; launch flags are applied elsewhere. The patched
//! Intel-macOS engine exposes Metal devices through `--list-devices`, while a
//! stock engine has no manifest and must never be assumed to support AMD Metal.

use super::engine::{resolve_llama_server, ResolvedEngine};
use crate::transcription::{detect_backend, BackendKind};
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};

const LIST_DEVICES_TIMEOUT: Duration = Duration::from_secs(10);
const DEVICE_ENV: &str = "LOCUS_LLAMA_DEVICE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlamaBackend {
    Cpu,
    AppleMetal,
    Vulkan,
    MetalAmd {
        device_index: u32,
        device_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendChoice {
    pub backend: LlamaBackend,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceLine {
    pub backend_prefix: String,
    pub index: u32,
    pub name: String,
    pub total_mib: u64,
}

pub struct DetectInput<'a> {
    pub macos_x86_64: bool,
    pub apple_silicon: bool,
    pub engine: &'a ResolvedEngine,
    pub list_devices: Option<&'a str>,
    pub device_override: Option<&'a str>,
}

/// Parses `llama-server --list-devices` output. Lines that do not match the
/// `PREFIX<index>: name (N MiB, ...)` shape are skipped.
pub fn parse_list_devices(output: &str) -> Vec<DeviceLine> {
    output.lines().filter_map(parse_device_line).collect()
}

fn parse_device_line(line: &str) -> Option<DeviceLine> {
    let (head, rest) = line.trim().split_once(':')?;
    let head = head.trim();
    let digit_start = head.find(|character: char| character.is_ascii_digit())?;
    let (prefix, digits) = head.split_at(digit_start);
    if prefix.is_empty()
        || !prefix
            .chars()
            .all(|character| character.is_ascii_alphabetic())
        || digits.is_empty()
        || !digits.chars().all(|character| character.is_ascii_digit())
    {
        return None;
    }
    let index = digits.parse::<u32>().ok()?;
    let (name, total_mib) = split_name_and_memory(rest.trim());
    if name.is_empty() {
        return None;
    }
    Some(DeviceLine {
        backend_prefix: prefix.to_ascii_uppercase(),
        index,
        name,
        total_mib,
    })
}

/// Device names may themselves contain parentheses (for example `Intel(R)`),
/// so only a trailing `(... MiB ...)` group is treated as memory information.
fn split_name_and_memory(rest: &str) -> (String, u64) {
    if let Some(open) = rest.rfind('(') {
        if rest.ends_with(')') {
            let inside = &rest[open + 1..rest.len() - 1];
            if inside.to_ascii_lowercase().contains("mib") {
                let name = rest[..open].trim().to_owned();
                let total_mib = inside
                    .split(|character: char| !character.is_ascii_digit())
                    .find(|part| !part.is_empty())
                    .and_then(|part| part.parse().ok())
                    .unwrap_or(0);
                return (name, total_mib);
            }
        }
    }
    (rest.to_owned(), 0)
}

/// True only when the device name identifies an AMD/Radeon GPU. Intel and
/// Apple integrated devices are rejected even if their names contain a match.
pub fn is_amd_gpu(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.contains("intel") || lower.contains("apple") {
        return false;
    }
    lower.contains("amd") || lower.contains("radeon")
}

enum Override<'a> {
    Cpu,
    Auto,
    Index(u32),
    Text(&'a str),
}

fn parse_override(value: Option<&str>) -> Override<'_> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("cpu") => Override::Cpu,
        Some(value) if value.eq_ignore_ascii_case("auto") => Override::Auto,
        Some(value) => match value.parse::<u32>() {
            Ok(index) => Override::Index(index),
            Err(_) => Override::Text(value),
        },
        None => Override::Auto,
    }
}

fn map_existing_backend(backend: BackendKind) -> LlamaBackend {
    match backend {
        BackendKind::Metal => LlamaBackend::AppleMetal,
        BackendKind::Vulkan => LlamaBackend::Vulkan,
        BackendKind::Cuda | BackendKind::Rocm | BackendKind::Cpu => LlamaBackend::Cpu,
    }
}

fn backend_label(backend: &LlamaBackend) -> String {
    match backend {
        LlamaBackend::Cpu => "Cpu".into(),
        LlamaBackend::AppleMetal => "AppleMetal".into(),
        LlamaBackend::Vulkan => "Vulkan".into(),
        LlamaBackend::MetalAmd { device_index, .. } => format!("MetalAmd[{device_index}]"),
    }
}

fn metal_devices(list_devices: &str) -> Vec<DeviceLine> {
    parse_list_devices(list_devices)
        .into_iter()
        .filter(|device| device.backend_prefix == "MTL")
        .collect()
}

fn metal_amd_choice(device: &DeviceLine, reason: String) -> BackendChoice {
    BackendChoice {
        backend: LlamaBackend::MetalAmd {
            device_index: device.index,
            device_name: device.name.clone(),
        },
        reason,
    }
}

/// Pure backend decision. First matching rule wins.
pub fn choose_llama_backend(input: &DetectInput<'_>) -> BackendChoice {
    let overridden = parse_override(input.device_override);
    if matches!(&overridden, Override::Cpu) {
        return BackendChoice {
            backend: LlamaBackend::Cpu,
            reason: format!("{DEVICE_ENV}=cpu forces the CPU backend"),
        };
    }

    if !input.macos_x86_64 {
        if input.apple_silicon {
            return BackendChoice {
                backend: LlamaBackend::AppleMetal,
                reason: "Apple Silicon: existing Metal backend selection is unchanged".into(),
            };
        }
        let existing = detect_backend();
        let backend = map_existing_backend(existing);
        return BackendChoice {
            reason: format!(
                "non-Intel-macOS: existing backend detection ({existing:?}) maps to {}",
                backend_label(&backend)
            ),
            backend,
        };
    }

    match input.engine.manifest.as_ref() {
        None => {
            return BackendChoice {
                backend: LlamaBackend::Cpu,
                reason: "stock llama-server: no AMD Metal support".into(),
            }
        }
        Some(manifest) if manifest.arch != "x86_64" => {
            return BackendChoice {
                backend: LlamaBackend::Cpu,
                reason: format!(
                    "engine manifest arch `{}` is not x86_64: no AMD Metal support",
                    manifest.arch
                ),
            }
        }
        Some(_) => {}
    }

    let Some(list_devices) = input.list_devices else {
        return BackendChoice {
            backend: LlamaBackend::Cpu,
            reason: "no --list-devices output: cannot confirm a Metal device".into(),
        };
    };
    let metal = metal_devices(list_devices);
    if metal.is_empty() {
        return BackendChoice {
            backend: LlamaBackend::Cpu,
            reason: "`--list-devices` listed no Metal devices".into(),
        };
    }

    let amd: Vec<&DeviceLine> = metal
        .iter()
        .filter(|device| is_amd_gpu(&device.name))
        .collect();
    if amd.is_empty() {
        return BackendChoice {
            backend: LlamaBackend::Cpu,
            reason: "no AMD GPU listed by `--list-devices`".into(),
        };
    }

    let mut warning = None;
    match &overridden {
        Override::Index(index) => {
            let index = *index;
            match metal.iter().find(|device| device.index == index) {
                Some(device) if is_amd_gpu(&device.name) => {
                    return metal_amd_choice(
                        device,
                        format!(
                            "{DEVICE_ENV}={index} selects {} ({})",
                            device.name, device.backend_prefix
                        ),
                    )
                }
                Some(device) => {
                    warning = Some(format!(
                        "ignored {DEVICE_ENV}={index}: {} is not an AMD GPU",
                        device.name
                    ));
                }
                None => {
                    warning = Some(format!(
                        "ignored {DEVICE_ENV}={index}: device is not listed"
                    ));
                }
            }
        }
        Override::Text(value) => {
            warning = Some(format!(
                "ignored {DEVICE_ENV}={value}: expected cpu, auto, or a non-negative integer"
            ));
        }
        Override::Cpu | Override::Auto => {}
    }

    let best = amd
        .iter()
        .copied()
        .reduce(|best, candidate| {
            if candidate.total_mib > best.total_mib
                || (candidate.total_mib == best.total_mib && candidate.index < best.index)
            {
                candidate
            } else {
                best
            }
        })
        .expect("AMD device list is not empty");

    let selection = if amd.len() == 1 {
        format!(
            "only AMD GPU listed: {} ({} MiB)",
            best.name, best.total_mib
        )
    } else {
        format!(
            "largest of {} AMD GPUs listed: {} ({} MiB)",
            amd.len(),
            best.name,
            best.total_mib
        )
    };
    let reason = match warning {
        Some(warning) => format!("{warning}; {selection}"),
        None => selection,
    };
    metal_amd_choice(best, reason)
}

/// Resolves the engine, runs one timed `--list-devices`, and caches the result
/// for the process lifetime. Spawn errors and timeouts become CPU results.
pub fn detect_llama_backend() -> BackendChoice {
    static CACHE: OnceLock<BackendChoice> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let engine = resolve_llama_server();
            let device_override = std::env::var(DEVICE_ENV).ok();
            let overridden = parse_override(device_override.as_deref());
            let macos_x86_64 = cfg!(all(target_os = "macos", target_arch = "x86_64"));
            let apple_silicon = cfg!(all(target_os = "macos", target_arch = "aarch64"));
            let needs_list = macos_x86_64
                && !matches!(&overridden, Override::Cpu)
                && engine
                    .manifest
                    .as_ref()
                    .is_some_and(|manifest| manifest.arch == "x86_64");

            let list_devices = if needs_list {
                match run_list_devices(&engine.binary) {
                    Ok(output) => Some(output),
                    Err(error) => {
                        let choice = BackendChoice {
                            backend: LlamaBackend::Cpu,
                            reason: format!("`llama-server --list-devices` failed: {error}"),
                        };
                        eprintln!(
                            "locus: llama backend {:?} ({})",
                            choice.backend, choice.reason
                        );
                        return choice;
                    }
                }
            } else {
                None
            };

            let input = DetectInput {
                macos_x86_64,
                apple_silicon,
                engine: &engine,
                list_devices: list_devices.as_deref(),
                device_override: device_override.as_deref(),
            };
            let choice = choose_llama_backend(&input);
            eprintln!(
                "locus: llama backend {:?} ({})",
                choice.backend, choice.reason
            );
            choice
        })
        .clone()
}

fn run_list_devices(binary: &Path) -> Result<String, String> {
    let mut command = Command::new(binary);
    command
        .arg("--list-devices")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy();
        if key.starts_with("TOSH_") || key.starts_with("GGML_METAL_") {
            command.env_remove(key.as_ref());
        }
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to spawn {}: {error}", binary.display()))?;
    let deadline = Instant::now() + LIST_DEVICES_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "timed out after {} s",
                        LIST_DEVICES_TIMEOUT.as_secs()
                    ));
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("failed to wait for the process: {error}"));
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|error| format!("failed to collect output: {error}"))?;
    let mut merged = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !merged.ends_with('\n') && !merged.is_empty() {
            merged.push('\n');
        }
        merged.push_str(&stderr);
    }
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::super::engine::{EngineManifest, EngineSource};
    use super::*;
    use std::path::PathBuf;

    const SINGLE_AMD: &str = include_str!("testdata/list-devices-single-amd.txt");
    const AMD_PLUS_INTEL: &str = include_str!("testdata/list-devices-amd-plus-intel.txt");
    const TWO_AMD: &str = include_str!("testdata/list-devices-two-amd.txt");
    const AMD_TIE: &str = include_str!("testdata/list-devices-amd-tie.txt");
    const CPU_ONLY: &str = include_str!("testdata/list-devices-cpu-only.txt");

    fn manifest(arch: &str) -> EngineManifest {
        EngineManifest {
            toshllm_tag: "test-tag".into(),
            toshllm_commit: "test-commit".into(),
            llama_commit: "test-llama".into(),
            arch: arch.into(),
            has_kernels: true,
            llama_server_sha256: "test-sha".into(),
        }
    }

    fn engine(arch: &str) -> ResolvedEngine {
        ResolvedEngine {
            binary: PathBuf::from("/nonexistent/llama-server"),
            source: EngineSource::Path,
            manifest: Some(manifest(arch)),
        }
    }

    fn stock_engine() -> ResolvedEngine {
        ResolvedEngine {
            binary: PathBuf::from("/nonexistent/llama-server"),
            source: EngineSource::Path,
            manifest: None,
        }
    }

    fn intel_mac<'a>(
        engine: &'a ResolvedEngine,
        list_devices: Option<&'a str>,
        device_override: Option<&'a str>,
    ) -> DetectInput<'a> {
        DetectInput {
            macos_x86_64: true,
            apple_silicon: false,
            engine,
            list_devices,
            device_override,
        }
    }

    #[test]
    fn parses_a_single_amd_gpu() {
        let devices = parse_list_devices(SINGLE_AMD);
        assert_eq!(devices.len(), 1);
        assert_eq!(
            devices[0],
            DeviceLine {
                backend_prefix: "MTL".into(),
                index: 0,
                name: "AMD Radeon RX 6600".into(),
                total_mib: 8176,
            }
        );
    }

    #[test]
    fn parses_amd_plus_intel_igpu_and_ignores_the_igpu() {
        let devices = parse_list_devices(AMD_PLUS_INTEL);
        assert_eq!(devices.len(), 2);
        assert!(is_amd_gpu(&devices[0].name));
        assert!(!is_amd_gpu(&devices[1].name));
        assert_eq!(devices[1].name, "Intel(R) UHD Graphics 630");
        assert_eq!(devices[1].total_mib, 1536);

        let engine = engine("x86_64");
        let choice = choose_llama_backend(&intel_mac(&engine, Some(AMD_PLUS_INTEL), None));
        assert_eq!(
            choice.backend,
            LlamaBackend::MetalAmd {
                device_index: 0,
                device_name: "AMD Radeon RX 6600".into(),
            }
        );
    }

    #[test]
    fn chooses_the_largest_amd_gpu_of_two() {
        let engine = engine("x86_64");
        let choice = choose_llama_backend(&intel_mac(&engine, Some(TWO_AMD), None));
        assert_eq!(
            choice.backend,
            LlamaBackend::MetalAmd {
                device_index: 1,
                device_name: "AMD Radeon RX 6600".into(),
            }
        );
        assert!(choice.reason.contains("largest"));
    }

    #[test]
    fn ties_choose_the_lowest_index() {
        let engine = engine("x86_64");
        let choice = choose_llama_backend(&intel_mac(&engine, Some(AMD_TIE), None));
        assert_eq!(
            choice.backend,
            LlamaBackend::MetalAmd {
                device_index: 0,
                device_name: "AMD Radeon RX 6600".into(),
            }
        );
    }

    #[test]
    fn garbage_and_empty_output_yield_no_devices() {
        for output in ["", "garbage\n???\nnot a device"] {
            assert!(parse_list_devices(output).is_empty());
            let engine = engine("x86_64");
            let choice = choose_llama_backend(&intel_mac(&engine, Some(output), None));
            assert_eq!(choice.backend, LlamaBackend::Cpu);
            assert!(choice.reason.contains("no Metal devices"));
        }
        let engine = engine("x86_64");
        let choice = choose_llama_backend(&intel_mac(&engine, None, None));
        assert_eq!(choice.backend, LlamaBackend::Cpu);
        assert!(choice.reason.contains("no --list-devices output"));
    }

    #[test]
    fn cpu_only_listing_yields_cpu() {
        let devices = parse_list_devices(CPU_ONLY);
        assert!(devices.is_empty());

        let engine = engine("x86_64");
        let choice = choose_llama_backend(&intel_mac(&engine, Some(CPU_ONLY), None));
        assert_eq!(choice.backend, LlamaBackend::Cpu);
        assert!(choice.reason.contains("no Metal devices"));
    }

    #[test]
    fn override_cpu_wins() {
        let engine = engine("x86_64");
        let choice = choose_llama_backend(&intel_mac(&engine, Some(AMD_PLUS_INTEL), Some("cpu")));
        assert_eq!(choice.backend, LlamaBackend::Cpu);
        assert!(choice.reason.contains("LOCUS_LLAMA_DEVICE=cpu"));

        let choice = choose_llama_backend(&intel_mac(&engine, Some(AMD_PLUS_INTEL), Some(" CPU ")));
        assert_eq!(choice.backend, LlamaBackend::Cpu);
    }

    #[test]
    fn override_index_used_only_when_listed_and_amd() {
        let engine = engine("x86_64");

        let chosen = choose_llama_backend(&intel_mac(&engine, Some(AMD_PLUS_INTEL), Some("0")));
        assert_eq!(
            chosen.backend,
            LlamaBackend::MetalAmd {
                device_index: 0,
                device_name: "AMD Radeon RX 6600".into(),
            }
        );
        assert!(chosen.reason.contains("LOCUS_LLAMA_DEVICE=0 selects"));

        let intel_override =
            choose_llama_backend(&intel_mac(&engine, Some(AMD_PLUS_INTEL), Some("1")));
        assert_eq!(
            intel_override.backend,
            LlamaBackend::MetalAmd {
                device_index: 0,
                device_name: "AMD Radeon RX 6600".into(),
            }
        );
        assert!(intel_override
            .reason
            .contains("ignored LOCUS_LLAMA_DEVICE=1"));

        let missing_override =
            choose_llama_backend(&intel_mac(&engine, Some(AMD_PLUS_INTEL), Some("9")));
        assert_eq!(
            missing_override.backend,
            LlamaBackend::MetalAmd {
                device_index: 0,
                device_name: "AMD Radeon RX 6600".into(),
            }
        );
        assert!(missing_override
            .reason
            .contains("ignored LOCUS_LLAMA_DEVICE=9"));
    }

    #[test]
    fn unknown_override_text_means_auto() {
        let engine = engine("x86_64");
        for value in ["auto", "banana", "-1", "3.5", ""] {
            let choice = choose_llama_backend(&intel_mac(&engine, Some(TWO_AMD), Some(value)));
            assert_eq!(
                choice.backend,
                LlamaBackend::MetalAmd {
                    device_index: 1,
                    device_name: "AMD Radeon RX 6600".into(),
                },
                "override {value:?}"
            );
        }
    }

    #[test]
    fn stock_engine_without_manifest_never_selects_metal_amd() {
        let engine = stock_engine();
        let choice = choose_llama_backend(&intel_mac(&engine, Some(SINGLE_AMD), None));
        assert_eq!(choice.backend, LlamaBackend::Cpu);
        assert_eq!(choice.reason, "stock llama-server: no AMD Metal support");
    }

    #[test]
    fn arm64_manifest_is_rejected() {
        let engine = engine("arm64");
        let choice = choose_llama_backend(&intel_mac(&engine, Some(SINGLE_AMD), None));
        assert_eq!(choice.backend, LlamaBackend::Cpu);
        assert!(choice.reason.contains("arm64"));
    }

    #[test]
    fn apple_silicon_and_non_macos_results_match_the_existing_detection() {
        let engine = engine("x86_64");

        let apple = choose_llama_backend(&DetectInput {
            macos_x86_64: false,
            apple_silicon: true,
            engine: &engine,
            list_devices: Some(AMD_PLUS_INTEL),
            device_override: None,
        });
        assert_eq!(apple.backend, LlamaBackend::AppleMetal);

        let existing = detect_backend();
        let expected = map_existing_backend(existing);
        let other = choose_llama_backend(&DetectInput {
            macos_x86_64: false,
            apple_silicon: false,
            engine: &engine,
            list_devices: Some(AMD_PLUS_INTEL),
            device_override: None,
        });
        assert_eq!(other.backend, expected);
    }

    #[test]
    fn parses_every_real_capture_in_testdata() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/llm/testdata");
        let engine = engine("x86_64");
        let mut captures = 0;
        for entry in std::fs::read_dir(&dir).expect("testdata directory") {
            let path = entry.expect("testdata entry").path();
            let file_name = path
                .file_name()
                .expect("testdata file name")
                .to_string_lossy()
                .into_owned();
            if !file_name.starts_with("list-devices-real-") || !file_name.ends_with(".txt") {
                continue;
            }
            captures += 1;
            let output = std::fs::read_to_string(&path).expect("read real capture");
            let devices = parse_list_devices(&output);
            assert!(
                !devices.is_empty(),
                "{file_name} did not parse to any device"
            );
            let has_amd = devices
                .iter()
                .any(|device| device.backend_prefix == "MTL" && is_amd_gpu(&device.name));
            let choice = choose_llama_backend(&intel_mac(&engine, Some(&output), None));
            if has_amd {
                assert!(
                    matches!(choice.backend, LlamaBackend::MetalAmd { .. }),
                    "{file_name}: expected MetalAmd, got {:?} ({})",
                    choice.backend,
                    choice.reason
                );
            }
        }
        assert!(
            captures > 0,
            "no list-devices-real-*.txt captures found in {}",
            dir.display()
        );
    }

    #[test]
    fn whisper_detection_is_unchanged() {
        // Whisper's own tests are untouched; this module only maps its result.
        assert_eq!(
            map_existing_backend(BackendKind::Metal),
            LlamaBackend::AppleMetal
        );
        assert_eq!(
            map_existing_backend(BackendKind::Vulkan),
            LlamaBackend::Vulkan
        );
        assert_eq!(map_existing_backend(BackendKind::Cpu), LlamaBackend::Cpu);
        assert_eq!(map_existing_backend(BackendKind::Cuda), LlamaBackend::Cpu);
        assert_eq!(map_existing_backend(BackendKind::Rocm), LlamaBackend::Cpu);
    }

    #[test]
    #[ignore]
    fn print_real_detection() {
        let choice = detect_llama_backend();
        println!("backend: {:?}", choice.backend);
        println!("reason: {}", choice.reason);
    }
}
