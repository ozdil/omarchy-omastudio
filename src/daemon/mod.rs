//! Persistent JSON-lines processing daemon (`omastudio-engine daemon`).
//!
//! A dedicated stdin reader thread parses bounded JSON lines (<= 1 MiB) and enqueues
//! them into [`CommandQueue`], coalescing back-to-back interactive `adjust` requests
//! so rapid slider drags keep only the freshest recipe while preserving strict
//! ordering across `load`, `save_recipe`, `export`, and `exit`.

pub mod protocol;
pub mod state;
pub mod worker;

use std::collections::VecDeque;
use std::io::{self, BufRead};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};

use omastudio_engine::{
    ai::{
        self, ai_auto_enhance, ai_classify_scene, ai_classify_scene_with_jev, ai_optimize_for_social,
        apply_jev_decisions_to_recipe,
    },
    fuji, gpu, icc,
    pipeline::{
        self, process_buffer_16_to_8_ex, process_split_comparison_16_to_8_ex,
        try_process_preview_dmabuf,
    },
    preview_shm::{
        ColorSpaceTag, PreviewRingWriter, SlotKind, dmabuf_enabled_by_env,
        send_dmabuf_frame_and_wait_ack, take_inherited_preview_fd,
    },
    raw::{self, RawImage},
    recipe::{Catalog, CatalogFilter, Recipe},
    security,
};
use rayon::prelude::*;

use crate::cli::{
    ResponseWrapper,
    fuji::FujiRecipeResult,
    inspect::{InspectResult, JevInspectResult, thumbnail_path_for},
    print_json,
};
use protocol::{DaemonCommand, DetailResult, REFERENCE_SLOT, RenderResult, apply_proof};
use state::{DaemonCache, cap_preview, resample_rect, retouched_full, spawn_full_decode};
use worker::{ExportJob, spawn_export_worker};

const MAX_LINE_BYTES: usize = 1024 * 1024; // 1 MiB JSON line limit

/// Thread-safe command queue that coalesces queued interactive `adjust` commands.
struct CommandQueue {
    inner: Mutex<QueueState>,
    cv: Condvar,
}

struct QueueState {
    items: VecDeque<DaemonCommand>,
    closed: bool,
}

impl CommandQueue {
    fn new() -> Self {
        Self {
            inner: Mutex::new(QueueState {
                items: VecDeque::new(),
                closed: false,
            }),
            cv: Condvar::new(),
        }
    }

    /// Enqueues `cmd`, replacing a pending interactive `adjust` command after the most
    /// recent barrier (`load` / `save_recipe` / `exit`) so stale slider drags are skipped.
    fn push(&self, cmd: DaemonCommand) {
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if cmd.cmd == "adjust" && cmd.out.is_none() {
            // Walk back from the tail until a barrier command; replace any queued interactive adjust.
            for existing in s.items.iter_mut().rev() {
                if existing.cmd == "adjust" && existing.out.is_none() {
                    *existing = cmd;
                    self.cv.notify_one();
                    return;
                }
                if matches!(existing.cmd.as_str(), "load" | "save_recipe" | "exit") {
                    break;
                }
            }
        }
        s.items.push_back(cmd);
        self.cv.notify_one();
    }

    fn close(&self) {
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.closed = true;
        self.cv.notify_all();
    }

    fn pop(&self) -> Option<DaemonCommand> {
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(cmd) = s.items.pop_front() {
                return Some(cmd);
            }
            if s.closed {
                return None;
            }
            s = self.cv.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }
}

fn spawn_stdin_reader(queue: Arc<CommandQueue>) {
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut stdin_lock = stdin.lock();
        let mut raw_buf: Vec<u8> = Vec::new();

        loop {
            raw_buf.clear();
            // Bounded byte read: an oversized line never lands in memory whole, and a stray
            // invalid UTF-8 byte yields an error response instead of silently ending the daemon.
            let bytes_read = match io::Read::take(&mut stdin_lock, MAX_LINE_BYTES as u64 + 1)
                .read_until(b'\n', &mut raw_buf)
            {
                Ok(0) => break, // EOF
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };

            if bytes_read > MAX_LINE_BYTES {
                // Discard the rest of the oversized line
                if raw_buf.last() != Some(&b'\n') {
                    let mut sink = Vec::new();
                    loop {
                        sink.clear();
                        match io::Read::take(&mut stdin_lock, 64 * 1024).read_until(b'\n', &mut sink) {
                            Ok(0) => break,
                            Ok(_) if sink.last() == Some(&b'\n') => break,
                            Ok(_) => continue,
                            Err(_) => break,
                        }
                    }
                }
                print_json::<()>(&ResponseWrapper::err_action(
                    "error",
                    "JSON line exceeded maximum limit of 1 MiB",
                ));
                continue;
            }

            let line_buf = match std::str::from_utf8(&raw_buf) {
                Ok(s) => s,
                Err(_) => {
                    print_json::<()>(&ResponseWrapper::err_action("error", "Command line is not valid UTF-8"));
                    continue;
                }
            };

            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            let cmd_obj: DaemonCommand = match serde_json::from_str(trimmed) {
                Ok(c) => c,
                Err(e) => {
                    print_json::<()>(&ResponseWrapper::err_action("error", format!("Invalid JSON command: {}", e)));
                    continue;
                }
            };

            let is_exit = cmd_obj.cmd == "exit";
            queue.push(cmd_obj);
            if is_exit {
                break;
            }
        }
        queue.close();
    });
}

