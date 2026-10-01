use crate::rendezvous::sha256_hex;
use crate::security::{
    run_bounded_command, run_bounded_command_stream_to_file, secure_command,
    verify_secure_open_file, SecureDir, SecureDirCleanupGuard,
};
use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteItem {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_remote: bool,
    pub thumbnail: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFolderEnvelope {
    pub version: u32,
    pub subfolder: String,
    pub timestamp_epoch_secs: u64,
    pub items: Vec<RemoteItem>,
}

/// Varsayilan Google Drive yerel dosya onbellegi
pub fn default_gdrive_cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cache/omastudio/gdrive")
}

/// Metadata onbelleginin barindigi guvenli alt dizin
pub fn gdrive_metadata_cache_dir() -> PathBuf {
    default_gdrive_cache_dir().join("metadata")
}

pub const METADATA_CACHE_TTL_SECS: u64 = 300; // 5 dakika taze onbellek
pub const METADATA_STALE_MAX_AGE_SECS: u64 = 7 * 86400; // 7 gun stale fallback
pub const MAX_METADATA_CACHE_BYTES: usize = 1024 * 1024; // 1 MiB HANCORE tavani
pub const MAX_RAW_DOWNLOAD_BYTES: usize = 250 * 1024 * 1024; // 250 MiB sinir

/// rclone uzerinde gdrive uzaginin yapilandirildigini denetler
pub fn is_gdrive_available() -> bool {
    let mut cmd = secure_command("rclone");
    cmd.arg("listremotes");
    if let Ok((code, stdout, _)) = run_bounded_command(cmd, Duration::from_secs(5)) {
        if code == 0 {
            let out = String::from_utf8_lossy(&stdout);
            return out.contains("gdrive:");
        }
    }
    false
}

/// Alt klasor yolundan guvenli ve deterministik onbellek dosya adi uretir
fn derive_metadata_filename(clean_subfolder: &str) -> String {
    let hash = sha256_hex(clean_subfolder.as_bytes());
    format!("dir_{}.json", &hash[..32])
}

