//! DRM/KMS dumb-buffer surface.
//!
//! Opens the card, finds the first connected connector with a usable
//! preferred mode, allocates a dumb buffer in `XRGB8888`, and modesets
//! it onto the CRTC. Provides `Frame::pixels_mut()` so the renderer can
//! draw into the buffer with tiny-skia. Saves and restores the previous
//! CRTC configuration on drop so the system console comes back when the
//! agent exits.
//!
//! TTY graphics-mode switching (KDSETMODE / `KD_GRAPHICS`) is documented
//! as a kernel cmdline requirement for v1: pass `quiet loglevel=3` to
//! suppress the kmsg overlay. Wiring the ioctl is a follow-up.

use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd};
use std::path::Path;

use drm::Device;
use drm::buffer::{Buffer, DrmFourcc};
use drm::control::{
    Device as ControlDevice, Mode, ResourceHandles, connector, crtc, encoder, framebuffer,
};
use tracing::{debug, info};

use crate::error::{Error, Result};

/// Wrapper that holds the file and implements drm-rs traits.
#[derive(Debug)]
pub struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl Device for Card {}
impl ControlDevice for Card {}

/// Saved CRTC configuration so we can restore on drop.
#[derive(Debug, Clone, Copy)]
struct SavedCrtc {
    handle: crtc::Handle,
    framebuffer: Option<framebuffer::Handle>,
    position: (u32, u32),
    mode: Option<Mode>,
}

/// A modeset DRM surface backed by a dumb buffer.
#[derive(Debug)]
pub struct DrmSurface {
    card: Card,
    connector: connector::Handle,
    width: u32,
    height: u32,
    fb: framebuffer::Handle,
    db: drm::control::dumbbuffer::DumbBuffer,
    saved: SavedCrtc,
}

impl DrmSurface {
    /// Open `card_path`, modeset the first connected output, and return
    /// a surface ready for drawing.
    pub fn open<P: AsRef<Path>>(card_path: P) -> Result<Self> {
        let path = card_path.as_ref();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| Error::Drm(format!("open {}: {e}", path.display())))?;
        let card = Card(file);

        let res: ResourceHandles = card
            .resource_handles()
            .map_err(|e| Error::Drm(format!("resource_handles: {e}")))?;

        let (connector, mode) = pick_connector(&card, &res)?;
        let (crtc_handle, _enc) = pick_crtc(&card, &res, &connector)?;

        let (w, h) = mode.size();
        let width = u32::from(w);
        let height = u32::from(h);
        info!(
            connector = ?connector.interface(),
            crtc = ?crtc_handle,
            width,
            height,
            refresh = mode.vrefresh(),
            "drm: modeset target",
        );

        let db = card
            .create_dumb_buffer((width, height), DrmFourcc::Xrgb8888, 32)
            .map_err(|e| Error::Drm(format!("create_dumb_buffer: {e}")))?;

        let fb = card
            .add_framebuffer(&db, 24, 32)
            .map_err(|e| Error::Drm(format!("add_framebuffer: {e}")))?;

        // Save the existing CRTC config before clobbering it.
        let prev = card
            .get_crtc(crtc_handle)
            .map_err(|e| Error::Drm(format!("get_crtc: {e}")))?;
        let saved = SavedCrtc {
            handle: crtc_handle,
            framebuffer: prev.framebuffer(),
            position: prev.position(),
            mode: prev.mode(),
        };

        card.set_crtc(crtc_handle, Some(fb), (0, 0), &[connector.handle()], Some(mode))
            .map_err(|e| Error::Drm(format!("set_crtc: {e}")))?;

        debug!("drm: surface ready");
        Ok(Self {
            card,
            connector: connector.handle(),
            width,
            height,
            fb,
            db,
            saved,
        })
    }

    pub const fn width(&self) -> u32 {
        self.width
    }
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Map the dumb buffer for CPU writes. Pixels are XRGB8888 little-endian
    /// (memory layout B, G, R, X per pixel). The caller must commit by
    /// dropping the returned `Frame` (no-op; the CRTC already scans it).
    pub fn frame(&mut self) -> Result<Frame<'_>> {
        let stride = self.db.pitch();
        let width = self.width;
        let height = self.height;
        let map = self
            .card
            .map_dumb_buffer(&mut self.db)
            .map_err(|e| Error::Drm(format!("map_dumb_buffer: {e}")))?;
        Ok(Frame {
            map,
            width,
            height,
            stride,
        })
    }
}

impl Drop for DrmSurface {
    fn drop(&mut self) {
        // Best-effort: log failures but don't panic in Drop.
        if let Err(e) = self.card.set_crtc(
            self.saved.handle,
            self.saved.framebuffer,
            self.saved.position,
            &[self.connector],
            self.saved.mode,
        ) {
            tracing::warn!("drm: restore_crtc failed: {e}");
        }
        if let Err(e) = self.card.destroy_framebuffer(self.fb) {
            tracing::warn!("drm: destroy_framebuffer failed: {e}");
        }
        if let Err(e) = self.card.destroy_dumb_buffer(self.db) {
            tracing::warn!("drm: destroy_dumb_buffer failed: {e}");
        }
    }
}

/// A live mapping into the dumb buffer. Holds the mmap until dropped.
pub struct Frame<'s> {
    map: drm::control::dumbbuffer::DumbMapping<'s>,
    width: u32,
    height: u32,
    stride: u32,
}

impl std::fmt::Debug for Frame<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("stride", &self.stride)
            .finish_non_exhaustive()
    }
}

impl Frame<'_> {
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        self.map.as_mut()
    }

    pub const fn width(&self) -> u32 {
        self.width
    }
    pub const fn height(&self) -> u32 {
        self.height
    }
    pub const fn stride(&self) -> u32 {
        self.stride
    }
}

fn pick_connector(card: &Card, res: &ResourceHandles) -> Result<(connector::Info, Mode)> {
    let chosen = res
        .connectors()
        .iter()
        .filter_map(|c| card.get_connector(*c, true).ok())
        .filter(|c| c.state() == connector::State::Connected)
        .find(|c| !c.modes().is_empty())
        .ok_or(Error::NoDrmConnector)?;

    let mode = chosen
        .modes()
        .iter()
        .copied()
        .find(|m| m.mode_type().contains(drm::control::ModeTypeFlags::PREFERRED))
        .or_else(|| chosen.modes().first().copied())
        .ok_or(Error::NoDrmConnector)?;

    Ok((chosen, mode))
}

fn pick_crtc(
    card: &Card,
    res: &ResourceHandles,
    connector: &connector::Info,
) -> Result<(crtc::Handle, encoder::Info)> {
    // Prefer the connector's current encoder + CRTC if present.
    if let Some(enc_handle) = connector.current_encoder() {
        if let Ok(enc) = card.get_encoder(enc_handle) {
            if let Some(crtc_h) = enc.crtc() {
                return Ok((crtc_h, enc));
            }
        }
    }
    // Otherwise pick any compatible encoder + CRTC.
    for enc_handle in connector.encoders() {
        if let Ok(enc) = card.get_encoder(*enc_handle) {
            if let Some(crtc_h) = res.filter_crtcs(enc.possible_crtcs()).first().copied() {
                return Ok((crtc_h, enc));
            }
        }
    }
    Err(Error::NoDrmConnector)
}
