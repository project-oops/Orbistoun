//! The last submission read whole, its draw segments' bindings made first.

use super::{MappedRegions, last_submission, live_pipeline};

/// Binds the textures and buffers the last submission left to its draw segments (D729), so what
/// reads it whole - the report, and the backend the worker drives it to - sees every draw's
/// bindings. Read from memory as it stands now, unsettled, so reading the report writes nothing
/// back; the slot keeps any submission made meanwhile.
pub(super) fn bind_last_submission() {
    let Some(mut submission) = last_submission()
        .lock()
        .ok()
        .and_then(|mut slot| slot.take_if(|held| held.resources_per_segment))
    else {
        return;
    };
    if let Some(pipeline) = live_pipeline()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_mut()
    {
        pipeline.bind_left_resources(&mut submission, &MappedRegions::current());
    }
    if let Ok(mut slot) = last_submission().lock() {
        slot.get_or_insert(submission);
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_translate::{Fidelity, Strategy, Width};

    use super::super::{Pipeline, Submission, last_submission, live_pipeline, tests::serial};

    /// A last submission whose bindings were left to its segments is bound before it is read
    /// whole, through the report or taken; one bound already is read as it is.
    #[test]
    fn the_last_submission_is_bound_before_it_is_read_whole() {
        let _guard = serial();
        let held = live_pipeline()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace(
                Pipeline::new(Strategy::Predicated {
                    fidelity: Fidelity::Lane,
                    width: Width::default(),
                })
                .expect("a pipeline"),
            );
        *last_submission().lock().expect("slot") = Some(Submission {
            resources_per_segment: true,
            ..Submission::default()
        });

        assert!(super::super::last_submission_report().is_some());
        let bound = super::super::take_last_submission().expect("the submission");
        assert!(!bound.resources_per_segment, "bound before it was read");
        assert!(super::super::take_last_submission().is_none(), "taken");

        *live_pipeline()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = held;
    }
}
