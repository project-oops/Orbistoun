//! What a compute wave finds in its registers at entry, and the account a guest dispatch keeps of
//! the memory it reaches.
//!
//! The hardware loads a compute wave's user data into `s0` upward, then the workgroup id
//! components `COMPUTE_PGM_RSRC2` enables (`TGID_X_EN`, `TGID_Y_EN`, `TGID_Z_EN`, in that order),
//! and each lane's thread id into `v0` upward, as many components as `TIDIG_COMP_CNT` asks for
//! (Mesa `src/gallium/drivers/radeonsi/gfx/si_shader_args.c` lines 515-592, the order radeonsi
//! declares these arguments in; `src/amd/registers/gfx10.json`, `COMPUTE_PGM_RSRC2`). One
//! translated invocation is one wave, and a group of several waves is a workgroup of that many
//! invocations (D858), so a lane's thread id is its wave's first thread plus its lane, x fastest.
//!
//! A module built for a guest dispatch writes guest memory for real, so an access it cannot place
//! exactly - outside its window, or through a descriptor whose addressing is not modelled - is
//! recorded rather than dropped, and the dispatch is refused when any lane made one.

use orbistoun_spirv::{Builder, Id, op};

use super::{EXEC_HI, EXEC_LO, PRIVATE, Wavefront};
use crate::TranslateError;
use crate::model::Model;

/// A guest compute dispatch's entry state: which workgroup id components follow the user data, and
/// the thread group's shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ComputeInputs {
    /// Which workgroup id components (x, y, z) are loaded, into consecutive scalar registers after
    /// the user data (`TGID_X_EN`, `TGID_Y_EN`, `TGID_Z_EN`).
    pub workgroup_ids: [bool; 3],
    /// How many thread id components land in `v0` upward: `TIDIG_COMP_CNT` plus one.
    pub thread_id_components: u32,
    /// The thread group's shape, `COMPUTE_NUM_THREAD_X`, `_Y` and `_Z`.
    pub threads: [u32; 3],
    /// The user-data words the stream never wrote, one bit each: a program that reads one is
    /// refused, since the register holds a value nothing here knows.
    #[serde(default)]
    pub unwritten_user_data: u32,
    /// A dispatch whose last group along a dimension runs fewer threads (`PARTIAL_TG_EN`).
    #[serde(default)]
    pub partial: Option<PartialGroups>,
    /// Whether the image a dispatch fetches from and the image it stores to have an unsigned
    /// integer format, which their declarations must say: a Vulkan view's numeric type and the
    /// shader's sampled type agree. Raw words move through them unconverted.
    #[serde(default)]
    pub integer_images: [bool; 2],
}

/// Where a dispatch's partial-group shape sits in the push-constant block (D855): the last group's
/// index along x, y and z, then the threads it runs along each - after the thirty-two words a
/// stage's user data may take.
pub const PARTIAL_PUSH_WORD: u32 = super::USER_DATA_STAGE_WORDS;

/// Where a dispatch's global loads' buffer's guest base sits in the push-constant block (D883): the
/// low half of the address the buffer starts at, after the partial-group shape.
pub const GLOBAL_BASE_PUSH_WORD: u32 = PARTIAL_PUSH_WORD + 6;

impl PartialGroups {
    /// The six words a module reads at [`PARTIAL_PUSH_WORD`].
    #[must_use]
    pub const fn push_words(self) -> [u32; 6] {
        [
            self.last[0],
            self.last[1],
            self.last[2],
            self.threads[0],
            self.threads[1],
            self.threads[2],
        ]
    }
}

/// The last group along each dimension of a dispatch with partial groups, and how many threads it
/// runs along that dimension (`COMPUTE_NUM_THREAD_X/Y/Z.NUM_THREAD_PARTIAL`): radv and radeonsi write
/// the remainder there, a whole group when the grid divides evenly (`radv_cmd_buffer.c:14777-14808`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PartialGroups {
    /// The last group's index along each dimension: the grid's size less one.
    pub last: [u32; 3],
    /// Threads a last group runs along each dimension.
    pub threads: [u32; 3],
}

