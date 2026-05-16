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

/// Rotation applied when blitting a logical pixmap into the native
/// dumb buffer. The Steam Deck's embedded panel is physically mounted
/// in portrait (native 800x1280) but always used in landscape, so the
/// renderer paints a logical landscape pixmap and this rotation maps
/// it into the actual native buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    None,
    /// Rotate content 90° clockwise. Used on Steam Deck OLED (panel
    /// orientation "right side up").
    Cw90,
}

/// A modeset DRM surface backed by a dumb buffer.
#[derive(Debug)]
pub struct DrmSurface {
    card: Card,
    connector: connector::Handle,
    /// Native panel pixel width.
    width: u32,
    /// Native panel pixel height.
    height: u32,
    fb: framebuffer::Handle,
    db: drm::control::dumbbuffer::DumbBuffer,
    saved: SavedCrtc,
    rotation: Rotation,
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
        // Heuristic: an embedded panel reporting portrait dimensions
        // is almost certainly a tablet / Steam-Deck-style device used
        // in landscape. Apply a 90° CW rotation when blitting so the
        // UI reads correctly. Future improvement: read the DRM
        // connector's panel-orientation property instead.
        let rotation = if connector.interface() == connector::Interface::EmbeddedDisplayPort
            && height > width
        {
            Rotation::Cw90
        } else {
            Rotation::None
        };
        info!(
            connector = ?connector.interface(),
            crtc = ?crtc_handle,
            width,
            height,
            refresh = mode.vrefresh(),
            rotation = ?rotation,
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
            rotation,
        })
    }

    /// Logical width to render with (swaps with height when rotated 90°).
    pub const fn width(&self) -> u32 {
        match self.rotation {
            Rotation::None => self.width,
            Rotation::Cw90 => self.height,
        }
    }

    /// Logical height to render with.
    pub const fn height(&self) -> u32 {
        match self.rotation {
            Rotation::None => self.height,
            Rotation::Cw90 => self.width,
        }
    }

    #[allow(dead_code)]
    pub const fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// Map the dumb buffer for CPU writes. Pixels are XRGB8888 little-endian
    /// (memory layout B, G, R, X per pixel). The caller must commit by
    /// dropping the returned `Frame` (no-op; the CRTC already scans it).
    pub fn frame(&mut self) -> Result<Frame<'_>> {
        let stride = self.db.pitch();
        let native_width = self.width;
        let native_height = self.height;
        let rotation = self.rotation;
        let map = self
            .card
            .map_dumb_buffer(&mut self.db)
            .map_err(|e| Error::Drm(format!("map_dumb_buffer: {e}")))?;
        Ok(Frame {
            map,
            native_width,
            native_height,
            stride,
            rotation,
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
    /// Native panel pixel width (orientation as the kernel reports it).
    native_width: u32,
    /// Native panel pixel height.
    native_height: u32,
    /// Bytes per row in the dumb buffer (≥ `native_width` * 4).
    stride: u32,
    /// Rotation to apply when blitting a logical pixmap.
    rotation: Rotation,
}

impl std::fmt::Debug for Frame<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("native_width", &self.native_width)
            .field("native_height", &self.native_height)
            .field("stride", &self.stride)
            .field("rotation", &self.rotation)
            .finish_non_exhaustive()
    }
}

impl Frame<'_> {
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        self.map.as_mut()
    }

    /// Logical width — what the renderer should use to size its pixmap.
    pub const fn width(&self) -> u32 {
        match self.rotation {
            Rotation::None => self.native_width,
            Rotation::Cw90 => self.native_height,
        }
    }

    /// Logical height.
    pub const fn height(&self) -> u32 {
        match self.rotation {
            Rotation::None => self.native_height,
            Rotation::Cw90 => self.native_width,
        }
    }

    pub const fn native_width(&self) -> u32 {
        self.native_width
    }
    pub const fn native_height(&self) -> u32 {
        self.native_height
    }
    pub const fn stride(&self) -> u32 {
        self.stride
    }
    pub const fn rotation(&self) -> Rotation {
        self.rotation
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
