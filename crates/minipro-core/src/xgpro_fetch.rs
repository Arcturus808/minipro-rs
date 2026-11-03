//! Pure-Rust replacement for `dump-alg-minipro.bash`.
//!
//! Downloads (or reads locally) the official XGPro setup archives, extracts
//! the T56/T76 FPGA algorithm files and firmware images from the nested
//! RAR5 payloads, and generates `algorithm.xml` — without bsdtar, wget,
//! sha256sum, strings, or any other external tool.
//!
//! Archive layout (verified against XGPro V1304 / T76 V1319):
//!
//! ```text
//! xgproV1304_..._Setup.rar      outer RAR5, one member
//!   └── XgproV1304_Setup.exe    WinRAR SFX — PE stub + inner RAR5 at 0x4e600
//!         ├── updateII.dat / UpdateT48.dat / updateT56.dat   (firmware)
//!         └── algorithm/*.alg                                (354 files)
//! xgpro_T76_V1319.rar           same structure
//!   └── Xgpro_T76_V1319.exe
//!         ├── UpdateT76.Dat
//!         └── algoT76/*.alg                                  (370 files)
//! ```
//!
//! Generated files are written to the per-user data directory by default
//! (see [`crate::database::user_data_dir`]), which the database resolver
//! already searches — no elevated permissions needed.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use flate2::write::GzEncoder;
use flate2::Compression;
use rar_codec::{Archive, EntryKind};
use sha2::{Digest, Sha256};

use crate::algorithm::decode_bitstream;
use crate::database::user_data_dir;
use crate::device::ProgrammerModel;
use crate::error::MiniproError;

// ── Pinned upstream archives ────────────────────────────────────────────────

const XGPRO_URL_BASE: &str =
    "https://github.com/Kreeblah/XGecu_Software/raw/refs/heads/master/Xgpro/13";

const RAR5_SIG: &[u8] = b"Rar!\x1a\x07";

/// Which upstream payload an archive is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    /// T48/T56/TL866II+ payload (algorithm/ + three firmware images).
    Main,
    /// T76 payload (algoT76/ + one firmware image).
    T76,
}

struct ArchiveSpec {
    role: Role,
    /// Download filename (also used for the cache copy).
    file: &'static str,
    sha256: &'static str,
    /// Inner-RAR directory holding this role's .alg files.
    alg_dir: &'static str,
    /// Firmware members: (lowercase basename, model, version). Output name
    /// is `firmware-<model>-<version>.dat`.
    firmware: &'static [(&'static str, &'static str, &'static str)],
}

const MAIN_SPEC: ArchiveSpec = ArchiveSpec {
    role: Role::Main,
    file: "xgproV1304_T48_T56_T866II_Setup.rar",
    sha256: "821db3ef1cc2b335d8a1e50ad37161032f804c8626cd3c1e7d03695d9aa75b1d",
    alg_dir: "algorithm",
    firmware: &[
        ("updateii.dat", "TL866II+", "04.2.132"),
        ("updatet48.dat", "T48", "01.1.35"),
        ("updatet56.dat", "T56", "01.1.73"),
    ],
};

const T76_SPEC: ArchiveSpec = ArchiveSpec {
    role: Role::T76,
    file: "xgpro_T76_V1319.rar",
    sha256: "8541d3d0f47a5d7dc1727e7b6dc41db7bf1132a4b6b549f2947a8cd210c40490",
    alg_dir: "algoT76",
    firmware: &[("updatet76.dat", "T76", "00.1.17")],
};

const SPECS: &[&ArchiveSpec] = &[&MAIN_SPEC, &T76_SPEC];

/// Expected algorithm counts for the pinned versions — warn-only, so a
/// future XGPro repack doesn't hard-fail on a count change.
const EXPECTED_T56: usize = 354;
const EXPECTED_T76: usize = 370;

// ── .alg payload offsets (from the bash script) ─────────────────────────────

/// T56: `tail -c +545` → bytes from 0x220 on.
const T56_PAYLOAD_OFFSET: usize = 544;
/// T76 description: `tail -c +17 | head -c 4080` → bytes 16..4096.
const T76_DESC_START: usize = 16;
const T76_DESC_END: usize = 4096;
/// T76 header fragment: `tail -c +5 | head -c 8` → bytes 4..12.
const T76_HDR_START: usize = 4;
const T76_HDR_END: usize = 12;
/// T76 payload: `tail -c +4097` → bytes from 0x1000 on.
const T76_PAYLOAD_OFFSET: usize = 4096;