impl ComputeInputs {
    /// How many threads one group runs.
    #[must_use]
    pub fn group_threads(self) -> u32 {
        self.threads
            .iter()
            .try_fold(1_u32, |total, &n| total.checked_mul(n))
            .unwrap_or(u32::MAX)
    }

    /// How many waves of `lanes` one group is: one invocation of the module each (D858).
    #[must_use]
    pub fn waves(self, lanes: u32) -> u32 {
        self.group_threads().div_ceil(lanes.max(1)).max(1)
    }

    /// Refuses a shape translated waves cannot run exactly.
    ///
    /// # Errors
    ///
    /// An empty group, a group of more than one wave whose `program` waits at a barrier or uses
    /// the local data share - its waves meet there, and separate invocations are not held for one
    /// another (D858) - or a thread id count outside one to three.
    pub fn check(self, lanes: u32, program: GroupProgram) -> Result<(), TranslateError> {
        let threads = self.group_threads();
        if threads == 0 {
            return Err(refusal("a thread group of no threads"));
        }
        if threads > lanes && (program.barrier || program.local_share) {
            return Err(TranslateError::WaveGroup {
                threads,
                waves: threads.div_ceil(lanes.max(1)),
                barrier: program.barrier,
                local_share: program.local_share,
            });
        }
        if !(1..=3).contains(&self.thread_id_components) {
            return Err(refusal("a thread id of other than one to three components"));
        }
        Ok(())
    }
}

/// What a compute program shares between the waves of its group: whether it waits at `s_barrier`,
/// and whether it reads or writes the local data share.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GroupProgram {
    /// Whether it executes `s_barrier`.
    pub barrier: bool,
    /// Whether it uses the local data share (`ds_*`).
    pub local_share: bool,
}

fn refusal(detail: &'static str) -> TranslateError {
    TranslateError::Unsupported { offset: 0, detail }
}

/// The variables a guest dispatch module declares: the workgroup id input, the wave's place in its
/// group when the group is several waves, and the escape flag.
#[derive(Debug, Clone, Copy)]
pub(super) struct DispatchState {
    /// The `uvec3` type and the `WorkgroupId` input variable.
    workgroup: (Id, Id),
    /// The `LocalInvocationIndex` input, which is the wave's index in its group, when the group is
    /// more than one wave (D858).
    wave: Option<Id>,
    /// A private word, non-zero once any access has left the memory the module can reach exactly.
    escaped: Id,
}

impl DispatchState {
    /// Declares the workgroup id, the wave index when the group is `waves` of more than one, and
    /// the escape flag, through the module's private word pointer type and its zero.
    pub(super) fn declare(
        b: &mut Builder,
        (u32_type, waves): (Id, u32),
        (escaped_ptr, zero): (Id, Id),
    ) -> Self {
        let workgroup = super::declare_workgroup_id(b, u32_type);
        let wave = (waves > 1).then(|| super::declare_lane_input(b, u32_type, true));
        let escaped = b.id();
        b.declare(op::VARIABLE, &[escaped_ptr.0, escaped.0, PRIVATE, zero.0]);
        Self {
            workgroup,
            wave,
            escaped,
        }
    }

    /// Declares the state when the module is a guest dispatch's: a compute stage given its entry
    /// state, run in waves of `lanes`.
    pub(super) fn for_stage(
        b: &mut Builder,
        (ids, lanes): (&super::Reserved, u32),
        stage: super::Stage,
        user_data: &super::UserData,
    ) -> Option<Self> {
        let inputs = user_data
            .compute
            .filter(|_| stage == super::Stage::Compute)?;
        Some(Self::declare(
            b,
            (ids.u32_type, inputs.waves(lanes)),
            (ids.counter_ptr, ids.counter_zero),
        ))
    }

    /// The input variables the entry point must name.
    pub(super) fn interface(self) -> impl Iterator<Item = u32> {
        std::iter::once(self.workgroup.1.0).chain(self.wave.map(|wave| wave.0))
    }

