//! A submission prepared for its draw segments to bind their own textures and buffers.
//!
//! A stream the command processor carries out a segment at a time (D729) has each segment prepared
//! again as it is reached, and drawn from that preparation alone; what the whole stream's
//! preparation binds for its draws is never drawn. Binding it reads every texture and buffer each
//! draw names, and in a frame of tens of thousands of draws that was half the frame's preparation.
//! The whole stream is still prepared for what it carries forward and reports - registers, tables,
//! shaders, targets, dispatches - and the bindings are made only where something reads it whole.

use super::{GuestMemory, Pipeline, Queue, RegisteredShader, Submission};

impl Pipeline {
    /// [`Self::submit`], leaving its draws' textures and buffers unbound, for a stream whose draws
    /// are each drawn from their own segment's preparation
    /// ([`Submission::resources_per_segment`]).
    pub fn submit_per_segment(
        &mut self,
        stream: &[u8],
        queue: Queue,
        registered: &[RegisteredShader],
        memory: &impl GuestMemory,
    ) -> Submission {
        let carried = self.carried.clone();
        self.prepare_over(carried, stream, (queue, registered), memory, false)
    }

    /// A preparation's last step: its draws' textures and buffers bound where `bind` says to, and
    /// otherwise left to its draw segments and marked so.
    pub(super) fn bind_or_leave(
        &mut self,
        submission: &mut Submission,
        memory: &impl GuestMemory,
        bind: bool,
    ) {
        if bind {
            crate::perf::span(crate::perf::Span::PrepareTextures, || {
                self.bind_resources(submission, memory);
            });
        } else {
            submission.resources_per_segment = true;
        }
    }

    /// Binds the textures and buffers [`Self::submit_per_segment`] left unbound, read from `memory`
    /// as it is now, for what reads the submission whole: the run's report of its last submission
    /// and the backend that report drives it to. Nothing for a submission bound already.
    pub fn bind_left_resources(&mut self, submission: &mut Submission, memory: &impl GuestMemory) {
        if !submission.resources_per_segment {
            return;
        }
        self.bind_resources(submission, memory);
        submission.resources_per_segment = false;
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_translate::{Fidelity, Strategy, Width};

    use super::super::{
        GuestMemory, Pipeline, Queue, RenderCommand, ResourceId, ShaderStage, Submission,
        TableBase, TextureSource, USER_DATA_WORDS,
    };

    struct Nothing;
    impl GuestMemory for Nothing {
        fn read(&self, _address: u64, _length: usize) -> Option<&[u8]> {
            None
        }
    }

    /// An 8 x 4 linear image at 0x1_0000, every byte 0x7f.
    struct Image(Vec<u8>);
    impl GuestMemory for Image {
        fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
            let start = usize::try_from(address.checked_sub(0x1_0000)?).ok()?;
            self.0.get(start..start.checked_add(length)?)
        }
    }

    fn pipeline() -> Pipeline {
        Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Lane,
            width: Width::default(),
        })
        .expect("a pipeline")
        .feeding_user_data()
    }

    fn cube_stream() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/captures/agc-gl-cube-fw1240-b.hex");
        std::fs::read_to_string(&path)
            .expect("the committed capture")
            .lines()
            .flat_map(|line| {
                line.split('#')
                    .next()
                    .unwrap_or_default()
                    .split_whitespace()
            })
            .map(|word| u32::from_str_radix(word, 16).expect("a hex word"))
            .flat_map(u32::to_le_bytes)
            .collect()
    }

    /// A stream prepared for its segments is the stream prepared whole, its bindings left and
    /// marked so; bound later, it is the same commands.
    #[test]
    fn a_stream_prepared_for_its_segments_is_the_whole_without_its_bindings() {
        let stream = cube_stream();
        let whole = pipeline().submit(&stream, Queue::Draw, &[], &Nothing);
        let mut segmented = pipeline();
        let mut left = segmented.submit_per_segment(&stream, Queue::Draw, &[], &Nothing);
        assert!(left.resources_per_segment);
        assert!(!whole.resources_per_segment);
        assert_eq!(left.report.draws, whole.report.draws);
        segmented.bind_left_resources(&mut left, &Nothing);
        assert!(!left.resources_per_segment);
        assert_eq!(left.commands, whole.commands);
    }

    /// The bindings left are made when the submission is read whole, once.
    #[test]
    fn left_bindings_are_made_once_when_read_whole() {
        let shader = ResourceId(7);
        let mut pipeline = pipeline();
        pipeline.texture_sources.insert(
            shader,
            vec![TextureSource {
                slot: 0,
                table_offset: None,
                table: TableBase::default(),
                sampler_offset: None,
                user_data: Some(0),
                sampler_user_data: Some(8),
                cube: false,
                volume: false,
                compare: false,
                saturated: [true; 2],
                mirrored: [false; 2],
                shader_compare: false,
                level_user_data: None,
                buffer_offset: None,
                layered: false,
            }],
        );
        let (texels, width, height) = (0x1_0000_u64, 8u32, 4u32);
        let mut words = [0u32; USER_DATA_WORDS];
        words[..4].copy_from_slice(&[
            u32::try_from(texels >> 8).expect("a 40-bit base"),
            (1 << 20) | (((width - 1) & 3) << 30),
            ((width - 1) >> 2) | ((height - 1) << 14) | (1 << 31),
            (9 << 28) | (27 << 20) | 4 | 1 << 9,
        ]);
        words[8..11].copy_from_slice(&[4 | 4 << 3, 0, 1 << 20 | 1 << 22]);
        let mut submission = Submission {
            commands: vec![
                RenderCommand::SetUserData {
                    stage: ShaderStage::Fragment,
                    words,
                },
                RenderCommand::BindShader {
                    stage: ShaderStage::Fragment,
                    shader,
                },
                RenderCommand::Draw {
                    vertices: 3,
                    instances: 1,
                    first_vertex: 0,
                },
            ],
            resources_per_segment: true,
            ..Submission::default()
        };
        let memory = Image(vec![0x7f; 0x1_0000]);
        let textures = |submission: &Submission| {
            submission
                .commands
                .iter()
                .filter(|command| matches!(command, RenderCommand::BindTexture { .. }))
                .count()
        };
        assert_eq!(textures(&submission), 0);
        pipeline.bind_left_resources(&mut submission, &memory);
        assert_eq!(textures(&submission), 1, "{:?}", submission.commands);
        assert!(!submission.resources_per_segment);
        pipeline.bind_left_resources(&mut submission, &memory);
        assert_eq!(textures(&submission), 1, "bound once");
    }
}