// ── Public API ───────────────────────────────────────────────────────────────

/// Progress notifications. The callback returns `false` to abort the fetch
/// (e.g. GUI cancel button or a dropped channel).
#[derive(Debug)]
pub enum Progress {
    /// Started downloading `archive` (`total` may be unknown).
    DownloadStart {
        archive: &'static str,
        total: Option<u64>,
    },
    /// Cumulative bytes received so far.
    DownloadBytes {
        archive: &'static str,
        received: u64,
    },
    /// Download body fully read (before checksum verification).
    DownloadFinished { archive: &'static str },
    /// A named pipeline stage began: "verifying", "extracting".
    Stage {
        archive: &'static str,
        stage: &'static str,
    },
    /// .alg files processed so far for this archive.
    Processed {
        archive: &'static str,
        done: usize,
        total: usize,
    },
    /// An output file was written.
    Written { path: PathBuf },
}

/// Options for [`fetch`].
#[derive(Default)]
pub struct FetchOptions<'a> {
    /// Output directory for `algorithm.xml` and `firmware-*.dat`.
    /// `None` → the per-user data directory ([`user_data_dir`]).
    pub output_dir: Option<PathBuf>,
    /// Local archives replacing downloads. Each file is auto-detected as
    /// the main or T76 payload; accepts the outer `.rar`, the SFX `.exe`,
    /// or the bare inner RAR. Supply up to two files (one per role).
    /// Archives without a local file are downloaded normally.
    pub input_files: &'a [PathBuf],
    /// Download URL base override for mirrors (`<base>/<filename>`).
    /// The pinned SHA-256 is still enforced.
    pub url_base: Option<&'a str>,
    /// Progress callback; return `false` to cancel.
    pub progress: Option<&'a mut dyn FnMut(Progress) -> bool>,
}

/// A firmware image written to the output directory.
pub struct FirmwareWritten {
    pub model: &'static str,
    pub version: &'static str,
    pub path: PathBuf,
}

/// What [`fetch`] produced.
pub struct FetchOutcome {
    pub output_dir: PathBuf,
    pub xml_path: PathBuf,
    pub firmware: Vec<FirmwareWritten>,
    pub t56_algorithms: usize,
    pub t76_algorithms: usize,
    /// Non-fatal issues worth showing the user (corrupt cache, count drift,
    /// checksum mismatches on `--from-file` inputs, …).
    pub warnings: Vec<String>,
}