    /// Loads the workgroup ids after the user data, the thread ids into `v0` upward, and narrows the
    /// execution mask to the group's threads. In a group of several waves each lane's thread id
    /// follows from the invocation's wave (D858).
    pub(super) fn seed(self, module: &mut Wavefront<'_>, inputs: ComputeInputs, user_sgprs: u32) {
        let u32_type = module.u32_type;
        let (uvec3, input) = self.workgroup;
        let id = module.builder.id();
        module.builder.function(op::LOAD, &[uvec3.0, id.0, input.0]);
        let mut register = user_sgprs;
        for (component, enabled) in inputs.workgroup_ids.into_iter().enumerate() {
            if !enabled {
                continue;
            }
            let value = module.builder.id();
            module.builder.function(
                op::COMPOSITE_EXTRACT,
                &[
                    u32_type.0,
                    value.0,
                    id.0,
                    u32::try_from(component).unwrap_or(0),
                ],
            );
            module.store_scalar(register, value);
            register += 1;
        }

        // The first thread of this invocation's wave, when the group is several (D858).
        let base = self.wave.map(|input| {
            let u32_type = module.u32_type;
            let wave = module.builder.id();
            module
                .builder
                .function(op::LOAD, &[u32_type.0, wave.0, input.0]);
            let width = module.lanes;
            let lanes = Model::constant(module, width);
            binary(module, op::IMUL, wave, lanes)
        });
        let threads = inputs.group_threads();
        let count = if base.is_some() {
            module.lanes
        } else {
            module.lanes.min(threads)
        };
        let mut lanes = Vec::new();
        for lane in 0..count {
            let (ids, place) = thread_ids(module, base, inputs.threads, lane);
            for (register, value) in ids
                .into_iter()
                .take(inputs.thread_id_components as usize)
                .enumerate()
            {
                let pointer = module.lane_pointer(u32::try_from(register).unwrap_or(0), lane);
                module.builder.function(op::STORE, &[pointer.0, value.0]);
            }
            lanes.push((ids, place));
        }

        let low = match threads {
            32.. => u32::MAX,
            n => (1_u32 << n) - 1,
        };
        let high = match threads {
            64.. => u32::MAX,
            n @ 33.. => (1_u32 << (n - 32)) - 1,
            _ => 0,
        };
        let (low, high) = if base.is_none() && inputs.partial.is_none() {
            (Model::constant(module, low), Model::constant(module, high))
        } else {
            Self::group_mask(module, (id, inputs), &lanes)
        };
        module.store_scalar(EXEC_LO, low);
        module.store_scalar(EXEC_HI, high);
    }