/// Diskte saklanan metadata onbellegini HANCORE guvenlik kurallariyla okur
pub fn read_cached_folder(clean_subfolder: &str, max_age_secs: u64) -> Option<Vec<RemoteItem>> {
    let meta_dir = gdrive_metadata_cache_dir();
    let secure_dir = SecureDir::open_or_create_hierarchy(&meta_dir).ok()?;
    let fname = derive_metadata_filename(clean_subfolder);
    let c_name = CString::new(fname.as_bytes()).ok()?;

    let file = secure_dir.open_existing_file_ro(&c_name).ok()?;
    let _ = verify_secure_open_file(&file, MAX_METADATA_CACHE_BYTES).ok()?;

    let mut buf = Vec::new();
    file.take((MAX_METADATA_CACHE_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .ok()?;

    if buf.len() > MAX_METADATA_CACHE_BYTES {
        return None;
    }

    let mut envelope: CachedFolderEnvelope = serde_json::from_slice(&buf).ok()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if now.saturating_sub(envelope.timestamp_epoch_secs) > max_age_secs {
        return None;
    }

    // Dinamik kucuk resim (thumbnail) zenginlestirmesi
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let thumb_dir = PathBuf::from(&home).join(".cache/omastudio/thumbnails");
    for item in &mut envelope.items {
        if !item.is_dir && item.thumbnail.is_empty() {
            let stem = Path::new(&item.name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("thumb");
            let candidate = thumb_dir.join(format!("{}.jpg", stem));
            if candidate.exists() {
                item.thumbnail = candidate.to_string_lossy().to_string();
            }
        }
    }

    Some(envelope.items)
}

/// Metadata listesini guvenli, atomik ve Mode 0600 izinleriyle diske yazar
pub fn write_cached_folder(clean_subfolder: &str, items: &[RemoteItem]) -> Result<(), String> {
    let meta_dir = gdrive_metadata_cache_dir();
    let secure_dir = SecureDir::open_or_create_hierarchy(&meta_dir)
        .map_err(|e| format!("Could not open metadata cache directory: {}", e))?;

    let fname = derive_metadata_filename(clean_subfolder);
    let c_dest_name = CString::new(fname.as_bytes())
        .map_err(|e| format!("Invalid metadata filename: {}", e))?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let envelope = CachedFolderEnvelope {
        version: 1,
        subfolder: clean_subfolder.to_string(),
        timestamp_epoch_secs: now,
        items: items.to_vec(),
    };

    let json_bytes = serde_json::to_vec(&envelope)
        .map_err(|e| format!("Failed to serialize metadata cache: {}", e))?;

    if json_bytes.len() > MAX_METADATA_CACHE_BYTES {
        return Err("Metadata cache exceeds maximum allowable size".to_string());
    }

    let (mut staging_file, c_staging_name) = secure_dir
        .open_staging_file(".tmp_meta", &fname)
        .map_err(|e| format!("Failed to create staging metadata file: {}", e))?;

    let mut cleanup_guard = SecureDirCleanupGuard {
        secure_dir: &secure_dir,
        filename: c_staging_name.clone(),
        active: true,
    };

    use std::io::Write;
    staging_file
        .write_all(&json_bytes)
        .map_err(|e| format!("Failed to write metadata staging file: {}", e))?;
    staging_file
        .sync_all()
        .map_err(|e| format!("Failed to sync metadata staging file: {}", e))?;

    secure_dir
        .rename_file(&c_staging_name, &c_dest_name)
        .map_err(|e| format!("Failed to atomically rename metadata cache: {}", e))?;

    cleanup_guard.active = false;
    Ok(())
}

/// Geriye uyumluluk icin sarmalayici fonksiyon
pub fn list_gdrive_folder(subfolder: &str) -> Result<Vec<RemoteItem>, String> {
    list_gdrive_folder_opt(subfolder, false)
}

/// Google Drive klasor listelemesi (Disk TTL onbellegi ve Stale-Fallback mimarisi)
pub fn list_gdrive_folder_opt(subfolder: &str, force_refresh: bool) -> Result<Vec<RemoteItem>, String> {
    let clean_sub = subfolder.trim_matches('/');
    if clean_sub.contains("..") || clean_sub.contains('\0') {
        return Err("Invalid subfolder path: directory traversal or null bytes detected".to_string());
    }

    // 1. Onbellek Kontrolu (Eger zorunlu tazeleme istenmediyse)
    if !force_refresh {
        if let Some(cached_items) = read_cached_folder(clean_sub, METADATA_CACHE_TTL_SECS) {
            return Ok(cached_items);
        }
    }

    // 2. rclone ile Optimize Edilmis Canli API Listelemesi
    let target = if clean_sub.is_empty() {
        "gdrive:".to_string()
    } else {
        format!("gdrive:{}", clean_sub)
    };

    let mut cmd = secure_command("rclone");
    cmd.arg("lsjson")
        .arg("--max-depth")
        .arg("1")
        .arg("--fast-list")
        .arg("--drive-skip-gdocs")
        .arg("--drive-skip-shortcuts")
        .arg("--drive-skip-dangling-shortcuts")
        .arg("--drive-pacer-min-sleep")
        .arg("10ms")
        .arg("--drive-pacer-burst")
        .arg("200")
        .arg("--drive-list-chunk")
        .arg("1000")
        .arg("--retries")
        .arg("2")
        .arg("--low-level-retries")
        .arg("2")
        .arg("--")
        .arg(&target);

    let res = run_bounded_command(cmd, Duration::from_secs(35));

    // 3. API Basarisizlik veya 403 Rate Limit Durumunda Stale Fallback
    match res {
        Ok((0, stdout, _stderr)) => {
            #[derive(Deserialize)]
            struct RcloneEntry {
                #[serde(rename = "Path")]
                path: String,
                #[serde(rename = "Name")]
                name: String,
                #[serde(rename = "Size")]
                size: i64,
                #[serde(rename = "IsDir")]
                is_dir: bool,
            }

            let entries: Vec<RcloneEntry> = serde_json::from_slice(&stdout)
                .map_err(|e| format!("Failed to parse rclone json: {}", e))?;

            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            let thumb_dir = PathBuf::from(&home).join(".cache/omastudio/thumbnails");

            let mut items = Vec::new();
            for entry in entries {
                let full_remote_path = if clean_sub.is_empty() {
                    entry.path.clone()
                } else {
                    format!("{}/{}", clean_sub, entry.path)
                };

                let is_raw = entry.is_dir || {
                    let lower = entry.name.to_lowercase();
                    lower.ends_with(".nef")
                        || lower.ends_with(".nrw")
                        || lower.ends_with(".raf")
                        || lower.ends_with(".cr2")
                        || lower.ends_with(".cr3")
                        || lower.ends_with(".arw")
                        || lower.ends_with(".dng")
                        || lower.ends_with(".rwl")
                        || lower.ends_with(".orf")
                        || lower.ends_with(".rw2")
                };

                if is_raw {
                    let thumb = if !entry.is_dir {
                        let stem = Path::new(&entry.name)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("thumb");
                        let thumb_candidate = thumb_dir.join(format!("{}.jpg", stem));
                        if thumb_candidate.exists() {
                            thumb_candidate.to_string_lossy().to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };

                    items.push(RemoteItem {
                        name: entry.name,
                        path: full_remote_path,
                        is_dir: entry.is_dir,
                        is_remote: true,
                        thumbnail: thumb,
                        size: entry.size,
                    });
                }
            }

            items.sort_by(|a, b| {
                match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                }
            });

            // Diske atomik olarak onbellekle
            let _ = write_cached_folder(clean_sub, &items);

            Ok(items)
        }
        err_case => {
            // Canli sorgu basarisiz oldu veya 403 Rate Limit alindi. Stale cache var mi?
            if let Some(stale_items) = read_cached_folder(clean_sub, METADATA_STALE_MAX_AGE_SECS) {
                eprintln!("[WARN] Google Drive API rate-limited or offline. Serving stale metadata cache for '{}'", clean_sub);
                return Ok(stale_items);
            }

            match err_case {
                Ok((code, _, stderr)) => Err(format!(
                    "rclone exited with code {}: {}",
                    code,
                    String::from_utf8_lossy(&stderr)
                )),
                Err(e) => Err(format!("Failed to execute rclone: {}", e)),
            }
        }
    }
}

/// Uzak RAW dosyasini yerel guvenli onbellege indirir (Hizlandirilmis pacer bayraklari ile)
pub fn fetch_remote_raw(remote_path: &str) -> Result<PathBuf, String> {
    if remote_path.contains("..") || remote_path.contains('\0') {
        return Err("Invalid remote path: directory traversal or null bytes detected".to_string());
    }

    let cache_dir = default_gdrive_cache_dir();
    let secure_dir = SecureDir::open_or_create_hierarchy(&cache_dir)
        .map_err(|e| format!("Could not create secure cache directory hierarchy: {}", e))?;

    let clean_path = remote_path.trim_start_matches('/');
    let target_remote = format!("gdrive:{}", clean_path);

    let filename = Path::new(clean_path)
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid remote file path".to_string())?;

    if filename.is_empty() || filename.starts_with('.') {
        return Err("Invalid file name".to_string());
    }

    let local_dest = cache_dir.join(filename);
    let c_dest_name = CString::new(filename.as_bytes())
        .map_err(|e| format!("Invalid filename string: {}", e))?;

    // Onceden indirilmis dosya dogrulamasi
    if let Ok(cached_file) = secure_dir.open_existing_file_ro(&c_dest_name) {
        let is_valid = (|| -> Option<()> {
            let len = crate::security::verify_secure_open_file(&cached_file, MAX_RAW_DOWNLOAD_BYTES).ok()?;
            if len < 1024 {
                return None;
            }
            if crate::raw::RawImage::open_from_file(&cached_file).is_err() {
                return None;
            }
            Some(())
        })()
        .is_some();

        if is_valid {
            return Ok(local_dest);
        } else {
            let _ = secure_dir.unlink_file(&c_dest_name);
        }
    }

    let (mut staging_file, c_staging_name) = secure_dir
        .open_staging_file(".tmp_gdrive", filename)
        .map_err(|e| format!("Failed to create private staging file in cache directory: {}", e))?;

    let mut cleanup_guard = SecureDirCleanupGuard {
        secure_dir: &secure_dir,
        filename: c_staging_name.clone(),
        active: true,
    };

    let mut cmd = secure_command("rclone");
    cmd.arg("cat")
        .arg("--drive-pacer-min-sleep")
        .arg("10ms")
        .arg("--drive-pacer-burst")
        .arg("200")
        .arg("--drive-skip-gdocs")
        .arg("--retries")
        .arg("3")
        .arg("--low-level-retries")
        .arg("3")
        .arg("--")
        .arg(&target_remote);

    run_bounded_command_stream_to_file(
        cmd,
        &mut staging_file,
        Duration::from_secs(180),
        MAX_RAW_DOWNLOAD_BYTES,
    )
    .map_err(|e| format!("Failed to download from Google Drive: {}", e))?;

    crate::security::verify_secure_open_file(&staging_file, MAX_RAW_DOWNLOAD_BYTES)
        .map_err(|e| format!("Staging file security verification failed: {}", e))?;

    crate::raw::RawImage::open_from_file(&staging_file)
        .map_err(|e| format!("Downloaded file is not a valid RAW image: {}", e))?;

    staging_file
        .sync_all()
        .map_err(|e| format!("Failed to sync staging file: {}", e))?;

    secure_dir
        .rename_file(&c_staging_name, &c_dest_name)
        .map_err(|e| format!("Failed to atomically publish downloaded file into cache: {}", e))?;

    cleanup_guard.active = false;
    drop(staging_file);

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let thumb_dir = PathBuf::from(&home).join(".cache/omastudio/thumbnails");
    let _ = SecureDir::open_or_create_hierarchy(&thumb_dir);
    let stem = Path::new(&filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("thumb");
    let thumb_path = thumb_dir.join(format!("{}.jpg", stem));
    if !thumb_path.exists() {
        if let Ok(raw) = crate::raw::RawImage::open(&local_dest) {
            let _ = raw.extract_thumbnail(&thumb_path);
        }
    }

    Ok(local_dest)
}

/// Islenmis fotograflari yuksek hizli parcali yukleme bayraklariyla dogrudan Drive'a yukler
pub fn upload_export_to_gdrive(local_path: &Path, remote_dest_folder: &str) -> Result<String, String> {
    if !local_path.exists() {
        return Err("Export file does not exist locally".to_string());
    }

    let clean_dest = remote_dest_folder.trim_matches('/');
    if clean_dest.contains("..") || clean_dest.contains('\0') {
        return Err("Invalid destination folder: traversal detected".to_string());
    }
    let target = if clean_dest.is_empty() {
        "gdrive:Photos/Exports".to_string()
    } else {
        format!("gdrive:{}", clean_dest)
    };

    let filename = local_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid filename".to_string())?;

    let mut cmd = secure_command("rclone");
    cmd.arg("copy")
        .arg("--no-traverse")
        .arg("--drive-chunk-size")
        .arg("32M")
        .arg("--drive-upload-cutoff")
        .arg("32M")
        .arg("--drive-pacer-min-sleep")
        .arg("10ms")
        .arg("--drive-pacer-burst")
        .arg("200")
        .arg("--retries")
        .arg("3")
        .arg("--low-level-retries")
        .arg("3")
        .arg("--")
        .arg(local_path)
        .arg(&target);

    let (code, _, stderr) = run_bounded_command(cmd, Duration::from_secs(60))
        .map_err(|e| format!("Failed to upload to Google Drive: {}", e))?;

    if code != 0 {
        return Err(format!(
            "rclone upload failed: {}",
            String::from_utf8_lossy(&stderr)
        ));
    }

    Ok(format!("{}/{}", target, filename))
}