/// Run the full download → extract → generate → verify pipeline.
///
/// Writes `algorithm.xml` plus `firmware-*.dat` into the output directory
/// atomically (tmp + rename, overwriting existing files).
pub fn fetch(opts: &mut FetchOptions) -> Result<FetchOutcome, MiniproError> {
    let output_dir = match &opts.output_dir {
        Some(d) => d.clone(),
        None => user_data_dir().ok_or_else(|| {
            MiniproError::FileFormat(
                "cannot determine the per-user data directory (HOME/LOCALAPPDATA unset); \
                 pass --output"
                    .into(),
            )
        })?,
    };
    fs::create_dir_all(&output_dir)?;

    let cache_dir = user_data_dir()
        .unwrap_or_else(|| output_dir.clone())
        .join("cache");

    // Classify each --from-file input once (reads the file, then drops the
    // buffer — the archive is re-read lazily when its role is needed, which
    // keeps peak memory at one archive at a time).
    let mut local: [Option<PathBuf>; 2] = [None, None];
    for path in opts.input_files {
        let data = fs::read(path)?;
        match file_role(&data) {
            Some(role) => {
                let slot = match role {
                    Role::Main => 0,
                    Role::T76 => 1,
                };
                if local[slot].is_some() {
                    return Err(MiniproError::FileFormat(format!(
                        "{}: duplicate {} payload — only one archive per role",
                        path.display(),
                        role_name(role)
                    )));
                }
                local[slot] = Some(path.clone());
            }
            None => {
                return Err(MiniproError::FileFormat(format!(
                    "{}: not a recognized XGPro archive (no algorithm/ or algoT76/ entries)",
                    path.display()
                )))
            }
        }
    }

    let mut warnings = Vec::new();
    let mut t56_entries = Vec::new();
    let mut t76_entries = Vec::new();
    // (out_name, model, version, bytes); first archive to provide a name wins.
    let mut firmware: Vec<(String, &'static str, &'static str, Vec<u8>)> = Vec::new();

    for spec in SPECS {
        let slot = match spec.role {
            Role::Main => 0,
            Role::T76 => 1,
        };
        let bytes = acquire_archive(
            spec,
            local[slot].as_deref(),
            opts,
            &cache_dir,
            &mut warnings,
        )?;
        emit(
            opts,
            Progress::Stage {
                archive: spec.file,
                stage: "extracting",
            },
        )?;
        let inner = unwrap_inner(&bytes)
            .map_err(|e| MiniproError::FileFormat(format!("{}: {}", spec.file, e)))?;
        let extracted = process_inner(&inner, spec, opts)?;
        match spec.role {
            Role::Main => t56_entries = extracted.algorithms,
            Role::T76 => t76_entries = extracted.algorithms,
        }
        for fw in extracted.firmware {
            if !firmware.iter().any(|(n, ..)| *n == fw.0) {
                firmware.push(fw);
            }
        }
    }

    // Deterministic order (glob order in the script was locale-dependent).
    t56_entries.sort_by(|a, b| a.name.cmp(&b.name));
    t76_entries.sort_by(|a, b| a.name.cmp(&b.name));

    if t56_entries.len() != EXPECTED_T56 {
        warnings.push(format!(
            "expected {} T56 algorithms for this XGPro version, found {}",
            EXPECTED_T56,
            t56_entries.len()
        ));
    }
    if t76_entries.len() != EXPECTED_T76 {
        warnings.push(format!(
            "expected {} T76 algorithms for this XGPro version, found {}",
            EXPECTED_T76,
            t76_entries.len()
        ));
    }

    // Generate + write, then verify the written file through the consumer
    // decode path.
    let xml = build_xml(&t56_entries, &t76_entries);
    let xml_path = output_dir.join("algorithm.xml");
    write_atomic(&xml_path, xml.as_bytes())?;
    emit(
        opts,
        Progress::Written {
            path: xml_path.clone(),
        },
    )?;

    let (v56, v76) = verify_xml(&xml_path)?;
    let generated = t56_entries.len() + t76_entries.len();
    if v56 + v76 != generated {
        return Err(MiniproError::Xml(format!(
            "verification decoded {}/{} generated algorithms",
            v56 + v76,
            generated
        )));
    }

    let mut firmware_out = Vec::new();
    for (name, model, version, data) in firmware {
        let path = output_dir.join(&name);
        write_atomic(&path, &data)?;
        emit(opts, Progress::Written { path: path.clone() })?;
        firmware_out.push(FirmwareWritten {
            model,
            version,
            path,
        });
    }

    Ok(FetchOutcome {
        output_dir,
        xml_path,
        firmware: firmware_out,
        t56_algorithms: t56_entries.len(),
        t76_algorithms: t76_entries.len(),
        warnings,
    })
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Main => "T48/T56",
        Role::T76 => "T76",
    }
}

// ── Progress plumbing ────────────────────────────────────────────────────────

fn emit(opts: &mut FetchOptions, ev: Progress) -> Result<(), MiniproError> {
    if let Some(cb) = opts.progress.as_deref_mut() {
        if !cb(ev) {
            return Err(MiniproError::Cancelled);
        }
    }
    Ok(())
}

// ── Acquisition: local file, cache, or download ─────────────────────────────

/// Obtain the outer-archive bytes for `spec`: a matching `--from-file`
/// input, a hash-verified cache copy, or a fresh download.
fn acquire_archive(
    spec: &ArchiveSpec,
    local_path: Option<&Path>,
    opts: &mut FetchOptions,
    cache_dir: &Path,
    warnings: &mut Vec<String>,
) -> Result<Vec<u8>, MiniproError> {
    if let Some(path) = local_path {
        let data = fs::read(path)?;
        let got = format!("{:x}", Sha256::digest(&data));
        if !got.eq_ignore_ascii_case(spec.sha256) {
            warnings.push(format!(
                "{}: sha256 {} differs from the pinned {} archive — \
                 treating it as a different XGPro release",
                path.display(),
                got,
                spec.file
            ));
        }
        return Ok(data);
    }

    // Cache: a hash-verified copy avoids the download entirely.
    let cache_path = cache_dir.join(spec.file);
    if let Ok(cached) = fs::read(&cache_path) {
        let got = format!("{:x}", Sha256::digest(&cached));
        if got.eq_ignore_ascii_case(spec.sha256) {
            return Ok(cached);
        }
        warnings.push(format!(
            "cached {} failed sha256 verification — downloading again",
            cache_path.display()
        ));
    }

    download(spec, opts, &cache_path)
}