    /// The execution mask of a wave that may lie past its group's end or in a partial group: each
    /// of `lanes` runs unless its place in the group is past the group's threads (the last wave of
    /// a group the wave width does not divide, D858), or, along a dimension where it lies at or
    /// past the last group's thread count, this group is the last. The last group and its counts
    /// are read from the push-constant block at [`PARTIAL_PUSH_WORD`] (D855), so one module serves
    /// every grid a program is dispatched over; a shape compiled in made each new grid size another
    /// module and another pipeline to build.
    fn group_mask(
        module: &mut Wavefront<'_>,
        (id, inputs): (Id, ComputeInputs),
        lanes: &[([Id; 3], Option<Id>)],
    ) -> (Id, Id) {
        let (u32_type, bool_type) = (module.u32_type, module.bool_type);
        let shape = inputs.threads;
        // Per dimension a group is wider than one thread along: whether this group is the last,
        // and the last group's thread count.
        let mut last = [None; 3];
        for (dimension, slot) in last.iter_mut().enumerate() {
            if shape[dimension] <= 1 || inputs.partial.is_none() {
                continue;
            }
            let index = u32::try_from(dimension).unwrap_or(0);
            let (Some(wanted), Some(count)) = (
                module.push_word(PARTIAL_PUSH_WORD + index),
                module.push_word(PARTIAL_PUSH_WORD + 3 + index),
            ) else {
                continue;
            };
            let value = module.builder.id();
            module
                .builder
                .function(op::COMPOSITE_EXTRACT, &[u32_type.0, value.0, id.0, index]);
            let is_last = module.builder.id();
            module
                .builder
                .function(op::IEQUAL, &[bool_type.0, is_last.0, value.0, wanted.0]);
            *slot = Some((is_last, count));
        }
        // A group the wave width divides has no thread past its end.
        let threads = inputs.group_threads();
        let end = (threads % module.lanes != 0).then(|| Model::constant(module, threads));
        let zero = Model::constant(module, 0);
        let mut halves = [zero, zero];
        for (lane, &(ids, place)) in (0_u32..).zip(lanes) {
            // Whether this thread lies past its group's end, or past a last group's count along
            // some dimension.
            let mut off: Option<Id> = None;
            let mut either = |module: &mut Wavefront<'_>, here: Id| {
                off = Some(match off {
                    None => here,
                    Some(before) => {
                        let both = module.builder.id();
                        module
                            .builder
                            .function(op::LOGICAL_OR, &[bool_type.0, both.0, before.0, here.0]);
                        both
                    }
                });
            };
            if let (Some(end), Some(place)) = (end, place) {
                let past = module.builder.id();
                module
                    .builder
                    .function(op::ULESS_THAN_EQUAL, &[bool_type.0, past.0, end.0, place.0]);
                either(module, past);
            }
            for dimension in 0..3 {
                let Some((is_last, count)) = last[dimension] else {
                    continue;
                };
                let past = module.builder.id();
                module.builder.function(
                    op::ULESS_THAN_EQUAL,
                    &[bool_type.0, past.0, count.0, ids[dimension].0],
                );
                let here = module.builder.id();
                module
                    .builder
                    .function(op::LOGICAL_AND, &[bool_type.0, here.0, is_last.0, past.0]);
                either(module, here);
            }
            let bit = Model::constant(module, 1 << (lane % 32));
            let contribution = match off {
                None => bit,
                Some(off) => {
                    let chosen = module.builder.id();
                    module
                        .builder
                        .function(op::SELECT, &[u32_type.0, chosen.0, off.0, zero.0, bit.0]);
                    chosen
                }
            };
            let half = (lane / 32) as usize;
            let merged = module.builder.id();
            module.builder.function(
                op::BITWISE_OR,
                &[u32_type.0, merged.0, halves[half].0, contribution.0],
            );
            halves[half] = merged;
        }
        (halves[0], halves[1])
    }

    /// Records an escape: sets the flag when `escaped` holds, for `lane` when it is active, or
    /// unconditionally for the scalar unit.
    pub(super) fn note(self, module: &mut Wavefront<'_>, escaped: Id, lane: Option<u32>) {
        let condition = match lane {
            Some(lane) => match module.lane_known(lane) {
                Some(false) => return,
                Some(true) => escaped,
                None => {
                    let active = module.lane_active(lane);
                    let bool_type = module.bool_type;
                    let both = module.builder.id();
                    module
                        .builder
                        .function(op::LOGICAL_AND, &[bool_type.0, both.0, escaped.0, active.0]);
                    both
                }
            },
            None => escaped,
        };
        let u32_type = module.u32_type;
        let one = Model::constant(module, 1);
        let before = module.builder.id();
        module
            .builder
            .function(op::LOAD, &[u32_type.0, before.0, self.escaped.0]);
        let after = module.builder.id();
        module.builder.function(
            op::SELECT,
            &[u32_type.0, after.0, condition.0, one.0, before.0],
        );
        module
            .builder
            .function(op::STORE, &[self.escaped.0, after.0]);
    }

    /// Publishes the flag: word zero of the observation becomes one when any access escaped, and is
    /// left alone otherwise, so no invocation clears another's finding.
    pub(super) fn publish(self, module: &mut Wavefront<'_>) {
        let u32_type = module.u32_type;
        let bool_type = module.bool_type;
        let zero = Model::constant(module, 0);
        let one = Model::constant(module, 1);
        let flag = module.builder.id();
        module
            .builder
            .function(op::LOAD, &[u32_type.0, flag.0, self.escaped.0]);
        let set = module.builder.id();
        module
            .builder
            .function(op::INOT_EQUAL, &[bool_type.0, set.0, flag.0, zero.0]);
        let (store, merge) = (module.builder.id(), module.builder.id());
        module.builder.function(op::SELECTION_MERGE, &[merge.0, 0]);
        module
            .builder
            .function(op::BRANCH_CONDITIONAL, &[set.0, store.0, merge.0]);
        module.builder.function(op::LABEL, &[store.0]);
        module.write_observation(zero, 0, one);
        module.builder.function(op::BRANCH, &[merge.0]);
        module.builder.function(op::LABEL, &[merge.0]);
    }
}