pub fn run() {
    let mut preview_sock: Option<OwnedFd> = None;
    let mut preview_ring: Option<PreviewRingWriter> = take_inherited_preview_fd().and_then(|sock| {
        match PreviewRingWriter::create()
            .and_then(|ring| ring.send_and_wait_ack(sock.as_raw_fd(), 2_000).map(|()| ring))
        {
            Ok(ring) => {
                preview_sock = Some(sock);
                Some(ring)
            }
            Err(e) => {
                eprintln!("omastudio: memfd preview ring unavailable, falling back to /dev/shm: {e}");
                None
            }
        }
    });
    let mut dmabuf_active = preview_sock.is_some() && dmabuf_enabled_by_env();

    let cache = Arc::new(Mutex::new(DaemonCache::default()));
    let (export_tx, export_worker) = spawn_export_worker();
    let mut warm_up_worker: Option<std::thread::JoinHandle<()>> = None;

    let queue = Arc::new(CommandQueue::new());
    spawn_stdin_reader(Arc::clone(&queue));

    while let Some(cmd_obj) = queue.pop() {
        let req_id = cmd_obj.request_id;

        match cmd_obj.cmd.as_str() {
            "ping" => {
                print_json(&ResponseWrapper::ok_action("ping", "pong").with_context(req_id, None));
            }
            "icc_profiles" => {
                print_json(&ResponseWrapper::ok_action("icc_profiles", icc::list_icc_profiles()).with_context(req_id, None));
            }
            "reference" => {
                // The other side of the compare view: the open photo with another recipe
                // (a snapshot, a version, or the defaults for "before")
                let (buffer, width, height, channels, transfer, baseline, preview_gen, photo_path) = {
                    let c = cache.lock().unwrap_or_else(|e| e.into_inner());
                    match c.raw_buffer_16.as_ref() {
                        Some(b) => (
                            Arc::clone(b),
                            c.width,
                            c.height,
                            c.channels,
                            c.transfer,
                            c.baseline_exposure,
                            c.preview_gen,
                            c.current_path.clone(),
                        ),
                        None => {
                            print_json::<()>(
                                &ResponseWrapper::err_action("reference", "No RAW image currently loaded in daemon cache")
                                    .with_context(req_id, None),
                            );
                            continue;
                        }
                    }
                };
                let recipe = Recipe { baseline_exposure: baseline, ..cmd_obj.recipe.unwrap_or_default() };
                let preview_key = gpu::preview_source_key(
                    preview_gen,
                    &recipe.retouch,
                    width as usize,
                    height as usize,
                    channels as usize,
                );
                let (mut rgb, _) = gpu::with_preview_source_key(preview_key, || {
                    process_buffer_16_to_8_ex(&buffer, width, height, channels, &recipe, transfer, false, false)
                });
                let proof_error = apply_proof(&mut rgb, cmd_obj.proof.as_ref());
                if let Some(published) = preview_ring.as_mut().and_then(|ring| {
                    ring.write_rgb8_as_rgba8(SlotKind::Reference, &rgb, width, height, ColorSpaceTag::Srgb)
                }) {
                    print_json(
                        &ResponseWrapper::ok_action(
                            "reference",
                            RenderResult::from_published(published, Default::default(), proof_error),
                        )
                        .with_context(req_id, photo_path),
                    );
                    continue;
                }
                let slot = REFERENCE_SLOT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 2;
                let dest = security::get_secure_preview_dir().join(format!("omastudio_reference_{}.ppm", slot));
                let mut ppm = format!("P6\n{} {}\n255\n", width, height).into_bytes();
                ppm.extend_from_slice(&rgb);
                match security::atomic_write_secure(&dest, &ppm) {
                    Ok(()) => print_json(
                        &ResponseWrapper::ok_action(
                            "reference",
                            RenderResult::from_file(
                                dest.to_string_lossy().to_string(),
                                Default::default(),
                                proof_error,
                            ),
                        )
                        .with_context(req_id, photo_path),
                    ),
                    Err(e) => print_json::<()>(
                        &ResponseWrapper::err_action("reference", format!("Failed to write compare view: {}", e))
                            .with_context(req_id, photo_path),
                    ),
                }
            }
            "load" => {
                // Compile the GPU shaders while the first photo is decoded, so the
                // first export doesn't wait ~2 s for them. Joined on daemon exit so
                // Vulkan/Mesa ICD teardown never races with a live warm-up thread.
                if warm_up_worker.is_none() {
                    warm_up_worker = Some(std::thread::spawn(|| {
                        gpu::point_ops::warm_up();
                        gpu::local::warm_up();
                    }));
                }
                let p = match cmd_obj.path {
                    Some(ref path) => path,
                    None => {
                        print_json::<()>(
                            &ResponseWrapper::err_action("load", "Missing path in load command")
                                .with_context(req_id, None),
                        );
                        continue;
                    }
                };

                match RawImage::open(p) {
                    Ok(raw) => {
                        let meta = raw.get_metadata().unwrap_or(raw::RawMetadata {
                            width: 0,
                            height: 0,
                            raw_width: 0,
                            raw_height: 0,
                            make: String::new(),
                            model: String::new(),
                            lens: String::new(),
                            iso: 0.0,
                            shutter: 0.0,
                            aperture: 0.0,
                            focal_length: 0.0,
                            timestamp: 0,
                            cam_mul: [1.0, 1.0, 1.0, 1.0],
                        });

                        match raw.process_preview_16(true) {
                            Ok(prev) => {
                                let (u16_full, pw, ph) = cap_preview(prev.as_slice_u16(), prev.width, prev.height, prev.channels);
                                let u16_vec = Arc::new(u16_full);
                                let u16_slice = &u16_vec[..];
                                let transfer = prev.transfer;
                                let lut: Vec<u8> = (0u32..=65535)
                                    .map(|x| (transfer.encode(x as f32 / 65535.0).clamp(0.0, 1.0) * 255.0).round() as u8)
                                    .collect();
                                let u8_vec: Vec<u8> = u16_slice.par_iter().map(|&x| lut[x as usize]).collect();
                                let u8_arc = Arc::new(u8_vec);

                                let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
                                c.current_path = Some(p.clone());
                                c.baseline_exposure = raw.baseline_exposure_from(&prev);
                                c.width = pw;
                                c.height = ph;
                                c.channels = prev.channels;
                                c.raw_buffer_16 = Some(u16_vec);
                                c.raw_buffer_8 = Some(Arc::clone(&u8_arc));
                                c.transfer = transfer;
                                c.metadata = Some(meta.clone());
                                c.full = None;
                                c.full_retouched = None;
                                c.preview_gen = c.preview_gen.wrapping_add(1).max(1);
                                drop(c);
                                spawn_full_decode(Arc::clone(&cache), p.clone());

                                let thumb_path = thumbnail_path_for(Path::new(p));
                                let _ = raw.extract_thumbnail(&thumb_path);

                                let sidecar = Recipe::load_initial(p);
                                let scene = ai_classify_scene(&u8_arc, pw, ph, prev.channels, &meta);

                                print_json(
                                    &ResponseWrapper::ok_action(
                                        "load",
                                        InspectResult {
                                            path: p.clone(),
                                            metadata: meta,
                                            thumbnail: thumb_path.to_string_lossy().to_string(),
                                            recipe: sidecar,
                                            scene,
                                        },
                                    )
                                    .with_context(req_id, Some(p.clone())),
                                );
                            }
                            Err(e) => print_json::<()>(
                                &ResponseWrapper::err_action("load", e).with_context(req_id, Some(p.clone())),
                            ),
                        }
                    }
                    Err(e) => print_json::<()>(
                        &ResponseWrapper::err_action("load", e).with_context(req_id, Some(p.clone())),
                    ),
                }
            }
            "adjust" => {
                let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
                let photo_path = c.current_path.clone();
                if c.raw_buffer_16.is_none() {
                    print_json::<()>(
                        &ResponseWrapper::err_action("adjust", "No RAW image currently loaded in daemon cache")
                            .with_context(req_id, photo_path),
                    );
                    continue;
                }

                let width = c.width;
                let height = c.height;
                let channels = c.channels;
                let transfer = c.transfer;
                let preview_gen = c.preview_gen;
                let recipe = Recipe { baseline_exposure: c.baseline_exposure, ..cmd_obj.recipe.unwrap_or_default() };
                let split = cmd_obj.split.unwrap_or(0.0);

                let next_slot = (c.ping_pong + 1) % 2;
                c.ping_pong = next_slot;

                let buffer_arc = match c.raw_buffer_16.as_ref() {
                    Some(b) => Arc::clone(b),
                    None => {
                        print_json::<()>(
                            &ResponseWrapper::err_action("adjust", "Buffer unavailable")
                                .with_context(req_id, photo_path),
                        );
                        continue;
                    }
                };

                // Drop cache lock immediately so other daemon queries or inspection are never blocked
                drop(c);

                let buffer = buffer_arc.as_ref();

                let highlight_mask = cmd_obj.highlight_mask.unwrap_or(false);
                let shadow_mask = cmd_obj.shadow_mask.unwrap_or(false);
                let preview_key = gpu::preview_source_key(
                    preview_gen,
                    &recipe.retouch,
                    width as usize,
                    height as usize,
                    channels as usize,
                );

                // Stage 4B (WP-15): Zero-copy Vulkan DMA-BUF + explicit SYNC_FD preview when
                // enabled and no CPU-only overlay (split/clipping/soft-proof/peaking) is active.
                let allow_dmabuf = dmabuf_active
                    && cmd_obj.out.is_none()
                    && cmd_obj.prefer_memfd != Some(true)
                    && split <= 0.001
                    && !highlight_mask
                    && !shadow_mask
                    && cmd_obj.proof.is_none()
                    && cmd_obj.focus_peaking != Some(true);
                if allow_dmabuf
                    && let Some(sock) = preview_sock.as_ref()
                    && let Some(exported) = gpu::with_preview_source_key(preview_key, || {
                        try_process_preview_dmabuf(
                            buffer,
                            width,
                            height,
                            channels,
                            &recipe,
                            transfer,
                            ColorSpaceTag::Srgb,
                        )
                    })
                {
                    match send_dmabuf_frame_and_wait_ack(
                        sock.as_raw_fd(),
                        &exported.desc,
                        exported.dmabuf_fd.as_raw_fd(),
                        exported.sync_fd.as_ref().map(AsRawFd::as_raw_fd),
                        2_000,
                    ) {
                        Ok(()) => {
                            print_json(
                                &ResponseWrapper::ok_action(
                                    "adjust",
                                    RenderResult::from_dmabuf(&exported.desc, exported.histogram, None),
                                )
                                .with_context(req_id, photo_path),
                            );
                            continue;
                        }
                        Err(e) => {
                            eprintln!("omastudio: DMA-BUF send failed, switching session to memfd: {e}");
                            dmabuf_active = false;
                        }
                    }
                }

                let (mut final_buf, hist) = gpu::with_preview_source_key(preview_key, || {
                    if split > 0.001 {
                        process_split_comparison_16_to_8_ex(
                            buffer,
                            width,
                            height,
                            channels,
                            &recipe,
                            transfer,
                            split,
                            highlight_mask,
                            shadow_mask,
                        )
                    } else {
                        process_buffer_16_to_8_ex(
                            buffer,
                            width,
                            height,
                            channels,
                            &recipe,
                            transfer,
                            highlight_mask,
                            shadow_mask,
                        )
                    }
                });

                let proof_error = apply_proof(&mut final_buf, cmd_obj.proof.as_ref());
                if cmd_obj.focus_peaking == Some(true) {
                    pipeline::peaking::apply(&mut final_buf, width as usize, height as usize);
                }

                if cmd_obj.out.is_none()
                    && let Some(published) = preview_ring.as_mut().and_then(|ring| {
                        ring.write_rgb8_as_rgba8(SlotKind::Preview, &final_buf, width, height, ColorSpaceTag::Srgb)
                    })
                {
                    print_json(
                        &ResponseWrapper::ok_action(
                            "adjust",
                            RenderResult::from_published(published, hist, proof_error),
                        )
                        .with_context(req_id, photo_path),
                    );
                    continue;
                }

                let base_dir = security::get_secure_preview_dir();
                let out_dest = cmd_obj.out.unwrap_or_else(|| {
                    base_dir.join(format!("omastudio_preview_{}.ppm", next_slot)).to_string_lossy().to_string()
                });
                let dest_path = Path::new(&out_dest);
                let write_result: Result<(), String> = if out_dest.ends_with(".ppm") {
                    let mut ppm_bytes = format!("P6\n{} {}\n255\n", width, height).into_bytes();
                    ppm_bytes.extend_from_slice(&final_buf);
                    security::atomic_write_secure(dest_path, &ppm_bytes).map_err(|e| e.to_string())
                } else {
                    match image::ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(width, height, final_buf) {
                        Some(img) => {
                            let mut encoded = Vec::new();
                            let fmt = if out_dest.ends_with(".png") {
                                image::ImageFormat::Png
                            } else {
                                image::ImageFormat::Jpeg
                            };
                            img.write_to(&mut std::io::Cursor::new(&mut encoded), fmt)
                                .map_err(|e| e.to_string())
                                .and_then(|_| security::atomic_write_secure(dest_path, &encoded).map_err(|e| e.to_string()))
                        }
                        None => Err("Rendered buffer size does not match image dimensions".to_string()),
                    }
                };

                if let Err(e) = write_result {
                    print_json::<()>(
                        &ResponseWrapper::err_action("adjust", format!("Failed to write preview: {}", e))
                            .with_context(req_id, photo_path),
                    );
                    continue;
                }

                print_json(
                    &ResponseWrapper::ok_action(
                        "adjust",
                        RenderResult::from_file(out_dest, hist, proof_error),
                    )
                    .with_context(req_id, photo_path),
                );
            }
            "detail" => {
                let (full, slot, photo_path) = {
                    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
                    c.detail_ping_pong = (c.detail_ping_pong + 1) % 2;
                    (c.full.clone(), c.detail_ping_pong, c.current_path.clone())
                };
                let Some(full) = full else {
                    print_json::<()>(
                        &ResponseWrapper::err_action("detail", "Full resolution not ready")
                            .with_context(req_id, photo_path),
                    );
                    continue;
                };
                let [rx, ry, rw, rh] = cmd_obj.region.unwrap_or([0.0, 0.0, 1.0, 1.0]);
                let (ow, oh) = (cmd_obj.out_width.unwrap_or(0) as usize, cmd_obj.out_height.unwrap_or(0) as usize);
                let valid_region = rx.is_finite()
                    && ry.is_finite()
                    && rw.is_finite()
                    && rh.is_finite()
                    && rw >= 1e-6
                    && rh >= 1e-6
                    && rw <= 1.0 + 1e-6
                    && rh <= 1.0 + 1e-6;
                let valid_dims = ow > 0
                    && oh > 0
                    && ow.checked_mul(oh).is_some_and(|px| px <= (64 << 20))
                    && full.width > 0
                    && full.height > 0;
                if !valid_region || !valid_dims {
                    print_json::<()>(
                        &ResponseWrapper::err_action("detail", "Invalid detail region")
                            .with_context(req_id, photo_path),
                    );
                    continue;
                }
                let scale = (ow as f64 / (rw * full.width as f64)).clamp(1e-6, 8.0);
                let vw = ((full.width as f64 * scale).round() as usize).max(1);
                let vh = ((full.height as f64 * scale).round() as usize).max(1);
                let ox = ((rx * vw as f64).round().max(0.0) as usize).min(vw - 1);
                let oy = ((ry * vh as f64).round().max(0.0) as usize).min(vh - 1);
                let ow = ow.min(vw - ox).max(1);
                let oh = oh.min(vh - oy).max(1);
                let recipe = Recipe { baseline_exposure: full.baseline_exposure, ..cmd_obj.recipe.unwrap_or_default() };
                let out = (ox, oy, ow, oh);
                let src = pipeline::region_source(out, vw, vh, &recipe);
                let full = retouched_full(&cache, full, &recipe.retouch);
                let input = resample_rect(&full, scale, src);
                let window = pipeline::Window { x0: src.0, y0: src.1, frame_w: vw, frame_h: vh };
                let mut rgb = pipeline::render_region_8(&input, src.2, src.3, window, out, &recipe, full.transfer);
                pipeline::clipping_overlay(
                    &mut rgb,
                    3,
                    cmd_obj.highlight_mask.unwrap_or(false),
                    cmd_obj.shadow_mask.unwrap_or(false),
                );
                let proof_error = apply_proof(&mut rgb, cmd_obj.proof.as_ref());
                if cmd_obj.focus_peaking == Some(true) {
                    pipeline::peaking::apply(&mut rgb, ow, oh);
                }

                let tag = (cmd_obj.tag.as_deref() == Some("loupe")).then_some("loupe");
                let region = [ox as f64 / vw as f64, oy as f64 / vh as f64, ow as f64 / vw as f64, oh as f64 / vh as f64];
                let slot_kind = if tag == Some("loupe") { SlotKind::Loupe } else { SlotKind::Detail };
                if let Some(published) = preview_ring.as_mut().and_then(|ring| {
                    ring.write_rgb8_as_rgba8(slot_kind, &rgb, ow as u32, oh as u32, ColorSpaceTag::Srgb)
                }) {
                    print_json(
                        &ResponseWrapper::ok_action(
                            "detail",
                            DetailResult {
                                image: published.url,
                                tag,
                                proof_error,
                                region,
                                width: ow,
                                height: oh,
                                slot: Some(published.slot),
                                generation: Some(published.generation),
                                stride: Some(published.stride),
                                format: Some(published.format.as_str()),
                                color_space: Some(published.color_space.as_str()),
                            },
                        )
                        .with_context(req_id, photo_path),
                    );
                    continue;
                }
                let dest = security::get_secure_preview_dir().join(format!("omastudio_{}_{}.ppm", tag.unwrap_or("detail"), slot));
                let mut ppm = format!("P6\n{} {}\n255\n", ow, oh).into_bytes();
                ppm.extend_from_slice(&rgb);
                if let Err(e) = security::atomic_write_secure(&dest, &ppm) {
                    print_json::<()>(
                        &ResponseWrapper::err_action("detail", format!("Failed to write detail view: {}", e))
                            .with_context(req_id, photo_path),
                    );
                    continue;
                }
                print_json(
                    &ResponseWrapper::ok_action(
                        "detail",
                        DetailResult {
                            image: dest.to_string_lossy().to_string(),
                            tag,
                            proof_error,
                            region,
                            width: ow,
                            height: oh,
                            slot: None,
                            generation: None,
                            stride: None,
                            format: None,
                            color_space: None,
                        },
                    )
                    .with_context(req_id, photo_path),
                );
            }
            "ai_auto" => {
                let c = cache.lock().unwrap_or_else(|e| e.into_inner());
                let photo_path = c.current_path.clone();
                if let (Some(buffer), Some(meta)) = (&c.raw_buffer_8, &c.metadata) {
                    let auto_recipe = ai_auto_enhance(buffer, c.width, c.height, c.channels, meta);
                    let scene = ai_classify_scene(buffer, c.width, c.height, c.channels, meta);
                    let path_str = photo_path.clone().unwrap_or_default();
                    print_json(
                        &ResponseWrapper::ok_action(
                            "ai_auto",
                            InspectResult {
                                path: path_str,
                                metadata: meta.clone(),
                                thumbnail: String::new(),
                                recipe: auto_recipe,
                                scene,
                            },
                        )
                        .with_context(req_id, photo_path),
                    );
                } else {
                    print_json::<()>(
                        &ResponseWrapper::err_action("ai_auto", "No image loaded in cache for AI Auto")
                            .with_context(req_id, photo_path),
                    );
                }
            }
            "ai_jev" => {
                let c = cache.lock().unwrap_or_else(|e| e.into_inner());
                let photo_path = c.current_path.clone();
                if let (Some(buffer), Some(meta)) = (&c.raw_buffer_8, &c.metadata) {
                    let mut auto_recipe = ai_auto_enhance(buffer, c.width, c.height, c.channels, meta);
                    let (mut scene, mut jev_opt) = ai_classify_scene_with_jev(buffer, c.width, c.height, c.channels, meta);
                    if jev_opt.is_none() {
                        let state = ai::jev::extract_photographic_state(buffer, c.width, c.height, c.channels, meta);
                        let offline_jev = ai::jev::generate_offline_jev_decisions(&state, meta);
                        apply_jev_decisions_to_recipe(&mut auto_recipe, &offline_jev);
                        scene.recommended_preset = offline_jev.recommended_preset.clone();
                        scene.description = format!(
                            "Jev Offline Expert [Score: {:.1}/10, Contrast: {}]",
                            offline_jev.aesthetic_score, offline_jev.recommended_contrast
                        );
                        jev_opt = Some(offline_jev);
                    } else if let Some(ref jev) = jev_opt {
                        apply_jev_decisions_to_recipe(&mut auto_recipe, jev);
                    }
                    let path_str = photo_path.clone().unwrap_or_default();
                    print_json(
                        &ResponseWrapper::ok_action(
                            "ai_jev",
                            JevInspectResult {
                                path: path_str,
                                metadata: meta.clone(),
                                thumbnail: String::new(),
                                recipe: auto_recipe,
                                scene,
                                jev_decisions: jev_opt,
                            },
                        )
                        .with_context(req_id, photo_path),
                    );
                } else {
                    print_json::<()>(
                        &ResponseWrapper::err_action("ai_jev", "No image loaded in cache for AI Jev")
                            .with_context(req_id, photo_path),
                    );
                }
            }
            "ai_social" => {
                let platform = cmd_obj.platform.unwrap_or_else(|| "ig".to_string());
                let c = cache.lock().unwrap_or_else(|e| e.into_inner());
                let photo_path = c.current_path.clone();
                if let Some(ref buffer) = c.raw_buffer_8 {
                    let base = cmd_obj.recipe.clone().unwrap_or_else(|| {
                        c.current_path.as_ref().map(Recipe::load_initial).unwrap_or_default()
                    });
                    let result = ai_optimize_for_social(
                        buffer,
                        c.width,
                        c.height,
                        c.channels,
                        &platform,
                        &base,
                    );
                    print_json(&ResponseWrapper::ok_action("ai_social", result).with_context(req_id, photo_path));
                } else {
                    print_json::<()>(
                        &ResponseWrapper::err_action("ai_social", "No image loaded in cache for AI Social")
                            .with_context(req_id, photo_path),
                    );
                }
            }
            "save_recipe" => {
                let target = cmd_obj
                    .path
                    .clone()
                    .or_else(|| cache.lock().unwrap_or_else(|e| e.into_inner()).current_path.clone());
                if let (Some(p), Some(recipe)) = (&target, &cmd_obj.recipe) {
                    match recipe.save_sidecar(p) {
                        Ok(sidecar_path) => {
                            // Keep the SQLite catalogue's rating and flag in step with the edit
                            if let Err(e) = Catalog::update_culling_indexed(p, Some(recipe.rating), Some(&recipe.flag), None) {
                                eprintln!("omastudio: cannot write the catalogue: {e}");
                            }
                            print_json(
                                &ResponseWrapper::ok_action("save_recipe", sidecar_path.to_string_lossy().to_string())
                                    .with_context(req_id, Some(p.clone())),
                            )
                        }
                        Err(e) => print_json::<()>(
                            &ResponseWrapper::err_action("save_recipe", format!("Failed to save sidecar: {}", e))
                                .with_context(req_id, Some(p.clone())),
                        ),
                    }
                } else {
                    print_json::<()>(
                        &ResponseWrapper::err_action("save_recipe", "No image loaded or recipe missing")
                            .with_context(req_id, target),
                    );
                }
            }
            "export" => {
                let p = {
                    let c = cache.lock().unwrap_or_else(|e| e.into_inner());
                    match cmd_obj.path.clone().or_else(|| c.current_path.clone()) {
                        Some(p) => p,
                        None => {
                            print_json::<()>(
                                &ResponseWrapper::err_action("export", "No image loaded")
                                    .with_context(req_id, None),
                            );
                            continue;
                        }
                    }
                };
                let recipe = Box::new(cmd_obj.recipe.unwrap_or_default());
                let options = Box::new(cmd_obj.options.unwrap_or_default());
                let job = ExportJob::Single { path: p.clone(), recipe, options, request_id: req_id };
                if let Err(err) = export_tx.try_send(job) {
                    let msg = match err {
                        std::sync::mpsc::TrySendError::Full(_) => "Export queue is full; wait for pending exports to finish",
                        std::sync::mpsc::TrySendError::Disconnected(_) => "Export worker is unavailable",
                    };
                    print_json::<()>(
                        &ResponseWrapper::err_action("export", msg).with_context(req_id, Some(p)),
                    );
                }
            }
            "batch_export" => {
                let items = match cmd_obj.items {
                    Some(its) if !its.is_empty() => its,
                    _ => {
                        print_json::<()>(
                            &ResponseWrapper::err_action("batch_export", "No items provided for batch export")
                                .with_context(req_id, None),
                        );
                        continue;
                    }
                };

                if items.len() > 100 {
                    print_json::<()>(
                        &ResponseWrapper::err_action(
                            "batch_export",
                            "Batch export exceeds maximum limit of 100 items",
                        )
                        .with_context(req_id, None),
                    );
                    continue;
                }
                let options = Box::new(cmd_obj.options.unwrap_or_default());
                let job = ExportJob::Batch { items, options, request_id: req_id };
                if let Err(err) = export_tx.try_send(job) {
                    let msg = match err {
                        std::sync::mpsc::TrySendError::Full(_) => "Export queue is full; wait for pending exports to finish",
                        std::sync::mpsc::TrySendError::Disconnected(_) => "Export worker is unavailable",
                    };
                    print_json::<()>(
                        &ResponseWrapper::err_action("batch_export", msg).with_context(req_id, None),
                    );
                }
            }
            "fuji_recipes" => {
                let list: Vec<_> = fuji::library::list()
                    .into_iter()
                    .map(|e| serde_json::json!({ "path": e.path, "recipe": FujiRecipeResult::new(e.profile) }))
                    .collect();
                print_json(&ResponseWrapper::ok_action("fuji_recipes", list).with_context(req_id, None));
            }
            "fuji_import" => match cmd_obj.path.as_deref().map(|p| fuji::library::import(Path::new(p))) {
                Some(Ok(profile)) => print_json(
                    &ResponseWrapper::ok_action("fuji_import", FujiRecipeResult::new(profile))
                        .with_context(req_id, cmd_obj.path),
                ),
                Some(Err(e)) => print_json::<()>(
                    &ResponseWrapper::err_action("fuji_import", e).with_context(req_id, cmd_obj.path),
                ),
                None => print_json::<()>(
                    &ResponseWrapper::err_action("fuji_import", "Missing path").with_context(req_id, None),
                ),
            },
            "catalog_query" => {
                let filter = CatalogFilter {
                    min_rating: cmd_obj.rating,
                    flag: cmd_obj.flag.clone(),
                    camera_make: None,
                    camera_model: None,
                    lens: None,
                    min_iso: None,
                    max_iso: None,
                    search_query: cmd_obj.search_query.clone(),
                };
                match Catalog::query_indexed(&filter) {
                    Ok(results) => print_json(&ResponseWrapper::ok_action("catalog_query", results).with_context(req_id, None)),
                    Err(_) => {
                        let cat = Catalog::load();
                        let results = cat.filter_items(&filter);
                        print_json(&ResponseWrapper::ok_action("catalog_query", results).with_context(req_id, None));
                    }
                }
            }
            "catalog_cull" => {
                let target_path = match cmd_obj.path {
                    Some(ref p) => p.clone(),
                    None => {
                        let c = cache.lock().unwrap_or_else(|e| e.into_inner());
                        match c.current_path.as_ref() {
                            Some(p) => p.clone(),
                            None => {
                                print_json::<()>(
                                    &ResponseWrapper::err_action("catalog_cull", "No path specified and no image loaded")
                                        .with_context(req_id, None),
                                );
                                continue;
                            }
                        }
                    }
                };

                match Catalog::update_culling_indexed(
                    &target_path,
                    cmd_obj.rating,
                    cmd_obj.flag.as_deref(),
                    cmd_obj.color_label.as_deref(),
                ) {
                    Ok(true) => print_json(
                        &ResponseWrapper::ok_action("catalog_cull", "Culling metadata saved successfully")
                            .with_context(req_id, Some(target_path)),
                    ),
                    Ok(false) => print_json::<()>(
                        &ResponseWrapper::err_action("catalog_cull", "Item not found in catalog")
                            .with_context(req_id, Some(target_path)),
                    ),
                    Err(e) => print_json::<()>(
                        &ResponseWrapper::err_action("catalog_cull", format!("Cannot write the catalogue: {e}"))
                            .with_context(req_id, Some(target_path)),
                    ),
                }
            }
            "exit" => {
                print_json(&ResponseWrapper::ok_action("exit", "Daemon exiting").with_context(req_id, None));
                break;
            }
            unknown => {
                print_json::<()>(
                    &ResponseWrapper::err_action("error", format!("Unknown daemon command: {}", unknown))
                        .with_context(req_id, None),
                );
            }
        }
    }

    drop(export_tx);
    let _ = export_worker.join();
    if let Some(worker) = warm_up_worker {
        let _ = worker.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(name: &str, request_id: u64, exposure: f32) -> DaemonCommand {
        serde_json::from_value(serde_json::json!({
            "cmd": name,
            "request_id": request_id,
            "recipe": { "exposure": exposure }
        }))
        .unwrap()
    }

    #[test]
    fn command_queue_coalesces_consecutive_interactive_adjusts_but_respects_barriers() {
        let q = CommandQueue::new();
        q.push(cmd("adjust", 1, 0.2));
        q.push(cmd("adjust", 2, 0.5));
        q.push(cmd("adjust", 3, 1.1));
        q.push(cmd("load", 4, 0.0));
        q.push(cmd("adjust", 5, -0.3));
        q.push(cmd("adjust", 6, -0.8));
        q.close();

        let first = q.pop().unwrap();
        assert_eq!(first.cmd, "adjust");
        assert_eq!(first.request_id, Some(3));
        assert!((first.recipe.unwrap().exposure - 1.1).abs() < 1e-6);

        let second = q.pop().unwrap();
        assert_eq!(second.cmd, "load");
        assert_eq!(second.request_id, Some(4));

        let third = q.pop().unwrap();
        assert_eq!(third.cmd, "adjust");
        assert_eq!(third.request_id, Some(6));
        assert!((third.recipe.unwrap().exposure + 0.8).abs() < 1e-6);

        assert!(q.pop().is_none());
    }
}