fn download(
    spec: &ArchiveSpec,
    opts: &mut FetchOptions,
    cache_path: &Path,
) -> Result<Vec<u8>, MiniproError> {
    let base = opts
        .url_base
        .unwrap_or(XGPRO_URL_BASE)
        .trim_end_matches('/');
    let url = format!("{}/{}", base, spec.file);

    let agent = ureq::Agent::config_builder()
        // OS trust store (Schannel/Secure Transport/system roots) so
        // corporate MITM proxies whose CA is installed locally still work.
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_global(Some(Duration::from_secs(300)))
        .timeout_connect(Some(Duration::from_secs(30)))
        .build()
        .new_agent();
    let mut resp = agent
        .get(&url)
        .call()
        .map_err(|e| MiniproError::Download(format!("{}: {}", url, e)))?;

    let total = resp.body().content_length();
    emit(
        opts,
        Progress::DownloadStart {
            archive: spec.file,
            total,
        },
    )?;

    let mut data = Vec::with_capacity(total.unwrap_or(0).min(128 << 20) as usize);
    let mut hasher = Sha256::new();
    let mut reader = resp.body_mut().as_reader();
    let mut chunk = [0u8; 65536];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        hasher.update(&chunk[..n]);
        data.extend_from_slice(&chunk[..n]);
        emit(
            opts,
            Progress::DownloadBytes {
                archive: spec.file,
                received: data.len() as u64,
            },
        )?;
    }
    emit(opts, Progress::DownloadFinished { archive: spec.file })?;

    emit(
        opts,
        Progress::Stage {
            archive: spec.file,
            stage: "verifying sha256",
        },
    )?;
    let got = format!("{:x}", hasher.finalize());
    if !got.eq_ignore_ascii_case(spec.sha256) {
        return Err(MiniproError::Download(format!(
            "{}: sha256 mismatch — expected {}, got {}",
            spec.file, spec.sha256, got
        )));
    }

    // Cache population is best-effort.
    if let Some(dir) = cache_path.parent() {
        if fs::create_dir_all(dir).is_ok() {
            let _ = write_atomic(cache_path, &data);
        }
    }
    Ok(data)
}

// ── Archive unwrapping ───────────────────────────────────────────────────────

/// Identify which role a `--from-file` payload fills, by looking at the
/// inner archive's directory layout. `None` for unrelated files.
fn file_role(data: &[u8]) -> Option<Role> {
    let inner = unwrap_inner(data).ok()?;
    let arc = Archive::open(&inner).ok()?;
    let mut has_main = false;
    for e in arc.entries() {
        if e.kind != EntryKind::File {
            continue;
        }
        let norm = e.name.replace('\\', "/");
        let norm = norm.trim_start_matches("./").to_lowercase();
        if norm.starts_with("algot76/") && norm.ends_with(".alg") {
            return Some(Role::T76);
        }
        if norm.starts_with("algorithm/") && norm.ends_with(".alg") {
            has_main = true;
        }
    }
    has_main.then_some(Role::Main)
}

/// Reduce an input blob to the inner RAR5 archive bytes.
///
/// Accepts:
///   - the outer `.rar` (extracts the setup `.exe` member, then unwraps it),
///   - the SFX `.exe` (scans for the embedded RAR5 signature),
///   - the bare inner RAR.
fn unwrap_inner(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.starts_with(RAR5_SIG) {
        let arc = Archive::open(data).map_err(|e| format!("invalid RAR: {}", e))?;
        let files: Vec<&rar_codec::Entry> = arc
            .entries()
            .iter()
            .filter(|e| e.kind == EntryKind::File)
            .collect();
        let has_algs = files
            .iter()
            .any(|e| e.name.replace('\\', "/").to_lowercase().contains(".alg"));
        if !has_algs {
            // Outer archive: find the setup executable member.
            let exe = files
                .iter()
                .find(|e| e.name.to_lowercase().ends_with(".exe"))
                .copied()
                .or_else(|| (files.len() == 1).then_some(files[0]))
                .ok_or_else(|| "outer archive has no executable member".to_string())?;
            let exe_data = arc
                .extract(exe)
                .map_err(|e| format!("extracting {}: {}", exe.name, e))?;
            return unwrap_sfx(&exe_data);
        }
        // Bare inner RAR.
        return Ok(data.to_vec());
    }
    // Assume SFX executable.
    unwrap_sfx(data)
}