/// `left operation right` on words.
fn binary(module: &mut Wavefront<'_>, operation: u16, left: Id, right: Id) -> Id {
    let u32_type = module.u32_type;
    let result = module.builder.id();
    module
        .builder
        .function(operation, &[u32_type.0, result.0, left.0, right.0]);
    result
}

/// A lane's thread id components in a group of `shape`, x fastest, and its place in the group:
/// constants when the group is one wave, and computed from `base`, the first thread of the
/// invocation's wave, when it is several (D858).
fn thread_ids(
    module: &mut Wavefront<'_>,
    base: Option<Id>,
    [x, y, _]: [u32; 3],
    lane: u32,
) -> ([Id; 3], Option<Id>) {
    let Some(base) = base else {
        let ids = [lane % x, (lane / x) % y, lane / (x * y)];
        return (ids.map(|value| Model::constant(module, value)), None);
    };
    let lane = Model::constant(module, lane);
    let place = binary(module, op::IADD, base, lane);
    // `n % d` as `n - (n / d) * d`.
    let divide = |module: &mut Wavefront<'_>, value: Id, by: u32| {
        let by = Model::constant(module, by);
        let quotient = binary(module, op::UDIV, value, by);
        let whole = binary(module, op::IMUL, quotient, by);
        (quotient, binary(module, op::ISUB, value, whole))
    };
    let (row, x) = divide(module, place, x);
    let (z, y) = divide(module, row, y);
    ([x, y, z], Some(place))
}

#[cfg(test)]
mod tests {
    use super::{ComputeInputs, GroupProgram};

    fn inputs(threads: [u32; 3], components: u32) -> ComputeInputs {
        ComputeInputs {
            workgroup_ids: [true, false, false],
            thread_id_components: components,
            threads,
            unwritten_user_data: 0,
            partial: None,
            integer_images: [false; 2],
        }
    }

    /// A group runs, in as many waves as it takes (D858); an empty one and a thread id of other than
    /// one to three components are refused.
    #[test]
    fn a_group_runs_in_as_many_waves_as_it_takes() {
        let plain = GroupProgram::default();
        assert!(inputs([64, 1, 1], 1).check(64, plain).is_ok());
        assert!(inputs([8, 8, 1], 2).check(64, plain).is_ok());
        assert!(inputs([64, 1, 1], 1).check(32, plain).is_ok());
        assert_eq!(inputs([64, 1, 1], 1).waves(32), 2);
        assert_eq!(inputs([100, 1, 1], 1).waves(64), 2);
        assert_eq!(inputs([8, 8, 1], 2).waves(64), 1);
        assert!(inputs([0, 1, 1], 1).check(64, plain).is_err());
        assert!(inputs([64, 1, 1], 0).check(64, plain).is_err());
        assert!(inputs([64, 1, 1], 4).check(64, plain).is_err());
    }

    /// A group of waves whose program waits at `s_barrier` or uses the local data share is refused,
    /// saying how many waves it is and which of the two it does: its waves meet there, and
    /// separate invocations are not held for one another (D858).
    #[test]
    fn a_group_of_waves_that_meet_is_refused() {
        let program = GroupProgram {
            barrier: true,
            local_share: true,
        };
        let refused = inputs([256, 1, 1], 1).check(64, program).unwrap_err();
        assert_eq!(
            refused.to_string(),
            concat!(
                "a thread group of 256 threads is 4 waves, and one translated invocation is one ",
                "wave; its program waits at s_barrier and uses the local data share"
            )
        );
        let shares = GroupProgram {
            barrier: false,
            local_share: true,
        };
        let refused = inputs([8, 8, 2], 3).check(32, shares).unwrap_err();
        assert_eq!(
            refused.to_string(),
            concat!(
                "a thread group of 128 threads is 4 waves, and one translated invocation is one ",
                "wave; its program waits at no barrier and uses the local data share"
            )
        );
        assert!(
            inputs([8, 8, 2], 3)
                .check(32, GroupProgram::default())
                .is_ok()
        );
    }
}
