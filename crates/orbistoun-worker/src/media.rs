//! A media player's source opened on the host: the guest path through the title's mounts, the
//! file through `orbistoun-media-host`'s playback, its streams and frames handed to
//! `orbistoun-video` at guest semantics (D925).

use std::sync::Arc;

use orbistoun_media_host::playback::{Playback, StreamKind as HostKind};
use orbistoun_video::media::{Media, OpenError, Picture, Sound, StreamDesc, StreamKind};

/// A source being decoded on the host.
struct HostMedia {
    playback: Playback,
    streams: Vec<StreamDesc>,
}

impl Media for HostMedia {
    fn streams(&self) -> &[StreamDesc] {
        &self.streams
    }

    fn next_picture(&mut self) -> Result<Option<Picture>, String> {
        Ok(self.playback.next_video()?.map(|frame| Picture {
            time_ms: frame.time_ms,
            width: frame.picture.width,
            height: frame.picture.height,
            nv12: frame.picture.nv12,
        }))
    }

    fn next_sound(&mut self) -> Result<Option<Sound>, String> {
        Ok(self.playback.next_audio()?.map(|frame| Sound {
            time_ms: frame.time_ms,
            channels: frame.pcm.channels,
            sample_rate: frame.pcm.sample_rate,
            samples: frame.pcm.samples,
        }))
    }

    fn pictures_ended(&self) -> bool {
        self.playback.pictures_ended()
    }
}

/// Opens the file at the guest `path` and starts decoding it. A path under no mount, or with no
/// file there, is not found; a file that is there and does not play is unreadable.
pub(crate) fn open(path: &str) -> Result<Box<dyn Media>, OpenError> {
    let host = orbistoun_fs::mount::resolve_existing(path).ok_or(OpenError::NotFound)?;
    let bytes = std::fs::read(&host).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => OpenError::NotFound,
        _ => OpenError::Unreadable(error.to_string()),
    })?;
    let playback = Playback::open(&Arc::from(bytes))
        .map_err(|error| OpenError::Unreadable(error.to_string()))?;
    let streams = playback
        .streams()
        .iter()
        .filter_map(|stream| {
            let kind = match stream.kind {
                HostKind::Video { width, height, .. } => StreamKind::Video {
                    width: u32::from(width),
                    height: u32::from(height),
                },
                HostKind::Audio {
                    channels,
                    sample_rate,
                } => StreamKind::Audio {
                    channels,
                    sample_rate,
                },
                HostKind::Other(_) => return None,
            };
            Some(StreamDesc {
                kind,
                duration_ms: stream.duration_ms,
                language: stream.language,
            })
        })
        .collect();
    Ok(Box::new(HostMedia { playback, streams }))
}