/// Locate the embedded RAR5 inside a WinRAR SFX executable and return the
/// archive bytes from the signature onward.
fn unwrap_sfx(exe: &[u8]) -> Result<Vec<u8>, String> {
    for (i, w) in exe.windows(RAR5_SIG.len()).enumerate() {
        if w == RAR5_SIG {
            let candidate = &exe[i..];
            match Archive::open(candidate) {
                Ok(a) if a.entries().iter().any(|e| e.kind == EntryKind::File) => {
                    return Ok(candidate.to_vec());
                }
                _ => continue,
            }
        }
    }
    Err("no embedded RAR5 archive found (not a WinRAR SFX?)".into())
}

// ── Inner-archive processing ─────────────────────────────────────────────────

struct AlgEntry {
    name: String,
    description: String,
    bitstream_b64: String,
}

struct Extracted {
    algorithms: Vec<AlgEntry>,
    firmware: Vec<(String, &'static str, &'static str, Vec<u8>)>,
}

fn process_inner(
    inner: &[u8],
    spec: &ArchiveSpec,
    opts: &mut FetchOptions,
) -> Result<Extracted, MiniproError> {
    let arc = Archive::open(inner)
        .map_err(|e| MiniproError::FileFormat(format!("inner archive: {}", e)))?;
    let alg_prefix = format!("{}/", spec.alg_dir.to_lowercase());

    let alg_indices: Vec<usize> = arc
        .entries()
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            if e.kind != EntryKind::File {
                return false;
            }
            let norm = e.name.replace('\\', "/");
            let norm = norm.trim_start_matches("./").to_lowercase();
            norm.starts_with(&alg_prefix) && norm.ends_with(".alg")
        })
        .map(|(i, _)| i)
        .collect();

    let total = alg_indices.len();
    let mut algorithms = Vec::with_capacity(total);
    for (done, &idx) in alg_indices.iter().enumerate() {
        let entry = &arc.entries()[idx];
        let data = arc
            .extract(entry)
            .map_err(|e| MiniproError::FileFormat(format!("extracting {}: {}", entry.name, e)))?;
        algorithms.push(match spec.role {
            Role::Main => munge_t56(&entry.name, &data),
            Role::T76 => munge_t76(&entry.name, &data),
        });
        if done % 32 == 0 || done + 1 == total {
            emit(
                opts,
                Progress::Processed {
                    archive: spec.file,
                    done: done + 1,
                    total,
                },
            )?;
        }
    }

    let mut firmware = Vec::new();
    for entry in arc.entries() {
        if entry.kind != EntryKind::File {
            continue;
        }
        let base = entry
            .name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&entry.name)
            .to_lowercase();
        if let Some((_, model, version)) =
            spec.firmware.iter().find(|(n, _, _)| *n == base.as_str())
        {
            let data = arc.extract(entry).map_err(|e| {
                MiniproError::FileFormat(format!("extracting {}: {}", entry.name, e))
            })?;
            firmware.push((
                format!("firmware-{}-{}.dat", model, version),
                *model,
                *version,
                data,
            ));
        }
    }

    Ok(Extracted {
        algorithms,
        firmware,
    })
}

// ── .alg → XML entry munging ─────────────────────────────────────────────────

fn munge_t56(member_name: &str, data: &[u8]) -> AlgEntry {
    let name = alg_name(member_name, false);
    // `read -d ""`: description is everything up to the first NUL.
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let description = clean_desc(&String::from_utf8_lossy(&data[..end]));
    let payload = if data.len() > T56_PAYLOAD_OFFSET {
        &data[T56_PAYLOAD_OFFSET..]
    } else {
        &[]
    };
    AlgEntry {
        name,
        description,
        bitstream_b64: gzip_b64(payload, Compression::best()),
    }
}

fn munge_t76(member_name: &str, data: &[u8]) -> AlgEntry {
    let name = alg_name(member_name, true);
    // `strings` over bytes 16..4096: printable runs ≥4, newline-joined.
    let region = if data.len() > T76_DESC_START {
        &data[T76_DESC_START..data.len().min(T76_DESC_END)]
    } else {
        &[]
    };
    let description = clean_desc(&gnu_strings(region).join("\n"));
    // bitstream = bytes 4..12 ++ bytes 4096..
    let mut payload = Vec::with_capacity(8 + data.len().saturating_sub(T76_PAYLOAD_OFFSET));
    if data.len() > T76_HDR_START {
        payload.extend_from_slice(&data[T76_HDR_START..data.len().min(T76_HDR_END)]);
    }
    if data.len() > T76_PAYLOAD_OFFSET {
        payload.extend_from_slice(&data[T76_PAYLOAD_OFFSET..]);
    }
    AlgEntry {
        name,
        description,
        bitstream_b64: gzip_b64(&payload, Compression::default()),
    }
}

