//! What the guest's media players decode with: a source opened by its guest path, its streams,
//! and its decoded pictures and sound, at guest semantics.
//!
//! This crate holds no decoder. Whoever can decode on the host installs an opener with
//! [`install_media`], as the display's observer is installed, so `libSceAvPlayer` names no host
//! API and a second decoder never needs surgery here. Until one is installed every source is
//! refused as unreadable.

use std::sync::OnceLock;

/// What a stream carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    /// Pictures of this size, as the container gives it.
    Video {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
    /// Sound.
    Audio {
        /// Channel count.
        channels: u16,
        /// Sample frames per second.
        sample_rate: u32,
    },
}

/// One stream of a source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamDesc {
    /// What it carries.
    pub kind: StreamKind,
    /// Its duration in milliseconds.
    pub duration_ms: u64,
    /// ISO 639-2/T language code.
    pub language: [u8; 3],
}

/// One decoded picture: 8-bit 4:2:0, the luma plane then the interleaved chroma plane, each row
/// `width` bytes with no padding.
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    /// When it is shown, in milliseconds from the start of the presentation.
    pub time_ms: u64,
    /// Visible width in pixels.
    pub width: u32,
    /// Visible height in pixels.
    pub height: u32,
    /// `width * height` luma bytes, then `width * height / 2` chroma bytes.
    pub nv12: Vec<u8>,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picture")
            .field("time_ms", &self.time_ms)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// Decoded sound: 16-bit samples, channels interleaved.
#[derive(Clone, PartialEq, Eq)]
pub struct Sound {
    /// When its first sample plays, in milliseconds from the start of the presentation.
    pub time_ms: u64,
    /// Channels per sample frame.
    pub channels: u16,
    /// Sample frames per second.
    pub sample_rate: u32,
    /// `channels` values per sample frame.
    pub samples: Vec<i16>,
}

impl std::fmt::Debug for Sound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sound")
            .field("time_ms", &self.time_ms)
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .field("samples", &self.samples.len())
            .finish()
    }
}

/// Why a source could not be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// No file is at the guest path.
    NotFound,
    /// The file is there and could not be played; the text says why.
    Unreadable(String),
}

/// A source being decoded. Each call answers at once: a frame not yet decoded is `None`.
pub trait Media: Send {
    /// Every stream, in the source's order.
    fn streams(&self) -> &[StreamDesc];
    /// The next decoded picture, if one is ready. `Err` when the stream cannot go on.
    fn next_picture(&mut self) -> Result<Option<Picture>, String>;
    /// The next decoded sound, if some is ready.
    fn next_sound(&mut self) -> Result<Option<Sound>, String>;
    /// Whether every picture has been decoded and handed out.
    fn pictures_ended(&self) -> bool;
}

/// Opens the source at a guest path.
pub type OpenMedia = fn(&str) -> Result<Box<dyn Media>, OpenError>;

static OPEN: OnceLock<OpenMedia> = OnceLock::new();

/// Installs what opens every player's source. First install wins.
pub fn install_media(open: OpenMedia) {
    let _ = OPEN.set(open);
}

/// Opens `path` with the installed opener.
pub(crate) fn open(path: &str) -> Result<Box<dyn Media>, OpenError> {
    match OPEN.get() {
        Some(open) => open(path),
        None => Err(OpenError::Unreadable("no decoder is installed".to_owned())),
    }
}
