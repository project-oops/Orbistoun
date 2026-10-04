//! What a compute wave finds in its registers at entry, and the account a guest dispatch keeps of
//! the memory it reaches.
//!
//! The hardware loads a compute wave's user data into `s0` upward, then the workgroup id
//! components `COMPUTE_PGM_RSRC2` enables (`TGID_X_EN`, `TGID_Y_EN`, `TGID_Z_EN`, in that order),
//! and each lane's thread id into `v0` upward, as many components as `TIDIG_COMP_CNT` asks for
//! (Mesa `src/gallium/drivers/radeonsi/gfx/si_shader_args.c` lines 515-592, the order radeonsi
//! declares these arguments in; `src/amd/registers/gfx10.json`, `COMPUTE_PGM_RSRC2`). One
//! translated invocation is one wave, and a dispatch's thread group fits one wave, so a lane's
//! thread id is its position in the group, x fastest.
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

    /// Refuses a shape one translated wave cannot run exactly.
    ///
    /// # Errors
    ///
    /// An empty group, a group wider than the wave (its waves would share a local data share one
    /// invocation cannot), or a thread id count outside one to three.
    pub fn check(self, lanes: u32) -> Result<(), TranslateError> {
        let threads = self.group_threads();
        if threads == 0 {
            return Err(refusal("a thread group of no threads"));
        }
        if threads > lanes {
            return Err(refusal(concat!(
                "a thread group larger than one wave: its waves share a local data share, and one ",
                "translated invocation is one wave"
            )));
        }
        if !(1..=3).contains(&self.thread_id_components) {
            return Err(refusal("a thread id of other than one to three components"));
        }
        Ok(())
    }
}

fn refusal(detail: &'static str) -> TranslateError {
    TranslateError::Unsupported { offset: 0, detail }
}

/// The variables a guest dispatch module declares: the workgroup id input and the escape flag.
#[derive(Debug, Clone, Copy)]
pub(super) struct DispatchState {
    /// The `uvec3` type and the `WorkgroupId` input variable.
    workgroup: (Id, Id),
    /// A private word, non-zero once any access has left the memory the module can reach exactly.
    escaped: Id,
}

impl DispatchState {
    /// Declares the workgroup id and the escape flag, through the module's private word pointer
    /// type and its zero.
    pub(super) fn declare(b: &mut Builder, u32_type: Id, (escaped_ptr, zero): (Id, Id)) -> Self {
        let workgroup = super::declare_workgroup_id(b, u32_type);
        let escaped = b.id();
        b.declare(op::VARIABLE, &[escaped_ptr.0, escaped.0, PRIVATE, zero.0]);
        Self { workgroup, escaped }
    }

    /// Declares the state when the module is a guest dispatch's: a compute stage given its entry
    /// state.
    pub(super) fn for_stage(
        b: &mut Builder,
        ids: &super::Reserved,
        stage: super::Stage,
        user_data: super::UserData,
    ) -> Option<Self> {
        (stage == super::Stage::Compute && user_data.compute.is_some())
            .then(|| Self::declare(b, ids.u32_type, (ids.counter_ptr, ids.counter_zero)))
    }

    /// The input variable the entry point must name.
    pub(super) fn interface(self) -> u32 {
        self.workgroup.1.0
    }

    /// Loads the workgroup ids after the user data, the thread ids into `v0` upward, and narrows the
    /// execution mask to the group's threads.
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

        let [x, y, _] = inputs.threads;
        let threads = inputs.group_threads();
        for lane in 0..module.lanes.min(threads) {
            let ids = [lane % x, (lane / x) % y, lane / (x * y)];
            for (register, value) in ids
                .into_iter()
                .take(inputs.thread_id_components as usize)
                .enumerate()
            {
                let value = Model::constant(module, value);
                let pointer = module.lane_pointer(u32::try_from(register).unwrap_or(0), lane);
                module.builder.function(op::STORE, &[pointer.0, value.0]);
            }
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
        let (low, high) = match inputs.partial {
            Some(partial) => Self::partial_mask(module, (id, inputs.threads), partial, threads),
            None => (Model::constant(module, low), Model::constant(module, high)),
        };
        module.store_scalar(EXEC_LO, low);
        module.store_scalar(EXEC_HI, high);
    }

    /// The execution mask of a group that may be a partial one: each thread runs unless, along a
    /// dimension where it lies past the partial count, this group is the last.
    fn partial_mask(
        module: &mut Wavefront<'_>,
        (id, shape): (Id, [u32; 3]),
        partial: PartialGroups,
        threads: u32,
    ) -> (Id, Id) {
        let (u32_type, bool_type) = (module.u32_type, module.bool_type);
        // Whether this group is the last along each dimension.
        let mut last = [None; 3];
        for (dimension, slot) in last.iter_mut().enumerate() {
            if partial.threads[dimension] >= shape[dimension] {
                continue;
            }
            let index = u32::try_from(dimension).unwrap_or(0);
            let value = module.builder.id();
            module
                .builder
                .function(op::COMPOSITE_EXTRACT, &[u32_type.0, value.0, id.0, index]);
            let wanted = Model::constant(module, partial.last[dimension]);
            let is_last = module.builder.id();
            module
                .builder
                .function(op::IEQUAL, &[bool_type.0, is_last.0, value.0, wanted.0]);
            *slot = Some(is_last);
        }
        let [x, y, _] = shape;
        let zero = Model::constant(module, 0);
        let mut halves = [zero, zero];
        for lane in 0..module.lanes.min(threads) {
            let ids = [lane % x, (lane / x) % y, lane / (x * y)];
            // The dimensions along which this thread lies past a last group's count.
            let mut off: Option<Id> = None;
            for dimension in 0..3 {
                let Some(is_last) = last[dimension] else {
                    continue;
                };
                if ids[dimension] < partial.threads[dimension] {
                    continue;
                }
                off = Some(match off {
                    None => is_last,
                    Some(before) => {
                        let either = module.builder.id();
                        module.builder.function(
                            op::LOGICAL_OR,
                            &[bool_type.0, either.0, before.0, is_last.0],
                        );
                        either
                    }
                });
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

#[cfg(test)]
mod tests {
    use super::ComputeInputs;

    fn inputs(threads: [u32; 3], components: u32) -> ComputeInputs {
        ComputeInputs {
            workgroup_ids: [true, false, false],
            thread_id_components: components,
            threads,
            unwritten_user_data: 0,
            partial: None,
        }
    }

    /// A group that fits one wave runs; an empty one, one wider than the wave, and a thread id of
    /// no components are refused.
    #[test]
    fn a_group_must_fit_one_wave() {
        assert!(inputs([64, 1, 1], 1).check(64).is_ok());
        assert!(inputs([8, 8, 1], 2).check(64).is_ok());
        assert!(
            inputs([64, 1, 1], 1).check(32).is_err(),
            "wider than a wave32"
        );
        assert!(inputs([0, 1, 1], 1).check(64).is_err());
        assert!(inputs([64, 1, 1], 0).check(64).is_err());
        assert!(inputs([64, 1, 1], 4).check(64).is_err());
    }
}