/// `<basename>` with the T76 `T7_` handling and `${file%.*}` extension
/// strip from the script. `strip_t7` removes the first `T7_` substring
/// (matches bash `${file/T7_/}` — `T76_*` survives because the `6` breaks
/// the pattern).
fn alg_name(member_name: &str, strip_t7: bool) -> String {
    let base = member_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(member_name);
    let base = if strip_t7 {
        base.replacen("T7_", "", 1)
    } else {
        base.to_string()
    };
    match base.rsplit_once('.') {
        Some((stem, _)) => stem.to_string(),
        None => base,
    }
}

/// GNU `strings` semantics: maximal runs of ≥4 printable-ASCII bytes.
fn gnu_strings(region: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, &b) in region.iter().enumerate() {
        let printable = (0x20..=0x7e).contains(&b);
        match (printable, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s >= 4 {
                    out.push(String::from_utf8_lossy(&region[s..i]).into_owned());
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        if region.len() - s >= 4 {
            out.push(String::from_utf8_lossy(&region[s..]).into_owned());
        }
    }
    out
}

/// Keep only characters legal in XML 1.0 attribute values (tab, LF, CR,
/// and everything ≥ U+0020). Deliberate improvement over the script, which
/// embeds raw bytes and can produce invalid XML.
fn clean_desc(s: &str) -> String {
    s.chars()
        .filter(|&c| c == '\t' || c == '\n' || c == '\r' || c >= ' ')
        .collect()
}

fn gzip_b64(payload: &[u8], level: Compression) -> String {
    let mut enc = GzEncoder::new(Vec::new(), level);
    enc.write_all(payload).expect("Vec write cannot fail");
    let gz = enc.finish().expect("gzip finish cannot fail");
    base64::engine::general_purpose::STANDARD.encode(gz)
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

fn build_xml(t56: &[AlgEntry], t76: &[AlgEntry]) -> String {
    let mut xml = String::from("<root>\n<database type=\"ALGORITHMS\">\n<algorithms_T56>\n");
    for e in t56 {
        push_entry(&mut xml, e);
    }
    xml.push_str("</algorithms_T56>\n<algorithms_T76>\n");
    for e in t76 {
        push_entry(&mut xml, e);
    }
    xml.push_str("</algorithms_T76>\n</database>\n</root>\n");
    xml
}

fn push_entry(xml: &mut String, e: &AlgEntry) {
    xml.push_str("<algorithm name=\"");
    xml.push_str(&xml_escape(&e.name));
    xml.push_str("\"\ndescription=\"");
    xml.push_str(&xml_escape(&e.description));
    xml.push_str("\"\nbitstream=\"");
    xml.push_str(&e.bitstream_b64);
    xml.push_str("\"/>\n");
}

// ── Verification ─────────────────────────────────────────────────────────────

/// Re-parse the generated file and decode every bitstream through the real
/// consumer path (base64 → gunzip → CRC32 → T76 level-2). Returns
/// (t56_ok, t76_ok) counts.
fn verify_xml(path: &Path) -> Result<(usize, usize), MiniproError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let file = fs::File::open(path)?;
    let mut reader = Reader::from_reader(std::io::BufReader::new(file));
    reader.config_mut().trim_text(true);

    let mut section: Option<ProgrammerModel> = None;
    let mut depth = 0u32;
    let mut counts = (0usize, 0usize);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                match tag.as_str() {
                    "algorithms_T56" => {
                        section = Some(ProgrammerModel::T56);
                        depth = 1;
                    }
                    "algorithms_T76" => {
                        section = Some(ProgrammerModel::T76);
                        depth = 1;
                    }
                    _ if section.is_some() => depth += 1,
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if let Some(model) = section.filter(|_| tag == "algorithm") {
                    let mut name = String::new();
                    let mut bitstream = None;
                    for attr in e.attributes().flatten() {
                        let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                        let val = std::str::from_utf8(&attr.value).unwrap_or("");
                        match key {
                            "name" => name = val.to_string(),
                            "bitstream" => bitstream = Some(val.to_string()),
                            _ => {}
                        }
                    }
                    let b64 = bitstream.ok_or_else(|| {
                        MiniproError::Xml(format!(
                            "generated entry '{}' has no bitstream attribute",
                            name
                        ))
                    })?;
                    decode_bitstream(&b64, model).map_err(|e| {
                        MiniproError::Xml(format!(
                            "generated algorithm '{}' failed verification: {}",
                            name, e
                        ))
                    })?;
                    match model {
                        ProgrammerModel::T56 => counts.0 += 1,
                        _ => counts.1 += 1,
                    }
                }
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if section.is_some() {
                    depth = depth.saturating_sub(1);
                    if tag == "algorithms_T56" || tag == "algorithms_T76" || depth == 0 {
                        section = None;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(MiniproError::Xml(format!(
                    "generated algorithm.xml failed to parse: {}",
                    e
                )))
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(counts)
}

// ── Output helpers ───────────────────────────────────────────────────────────

/// Write via a sibling temp file + rename so a crash can't leave a
/// truncated `algorithm.xml` behind. Overwrites existing files.
fn write_atomic(path: &Path, data: &[u8]) -> Result<(), MiniproError> {
    let tmp = path.with_file_name(format!(
        "{}.part",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("out")
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).or_else(|e| {
        // Windows won't rename over an existing read-only target; remove
        // and retry once before giving up.
        if path.exists() && fs::remove_file(path).is_ok() {
            fs::rename(&tmp, path)
        } else {
            Err(e)
        }
    })?;
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Headered bitstream the CRC check accepts: u32 size + u32 crc + data.
    fn valid_bitstream(data: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(&crate::util::crc_32(data, 0xFFFF_FFFF).to_le_bytes());
        v.extend_from_slice(data);
        v
    }

    /// Minimal T56 .alg: NUL-terminated description, padding to 544, then a
    /// header (u32 size, u32 crc) + payload the decoder can verify.
    fn synthetic_t56(desc: &str, payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(desc.as_bytes());
        v.push(0);
        v.resize(T56_PAYLOAD_OFFSET, 0xAA);
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn test_alg_name_t56() {
        assert_eq!(alg_name("algorithm/SPI25F10.alg", false), "SPI25F10");
        assert_eq!(alg_name("algorithm\\MW93ALG.alg", false), "MW93ALG");
        // T56 names keep any T7_ present (no strip for this section).
        assert_eq!(alg_name("algorithm/T7_X.alg", false), "T7_X");
    }

    #[test]
    fn test_alg_name_t76() {
        // T7_ prefix stripped; T76_ survives; mid-string T7_ also stripped
        // (bash ${file/T7_/} semantics).
        assert_eq!(alg_name("algoT76/T7_SPI25F10.alg", true), "SPI25F10");
        assert_eq!(
            alg_name("algoT76/T76_vga1024x768.alg", true),
            "T76_vga1024x768"
        );
        assert_eq!(alg_name("algoT76/Test_100M.alg", true), "Test_100M");
        assert_eq!(alg_name("algoT76/xxT7_yy.alg", true), "xxyy");
    }

    #[test]
    fn test_t56_munge() {
        let data = synthetic_t56("SPI-18", &valid_bitstream(&[1, 2, 3, 4]));
        let e = munge_t56("algorithm/SPI25F10.alg", &data);
        assert_eq!(e.name, "SPI25F10");
        assert_eq!(e.description, "SPI-18");
        // Round-trips through the real decoder.
        decode_bitstream(&e.bitstream_b64, ProgrammerModel::T56).unwrap();
    }

    #[test]
    fn test_t76_strings_desc() {
        let mut data = vec![0u8; T76_DESC_END + 64];
        // Region starts at 16; runs <4 chars must be dropped.
        data[16..25].copy_from_slice(b"W25Q128FV");
        data[26] = b'X'; // run of 1 → dropped
        data[30..40].copy_from_slice(b"SPI-NOR   ");
        let e = munge_t76("algoT76/T7_SPI25F10.alg", &data);
        // Trailing spaces are printable → part of the run (GNU strings parity).
        assert_eq!(e.description, "W25Q128FV\nSPI-NOR   ");
        assert_eq!(e.name, "SPI25F10");
    }

    #[test]
    fn test_t76_payload_layout() {
        // Marker bytes at 4..12 and 4096.. prove offsets end-to-end.
        let mut data = vec![0u8; 8192];
        for (i, b) in data[4..12].iter_mut().enumerate() {
            *b = 0x10 + i as u8;
        }
        data[4096] = 0xEE;
        let e = munge_t76("algoT76/Test_100M.alg", &data);
        // Decompress the emitted bitstream and check byte layout directly
        // (CRC check isn't applicable — this fixture isn't headered).
        let gz = base64::engine::general_purpose::STANDARD
            .decode(&e.bitstream_b64)
            .unwrap();
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(&gz[..])
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(&raw[..8], &data[4..12]);
        assert_eq!(raw[8], 0xEE);
        assert_eq!(raw.len(), 8 + (8192 - 4096));
    }

    #[test]
    fn test_xml_escape_and_clean() {
        assert_eq!(
            xml_escape("a<b>&\"'\""),
            "a&lt;b&gt;&amp;&quot;&apos;&quot;"
        );
        assert_eq!(clean_desc("a\x01b\x7fc\nd"), "ab\x7fc\nd");
    }

    #[test]
    fn test_build_xml_structure() {
        let mk = |n: &str| AlgEntry {
            name: n.into(),
            description: "d".into(),
            bitstream_b64: "AAAA".into(),
        };
        let xml = build_xml(&[mk("B"), mk("A")], &[mk("C")]);
        let a = xml.find("name=\"B\"").unwrap();
        let b = xml.find("name=\"A\"").unwrap();
        let c = xml.find("name=\"C\"").unwrap();
        // Order in file = slice order (sorting is the caller's job).
        assert!(a < b && b < c);
        assert!(xml.starts_with("<root>\n<database type=\"ALGORITHMS\">\n<algorithms_T56>\n"));
        assert!(xml.ends_with("</algorithms_T76>\n</database>\n</root>\n"));
        // Three-line entry layout, matching the script's output.
        assert!(xml.contains("<algorithm name=\"B\"\ndescription=\"d\"\nbitstream=\"AAAA\"/>\n"));
    }

    #[test]
    fn test_nested_rar_roundtrip() {
        // Synthetic fixture: inner RAR with one .alg + one firmware,
        // embedded after a fake SFX stub, inside an outer RAR.
        let alg = synthetic_t56("SPI-18", &valid_bitstream(&[9, 9, 9]));
        let mut inner_builder = rar_codec::ArchiveBuilder::new();
        inner_builder
            .add_stored("algorithm/SPI25F10.alg", &alg)
            .unwrap();
        inner_builder
            .add_stored("updateT56.dat", b"FIRMWAREBYTES")
            .unwrap();
        let inner = inner_builder.finish();

        let mut exe = b"MZ-fake-sfx-stub".to_vec();
        exe.extend_from_slice(&inner);
        let mut outer_builder = rar_codec::ArchiveBuilder::new();
        outer_builder.add_stored("Setup.exe", &exe).unwrap();
        let outer = outer_builder.finish();

        assert_eq!(file_role(&outer), Some(Role::Main));

        let inner_bytes = unwrap_inner(&outer).unwrap();
        let mut opts = FetchOptions::default();
        let ext = process_inner(&inner_bytes, &MAIN_SPEC, &mut opts).unwrap();
        assert_eq!(ext.algorithms.len(), 1);
        assert_eq!(ext.algorithms[0].name, "SPI25F10");
        assert_eq!(ext.algorithms[0].description, "SPI-18");
        decode_bitstream(&ext.algorithms[0].bitstream_b64, ProgrammerModel::T56).unwrap();
        assert_eq!(ext.firmware.len(), 1);
        assert_eq!(ext.firmware[0].0, "firmware-T56-01.1.73.dat");
        assert_eq!(ext.firmware[0].3, b"FIRMWAREBYTES");
    }

    #[test]
    fn test_unwrap_bare_inner_and_exe() {
        let mut builder = rar_codec::ArchiveBuilder::new();
        builder
            .add_stored("algoT76/T7_Test.alg", b"fakealg")
            .unwrap();
        let inner = builder.finish();

        // Bare inner rar input.
        assert_eq!(unwrap_inner(&inner).unwrap(), inner);
        assert_eq!(file_role(&inner), Some(Role::T76));

        // Same bytes behind an SFX stub.
        let mut exe = b"MZ....".to_vec();
        exe.extend_from_slice(&inner);
        assert_eq!(unwrap_inner(&exe).unwrap(), inner);
        assert_eq!(file_role(&exe), Some(Role::T76));
    }

    #[test]
    #[ignore = "downloads ~65MB from GitHub"]
    fn test_real_download() {
        let dir = std::env::temp_dir().join("minipro-xgpro-test");
        let mut opts = FetchOptions {
            output_dir: Some(dir.clone()),
            ..Default::default()
        };
        let out = fetch(&mut opts).unwrap();
        assert_eq!(out.t56_algorithms, EXPECTED_T56);
        assert_eq!(out.t76_algorithms, EXPECTED_T76);
        assert_eq!(out.firmware.len(), 4);
        let _ = fs::remove_dir_all(&dir);
    }
}
