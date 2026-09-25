# Work log

Append-only running record of what was done, in order. Its job is to let a session
that has lost its conversation history pick the work back up without guessing.

**Read this file, and [DECISIONS.md](DECISIONS.md), at the start of any working
session.** Append to it at the end of every completed unit of work - not at the end
of a session, which may not arrive cleanly.

Entry format: what changed, what it unblocks, what is next, and anything surprising
found on the way. Surprises are the most valuable field; they are what a fresh
context cannot re-derive.
**This table is generated.** Edit an item under `worklog/`, then run
`tools/split-doc.sh --index orbistoun WORKLOG 1 worklog`.

| date | entry |
|---|---|
| 2026-08-19 | [Scaffold](worklog/001-scaffold.md) |
| 2026-08-19 | [Terminology and documentation pass](worklog/002-terminology-and-documentation-pass.md) |
| 2026-08-19 | [Concept intake and roadmap resequence](worklog/003-concept-intake-and-roadmap-resequence.md) |
| 2026-08-19 | [Test corpus and the accuracy suite](worklog/004-test-corpus-and-the-accuracy-suite.md) |
| 2026-08-19 | [Observability and per-title overrides](worklog/005-observability-and-per-title-overrides.md) |
| 2026-08-19 | [Real material arrived; plan revised against it](worklog/006-real-material-arrived-plan-revised.md) |
| 2026-08-19 | [Crunch: phase 0c, 0e, and most of phase 1](worklog/007-crunch-phase-0c-0e-and-most-of-phase-1.md) |
| 2026-08-19 | [Crunch continued: phases 2, 0e, and 3](worklog/008-crunch-continued-phases-2-0e-and-3.md) |
| 2026-08-19 | [Crunch continued: worker mode and image placement](worklog/009-crunch-continued-worker-mode-and-image.md) |
| - | [Open at end of session](worklog/010-open-at-end-of-session.md) |
| 2026-08-19 | [Guest code executes](worklog/011-guest-code-executes.md) |
| 2026-08-19 | [(later) - The guest runs, and says what it wants](worklog/012-later-the-guest-runs-and-says-what-it.md) |
| 2026-08-19 | [(later still) - The name search](worklog/013-later-still-the-name-search.md) |
| 2026-08-19 | [(evening) - Names, and proving they are ours](worklog/014-evening-names-and-proving-they-are-ours.md) |
| 2026-08-19 | [(late) - The loop, made runnable](worklog/015-late-the-loop-made-runnable.md) |
| 2026-08-19 | [(night) - Making the loop canonical](worklog/016-night-making-the-loop-canonical.md) |
| 2026-08-19 | [(very late) - The first implemented function, and what it taught](worklog/017-very-late-the-first-implemented.md) |
| 2026-08-19 | [GPU instrumentation: counting before translating](worklog/018-gpu-instrumentation-counting-before.md) |
| 2026-08-19 | [The encoding table stops being a guess](worklog/019-the-encoding-table-stops-being-a-guess.md) |
| 2026-08-19 | [Operands, and a surface to see them through](worklog/020-operands-and-a-surface-to-see-them.md) |
| 2026-08-19 | [Operand layouts, and the limit of the per-family model](worklog/021-operand-layouts-and-the-limit-of-the.md) |
| 2026-08-19 | [Per-opcode operands, solved rather than written](worklog/022-per-opcode-operands-solved-rather-than.md) |
| 2026-08-19 | [The execution model, decided and stubbed](worklog/023-the-execution-model-decided-and-stubbed.md) |
| 2026-08-19 | [Translated shaders can now be run, not just validated](worklog/024-translated-shaders-can-now-be-run-not.md) |
| 2026-08-19 | [The first guest instruction, translated and executed](worklog/025-the-first-guest-instruction-translated.md) |
| 2026-08-19 | [The worklist starts moving](worklog/026-the-worklist-starts-moving.md) |
| 2026-08-20 | [Two wavefront models, agreeing](worklog/027-two-wavefront-models-agreeing.md) |
| 2026-08-20 | [Factoring, floats, and the solver earning its refusals](worklog/028-factoring-floats-and-the-solver-earning.md) |
| 2026-08-20 | [Flat memory layouts, and the same lesson learned twice](worklog/029-flat-memory-layouts-and-the-same-lesson.md) |
| - | [Guest memory, and two driver faults that a builder could have caught](worklog/030-guest-memory-and-two-driver-faults-that.md) |
| - | [The worklist blockers, and a solver that was quietly wrong about three of them](worklog/031-the-worklist-blockers-and-a-solver-that.md) |
| - | [The last three encoding families, and one of them was wrong](worklog/032-the-last-three-encoding-families-and.md) |
| - | [The execution mask becomes real](worklog/033-the-execution-mask-becomes-real.md) |
| - | [Comparisons, and a conditional shader with no branch in it](worklog/034-comparisons-and-a-conditional-shader.md) |
| - | [Lanes that do different things](worklog/035-lanes-that-do-different-things.md) |
| - | [Control flow: a switch inside a loop](worklog/036-control-flow-a-switch-inside-a-loop.md) |
| - | [The seam: a submitted command buffer reaches the GPU](worklog/037-the-seam-a-submitted-command-buffer.md) |
| - | [The condition code, and the first compiled shader to translate](worklog/038-the-condition-code-and-the-first.md) |
| - | [Vector arithmetic, and the bits nothing was looking at](worklog/039-vector-arithmetic-and-the-bits-nothing.md) |
| - | [The condition code, everywhere it was missing](worklog/040-the-condition-code-everywhere-it-was.md) |
| 2026-08-20 | [Attribution, the submit function, and honest word lists](worklog/041-attribution-the-submit-function-and.md) |
| - | [The second sub-encoding, and a check that was lying](worklog/042-the-second-sub-encoding-and-a-check.md) |
| - | [Two routes to a shader, because the loader thread named the entry points](worklog/043-two-routes-to-a-shader-because-the.md) |
| - | [Reconciling with the loader thread](worklog/044-reconciling-with-the-loader-thread.md) |
| 2026-08-20 | [(later) - A home for what we learn](worklog/045-later-a-home-for-what-we-learn.md) |
| - | [G9: the capture format, and a guard that was right](worklog/046-g9-the-capture-format-and-a-guard-that.md) |
| - | [G13 measured rather than assumed, and the work that does have an oracle](worklog/047-g13-measured-rather-than-assumed-and.md) |
| - | [The local data share, and an operand that was never in any sample](worklog/048-the-local-data-share-and-an-operand.md) |
| - | [Using the oracle properly](worklog/049-using-the-oracle-properly.md) |
| - | [The generator found a real bug, which is what it was for](worklog/050-the-generator-found-a-real-bug-which-is.md) |
| - | [The target generation was never checked](worklog/051-the-target-generation-was-never-checked.md) |
| - | [Named instructions, because there are two targets](worklog/052-named-instructions-because-there-are.md) |
| 2026-08-20 | [(evening) - The wall had a name, and it was C++](worklog/053-evening-the-wall-had-a-name-and-it-was-c.md) |
| - | [The dispatch speaks names](worklog/054-the-dispatch-speaks-names.md) |
| - | [The retarget, and the tree is red on purpose](worklog/055-the-retarget-and-the-tree-is-red-on.md) |
| - | [The retarget landed](worklog/056-the-retarget-landed.md) |
| - | [Threads are real, and the entry point was reading garbage](worklog/057-threads-are-real-and-the-entry-point.md) |
| - | [The crash was the harness rebuilding Vulkan ninety-six times](worklog/058-the-crash-was-the-harness-rebuilding.md) |
| - | [The retarget was not finished, and cargo had been hiding it](worklog/059-the-retarget-was-not-finished-and-cargo.md) |
| - | [The entry image is built, and measured out of the running](worklog/060-the-entry-image-is-built-and-measured.md) |
| - | [The division sequence, two thirds of it](worklog/061-the-division-sequence-two-thirds-of-it.md) |
| - | [The tail found the wall, and the wall moved once](worklog/062-the-tail-found-the-wall-and-the-wall.md) |
| - | [A debugger, of the cheap kind - and it named the parked bug in one run](worklog/063-a-debugger-of-the-cheap-kind-and-it.md) |
| - | [The number nobody had recorded: every guest call was misaligned](worklog/064-the-number-nobody-had-recorded-every.md) |
| - | [Float controls, then narrow wavefronts](worklog/065-float-controls-then-narrow-wavefronts.md) |
| - | [Starting the GUI found the leak before writing a line of GUI](worklog/066-starting-the-gui-found-the-leak-before.md) |
| - | [The subgroup level runs](worklog/067-the-subgroup-level-runs.md) |
| - | [Phase 2b: a window, and it launches guests](worklog/068-phase-2b-a-window-and-it-launches-guests.md) |
| - | [The last unblocked instruction, and the buffer descriptor's operands](worklog/069-the-last-unblocked-instruction-and-the.md) |
| - | [The shell grew a menu, a toolbar, and an argument about dead controls](worklog/070-the-shell-grew-a-menu-a-toolbar-and-an.md) |
| - | [Untyped buffer access](worklog/071-untyped-buffer-access.md) |
| - | [The worklist ranks by reachability first](worklog/072-the-worklist-ranks-by-reachability-first.md) |
| - | [What downloadable homebrew can and cannot do for us](worklog/073-what-downloadable-homebrew-can-and.md) |
| - | [Library rows were being rebuilt sixty times a second](worklog/074-library-rows-were-being-rebuilt-sixty.md) |
| - | [The library folder is a setting now, and refresh is a button](worklog/075-the-library-folder-is-a-setting-now-and.md) |
| - | [A filesystem, and the guest read every file it asked for](worklog/076-a-filesystem-and-the-guest-read-every.md) |
| - | [Review queue, and the shader loop gets a verdict](worklog/077-review-queue-and-the-shader-loop-gets-a.md) |
| - | [The bisection lever had never been connected](worklog/078-the-bisection-lever-had-never-been.md) |
| - | [The two rungs](worklog/079-the-two-rungs.md) |
| - | [One name out of a quarter million, and sixteen useful refusals](worklog/080-one-name-out-of-a-quarter-million-and.md) |
| - | [The name sweep could not have found the answer](worklog/081-the-name-sweep-could-not-have-found-the.md) |
| - | [The decision log had been quietly losing its own references](worklog/082-the-decision-log-had-been-quietly.md) |
| - | [Video-out handles, two more names, and a wall that will not move](worklog/083-video-out-handles-two-more-names-and-a.md) |
| - | [The sub-encoding list was never needed](worklog/084-the-sub-encoding-list-was-never-needed.md) |
| - | [The oracle for hidden side effects was in the fixtures already](worklog/085-the-oracle-for-hidden-side-effects-was.md) |
| - | [Notes from the obSCEne thread: one declined, one implemented](worklog/086-notes-from-the-obscene-thread-one.md) |
| - | [A recorded gap was recorded wrongly](worklog/087-a-recorded-gap-was-recorded-wrongly.md) |
| - | [The dispatch loop was not where the cost was](worklog/088-the-dispatch-loop-was-not-where-the.md) |
| - | [The log line became a check](worklog/089-the-log-line-became-a-check.md) |
| - | [A stack trace, at last](worklog/090-a-stack-trace-at-last.md) |
| - | [Call sites, aliasing, and a wall that has taken eight attempts](worklog/091-call-sites-aliasing-and-a-wall-that-has.md) |
| - | [Three assumptions asked a question they had only ever answered themselves](worklog/092-three-assumptions-asked-a-question-they.md) |
| - | [The filesystem is cleared, and there are four walls](worklog/093-the-filesystem-is-cleared-and-there-are.md) |
| - | [The most common instruction in the set had no operands, and every test was green](worklog/094-the-most-common-instruction-in-the-set.md) |
| - | [The split opcode, and a test that asked to be deleted](worklog/095-the-split-opcode-and-a-test-that-asked.md) |
| - | [Two titles that never parsed, and an abort reported as an illegal instruction](worklog/096-two-titles-that-never-parsed-and-an.md) |
| - | [Typed buffers: operands solved, formats measured](worklog/097-typed-buffers-operands-solved-formats.md) |
| - | [The decision-number ceiling drops to thirteen](worklog/098-the-decision-number-ceiling-drops-to.md) |
| - | [A documentation sweep, because six units of change left claims behind](worklog/099-a-documentation-sweep-because-six-units.md) |
| - | [The run now says what to do about itself](worklog/100-the-run-now-says-what-to-do-about-itself.md) |
| - | [Typed buffer translation, and the last thing in this lane that needed nobody](worklog/101-typed-buffer-translation-and-the-last.md) |
| - | [The decode side is finished](worklog/102-the-decode-side-is-finished.md) |
| 2026-08-21 | [Behavioural provenance (D180)](worklog/103-behavioural-provenance-d180.md) |
| 2026-08-21 | [Run conditions and the discount on the headline (D181)](worklog/104-run-conditions-and-the-discount-on-the.md) |
| - | [A provenance rule that was stated too narrowly, and a refusal defended for the wrong reason](worklog/105-a-provenance-rule-that-was-stated-too.md) |
| 2026-08-21 | [The compatibility record (D182)](worklog/106-the-compatibility-record-d182.md) |
| - | [The third-source idea had no consumer, and I had already written that it did](worklog/107-the-third-source-idea-had-no-consumer.md) |
| 2026-08-21 | [snprintf_s, and what "implemented" does not mean (D183)](worklog/108-snprintf-s-and-what-implemented-does.md) |
| 2026-08-21 | [The abort-at-53 is one bug, in two Unity titles](worklog/109-the-abort-at-53-is-one-bug-in-two-unity.md) |
| - | [The interop contract, pinned before either side has code](worklog/110-the-interop-contract-pinned-before.md) |
| - | [The probe-record reader, built against transcripts and no hardware](worklog/111-the-probe-record-reader-built-against.md) |
| 2026-08-21 | [The abort-at-53, diagnosed (D186, D187)](worklog/112-the-abort-at-53-diagnosed-d186-d187.md) |
| 2026-08-21 | [The database was there all along (D188)](worklog/113-the-database-was-there-all-along-d188.md) |
| 2026-08-21 | [A shape, not a word list (D189), and an audit that was already red](worklog/114-a-shape-not-a-word-list-d189-and-an.md) |
| - | [Provenance across the boundary, and the demotion that has to stay visible](worklog/115-provenance-across-the-boundary-and-the.md) |
| 2026-08-21 | [memalign, and one allocation path (D190)](worklog/116-memalign-and-one-allocation-path-d190.md) |
| 2026-08-21 | [The nine-attempt wall: a clean negative, after three false starts](worklog/117-the-nine-attempt-wall-a-clean-negative.md) |
| - | [The corpus is records all the way down, and the reader was looking in the wrong place](worklog/118-the-corpus-is-records-all-the-way-down.md) |
| - | [From "a check passed" to "this function returns this, and here is how well we know it"](worklog/119-from-a-check-passed-to-this-function.md) |
| - | [A record becomes a knowledge entry, graded, or it does not become one at all](worklog/120-a-record-becomes-a-knowledge-entry.md) |
| - | [Symbols are graded differently from values, and four record kinds were left unparsed](worklog/121-symbols-are-graded-differently-from.md) |
| - | [Per-area coverage, and what a skip is not](worklog/122-per-area-coverage-and-what-a-skip-is-not.md) |
| - | [A probe cannot certify its own machine, and the grading was resting on it](worklog/123-a-probe-cannot-certify-its-own-machine.md) |
| - | [orbistoun drives the session now, and D207 said it never would](worklog/124-orbistoun-drives-the-session-now-and.md) |
| 2026-08-22 | [FreeBSD harvest: the walker ignored its own rule (D191)](worklog/125-freebsd-harvest-the-walker-ignored-its.md) |
| - | [`call` and `read` are live, and the fixture for one of them is broken](worklog/126-call-and-read-are-live-and-the-fixture.md) |
| - | [The streamed report needed no change, and now there is a test saying so](worklog/127-the-streamed-report-needed-no-change.md) |
| - | [The pinned defect fired, and the replacement pins the shape instead](worklog/128-the-pinned-defect-fired-and-the.md) |
| - | [Three ways of not knowing, kept as three](worklog/129-three-ways-of-not-knowing-kept-as-three.md) |
| - | [A shared bridge file, and the open-enum rule caught a live bug here](worklog/130-a-shared-bridge-file-and-the-open-enum.md) |
| 2026-08-22 | [Closing the automation gaps (D193, D194, D195)](worklog/131-closing-the-automation-gaps-d193-d194.md) |
| 2026-08-24 | [PROJECT_STATUS.md rewritten, because it opened with a falsehood](worklog/132-project-status-md-rewritten-because-it.md) |
| - | [A claim made to the other thread, then tested](worklog/133-a-claim-made-to-the-other-thread-then.md) |
| 2026-08-24 | [The unknowns become a queue (D196)](worklog/134-the-unknowns-become-a-queue-d196.md) |
| 2026-08-24 | [The loop, written down; and a documentation audit (D197, D198, D199)](worklog/135-the-loop-written-down-and-a.md) |
| 2026-08-24 | [The repository's own layout, audited (D208)](worklog/136-the-repository-s-own-layout-audited-d208.md) |
| 2026-08-24 | [`orbistoun-gen`: the table generators, and the seam that makes them checkable (D209)](worklog/137-orbistoun-gen-the-table-generators-and.md) |
| 2026-08-24 | [The obSCEne bridge caught a live memory bug (D210)](worklog/138-the-obscene-bridge-caught-a-live-memory.md) |
| 2026-08-24 | [What `observed` actually meant, and the ceiling that was not one (D213)](worklog/139-what-observed-actually-meant-and-the.md) |
| 2026-08-24 | [A language-model service, isolated from everything](worklog/140-a-language-model-service-isolated-from.md) |
| 2026-08-24 | [The first proposer, and the optimisation that had to be reverted](worklog/141-the-first-proposer-and-the-optimisation.md) |
| 2026-08-24 | [The toolbar captures the window, and recording is greyed out (D215)](worklog/142-the-toolbar-captures-the-window-and.md) |
| 2026-08-24 | [The wall was readable all along (D217)](worklog/143-the-wall-was-readable-all-along-d217.md) |
| 2026-08-24 | [Four eliminations, no confirmations (D218)](worklog/144-four-eliminations-no-confirmations-d218.md) |
| 2026-08-24 | [A diagnostics toolkit, and the registry that should have come first (D220, D221)](worklog/145-a-diagnostics-toolkit-and-the-registry.md) |
| 2026-08-24 | [A build says which build it is (D222)](worklog/146-a-build-says-which-build-it-is-d222.md) |
| 2026-08-25 | [The wall moved, and the address had been right all along (D223, D224)](worklog/147-the-wall-moved-and-the-address-had-been.md) |
| - | [The console gets a console](worklog/148-the-console-gets-a-console.md) |
| - | [The rule became code, and a convenience script nearly corrupted the fixtures](worklog/149-the-rule-became-code-and-a-convenience.md) |
| 2026-08-25 | [two eliminations that were never measured](worklog/150-two-eliminations-that-were-never.md) |
| - | [Every diagnostic axis, and a dispatcher that turns the loop](worklog/151-every-diagnostic-axis-and-a-dispatcher.md) |
| - | [Two files that disagreed, and the last unexplained sweep result](worklog/152-two-files-that-disagreed-and-the-last.md) |
| - | [The last channel into the wall, and two words that paid for themselves](worklog/153-the-last-channel-into-the-wall-and-two.md) |
| - | [Pointing the model at the right question](worklog/154-pointing-the-model-at-the-right-question.md) |
| - | [Seven names, two words, and a stale binary](worklog/155-seven-names-two-words-and-a-stale-binary.md) |
| - | [Asking whether the next name needs a word or a shape](worklog/156-asking-whether-the-next-name-needs-a.md) |
| - | [Three measured shapes, and a delimiter that could not spell a library](worklog/157-three-measured-shapes-and-a-delimiter.md) |
| - | [The model gets a door of its own](worklog/158-the-model-gets-a-door-of-its-own.md) |
| - | [Watchpoints, and what the first one found](worklog/159-watchpoints-and-what-the-first-one-found.md) |
| - | [A library that was written, declared, tested, and never registered](worklog/160-a-library-that-was-written-declared.md) |
| - | [The sweep gains a second dimension, and a wall opens](worklog/161-the-sweep-gains-a-second-dimension-and.md) |
| - | [The loop turns itself, and clears the wall doing it](worklog/162-the-loop-turns-itself-and-clears-the.md) |
| - | [Promotion: what a turn measured, and what it did not](worklog/163-promotion-what-a-turn-measured-and-what.md) |
| - | [The merge rule leaves the shim](worklog/164-the-merge-rule-leaves-the-shim.md) |
| - | [Every floating-point function worked, and every one was reported missing](worklog/165-every-floating-point-function-worked.md) |
| - | [The dispatcher becomes its own crate, and the loop becomes a command](worklog/166-the-dispatcher-becomes-its-own-crate.md) |
| - | [Teach a man to fish: the stub policy learns to write](worklog/167-teach-a-man-to-fish-the-stub-policy.md) |
| - | [Tier one: the loop writes its own patch](worklog/168-tier-one-the-loop-writes-its-own-patch.md) |
| - | [The learned file becomes something you can send somebody](worklog/169-the-learned-file-becomes-something-you.md) |
| - | [The loop finds a gap nobody had looked at](worklog/170-the-loop-finds-a-gap-nobody-had-looked.md) |
| - | [Two hypotheses instead of one, and the oracle that could not tell them apart](worklog/171-two-hypotheses-instead-of-one-and-the.md) |
| - | [The oracle arrives, and immediately corroborates](worklog/172-the-oracle-arrives-and-immediately.md) |
| - | [Five hours to fourteen seconds](worklog/173-five-hours-to-fourteen-seconds.md) |
| 2026-08-26 | [The loader learned to read the other half of the world](worklog/174-the-loader-learned-to-read-the-other.md) |
| - | [The vocabulary regrew, and the only alarm was a clock](worklog/175-the-vocabulary-regrew-and-the-only.md) |
| - | [The shell button had nowhere to go](worklog/176-the-shell-button-had-nowhere-to-go.md) |
| - | [One word for the shell, and the window had already taken it](worklog/177-one-word-for-the-shell-and-the-window.md) |
| 2026-08-27 | [Asked the guest what it wanted instead of guessing](worklog/178-asked-the-guest-what-it-wanted-instead.md) |
| 2026-08-27 | [Data imports finally get something they can dereference](worklog/179-data-imports-finally-get-something-they.md) |
| 2026-08-27 | [Two backlog items, and a method that was quietly unsound](worklog/180-two-backlog-items-and-a-method-that-was.md) |
| 2026-08-27 | [A model that needs no key, and the parts not worth copying](worklog/181-a-model-that-needs-no-key-and-the-parts.md) |
| 2026-08-27 | [The benchmark disagreed with me twice before it was right](worklog/182-the-benchmark-disagreed-with-me-twice.md) |
| - | [A controller subsystem, and a name I talked myself out of](worklog/183-a-controller-subsystem-and-a-name-i.md) |
| 2026-08-27 | [The payloads started working](worklog/184-the-payloads-started-working.md) |
| 2026-08-27 | [Asked to drop an engine, and the engine was fine](worklog/185-asked-to-drop-an-engine-and-the-engine.md) |
| - | [Two modelled states made real, after being told they were only modelled](worklog/186-two-modelled-states-made-real-after.md) |
| 2026-08-27 | [The tags outlived the instruction that removed the thinking](worklog/187-the-tags-outlived-the-instruction-that.md) |
| - | [User support, and the third thing found modelled and inert](worklog/188-user-support-and-the-third-thing-found.md) |
| 2026-08-27 | [klogsrv prints its banner](worklog/189-klogsrv-prints-its-banner.md) |
| - | [The POSIX names were unserved, and they were never missing behaviour](worklog/190-the-posix-names-were-unserved-and-they.md) |
| 2026-08-27 | [The guest names its own requirement](worklog/191-the-guest-names-its-own-requirement.md) |
| 2026-08-27 | [The BSD harvest was names-only, and the work outgrew it](worklog/192-the-bsd-harvest-was-names-only-and-the.md) |
| - | [Coverage crunch: libc, elf, hle](worklog/193-coverage-crunch-libc-elf-hle.md) |
| 2026-08-27 | [The harvest became a command, and writing it twice found a bug](worklog/194-the-harvest-became-a-command-and.md) |
| - | [Coverage crunch: kernel and thunk](worklog/195-coverage-crunch-kernel-and-thunk.md) |
| 2026-08-27 | [Proof of sourcing for the harvested constants](worklog/196-proof-of-sourcing-for-the-harvested.md) |
| 2026-08-29 | [The null jump is not the sysctl refusal](worklog/197-the-null-jump-is-not-the-sysctl-refusal.md) |
| 2026-08-29 | [The null jump was the shortcut coming due](worklog/198-the-null-jump-was-the-shortcut-coming.md) |
| 2026-08-29 | [Markers in `.bss`, and the SDK's logging helper](worklog/199-markers-in-bss-and-the-sdk-s-logging.md) |
| 2026-08-29 | [Gate green](worklog/200-gate-green.md) |
| 2026-08-29 | [The payload wall, answered by asking the guest](worklog/201-the-payload-wall-answered-by-asking-the.md) |
| 2026-08-29 | [Sockets, and the file calls that go with them](worklog/202-sockets-and-the-file-calls-that-go-with.md) |
| 2026-08-29 | [klogsrv imports nothing that is not implemented](worklog/203-klogsrv-imports-nothing-that-is-not.md) |
| 2026-08-29 | [What about klogsrv](worklog/204-what-about-klogsrv.md) |
| 2026-08-29 | [The syscall boundary, built and not yet reached](worklog/205-the-syscall-boundary-built-and-not-yet.md) |
| 2026-08-29 | [A setting consulted nowhere, for the fourth time](worklog/206-a-setting-consulted-nowhere-for-the.md) |
| 2026-08-29 | [A fault report that names our own code](worklog/207-a-fault-report-that-names-our-own-code.md) |
| 2026-08-29 | [klogsrv opens a port and something connects to it](worklog/208-klogsrv-opens-a-port-and-something.md) |
| 2026-08-29 | [ftpsrv talks, and asks to be root](worklog/209-ftpsrv-talks-and-asks-to-be-root.md) |
| 2026-08-29 | [A one-line disagreement about zero](worklog/210-a-one-line-disagreement-about-zero.md) |
| 2026-08-30 | [An FTP server, and the six things in front of it](worklog/211-an-ftp-server-and-the-six-things-in.md) |
| 2026-08-30 | [The listing](worklog/212-the-listing.md) |
| 2026-08-30 | [The download, and three blind instruments](worklog/213-the-download-and-three-blind-instruments.md) |
| 2026-08-30 | [When, not which](worklog/214-when-not-which.md) |
| 2026-08-30 | [Ask the guest, one field at a time](worklog/215-ask-the-guest-one-field-at-a-time.md) |
| 2026-08-30 | [What real hardware sent back](worklog/216-what-real-hardware-sent-back.md) |
| 2026-08-30 | [The probe answers a question about itself](worklog/217-the-probe-answers-a-question-about.md) |
| 2026-08-30 | [Seven placeholders, retired by one file](worklog/218-seven-placeholders-retired-by-one-file.md) |
| 2026-08-30 | [The constant was a handle, and the wall was an argument](worklog/219-the-constant-was-a-handle-and-the-wall.md) |
| 2026-08-30 | [A landing zone, and the first syscalls a payload has ever made here](worklog/220-a-landing-zone-and-the-first-syscalls-a.md) |
| 2026-08-30 | [The work list could not see a system call](worklog/221-the-work-list-could-not-see-a-system.md) |
| 2026-08-30 | [The hardware had already answered; the results were on disk](worklog/222-the-hardware-had-already-answered-the.md) |
| 2026-08-30 | [Word zero is getpid, and the payload's own arithmetic lands on real functions](worklog/223-word-zero-is-getpid-and-the-payload-s.md) |
| 2026-08-31 | [Four plans crunched: testable layout, profiles, port reporting, vaddr provenance](worklog/224-four-plans-crunched-testable-layout.md) |
| 2026-08-31 | [obSCEne's hardware run absorbed: five vaddrs confirmed, and a payload retry](worklog/225-obscene-s-hardware-run-absorbed-five.md) |
| 2026-08-31 | [Disassembled `klog.elf` around `image+0x2708`: kernel_copyout, setsockopt, and high-half kpipe_addr (D411)](worklog/226-disassembled-klog-elf-around-image.md) |
| 2026-08-31 | [Implemented Syscall 477 (`mmap`) and wired to the syscall table (D412)](worklog/227-implemented-syscall-477-mmap-and-wired.md) |
| 2026-08-31 | [(later) - payload retry after further changes: 7 -> 32 syscalls, into klog's socket loop](worklog/228-later-payload-retry-after-further.md) |
| 2026-08-31 | [Emulated kernel escape R/W pipe for dynamic symbol resolution (D413)](worklog/229-emulated-kernel-escape-r-w-pipe-for.md) |
| 2026-08-31 | [(later) - orbistoun runs the whole obSCEne suite; obSCEne becomes its conformance oracle](worklog/230-later-orbistoun-runs-the-whole-obscene.md) |
| 2026-08-31 | [(later) - the test corpus becomes a verb (D414)](worklog/231-later-the-test-corpus-becomes-a-verb.md) |
| 2026-08-31 | [(later) - COMPATIBILITY.md, generated from the records (D415)](worklog/232-later-compatibility-md-generated-from.md) |
| 2026-08-31 | [(later) - four HLE fixes from the hardware diff (D416)](worklog/233-later-four-hle-fixes-from-the-hardware.md) |
| 2026-08-31 | [(later) - three more HLE fixes: thread join, mutex type, audio (D417)](worklog/234-later-three-more-hle-fixes-thread-join.md) |
| 2026-08-31 | [(later) - the census-control leak, root-caused and fixed (D418)](worklog/235-later-the-census-control-leak-root.md) |
| 2026-08-31 | [SwVersion write refined from the hardware dump](worklog/236-swversion-write-refined-from-the.md) |
| 2026-08-31 | [(later) - second payload run absorbed: five vaddrs confirmed (D419)](worklog/237-later-second-payload-run-absorbed-five.md) |
| 2026-08-31 | [(later) - the "surely there's more" pass found a wrong firmware value (D420)](worklog/238-later-the-surely-there-s-more-pass.md) |
| 2026-08-31 | [(later) - sceKernelGetSystemSwVersion made a profile setting (D421)](worklog/239-later-scekernelgetsystemswversion-made.md) |
| 2026-08-31 | [(later) - per-title device sandbox, from the overlay already there (D422)](worklog/240-later-per-title-device-sandbox-from-the.md) |
| 2026-08-31 | [(later) - C succeeded: the full current obSCEne runs as the payload](worklog/241-later-c-succeeded-the-full-current.md) |
| 2026-08-31 | [(later) - video flip model: the first complete obSCEne run in orbistoun (D424)](worklog/242-later-video-flip-model-the-first.md) |
| 2026-08-31 | [(later) - first fidelity fix off the complete run: GetModuleInfo refusal code](worklog/243-later-first-fidelity-fix-off-the.md) |
| 2026-08-31 | [(later) - resolution-status: implemented, and the "headless" skip debunked](worklog/244-later-resolution-status-implemented-and.md) |
| 2026-09-01 | [systematic hardware diff: video error family + single-output-ownership (D426)](worklog/245-systematic-hardware-diff-video-error.md) |
| 2026-09-01 | [started the GPU subsystem: PM4 command builders (D427)](worklog/246-started-the-gpu-subsystem-pm4-command.md) |
| 2026-09-01 | [real-title loop: libSceSysmodule (the load call nearly every title makes)](worklog/247-real-title-loop-libscesysmodule-the.md) |
| 2026-09-01 | [reentrant guest execution + std::call_once (D430)](worklog/248-reentrant-guest-execution-std-call-once.md) |
| 2026-09-01 | [sceKernelVirtualQuery + the C-runtime threading family (D431)](worklog/249-scekernelvirtualquery-the-c-runtime.md) |
| 2026-09-01 | [Fault reports carry the faulting instruction (D432)](worklog/250-fault-reports-carry-the-faulting.md) |
| 2026-09-01 | [PPSA28061's wall diagnosed: no guest thread pointer (D432)](worklog/251-ppsa28061-s-wall-diagnosed-no-guest.md) |
| 2026-09-01 | [(/loop) - TLS block built; Windows fs-base limitation found; knowledge debt cleared](worklog/252-loop-tls-block-built-windows-fs-base.md) |
| 2026-09-01 | [(/loop) - Windows fs-base backstop; PPSA28061 FURTHER to the JSON/ULT wall](worklog/253-loop-windows-fs-base-backstop-ppsa28061.md) |
| 2026-09-01 | [(/loop) - libSceUlt mutex/condvar/ulthread; PPSA28061 wall is now online (D435)](worklog/254-loop-libsceult-mutex-condvar-ulthread.md) |
| 2026-09-01 | [(/loop) - names search: PPSA28061's online import is un-nameable locally](worklog/255-loop-names-search-ppsa28061-s-online.md) |
| 2026-09-01 | [(/loop) - remaining walls characterised; all need obscene/external data (D436)](worklog/256-loop-remaining-walls-characterised-all.md) |
| 2026-09-01 | [(/loop) - obSCEne hardware memory data mined; map now starts at 0x10000 (D437)](worklog/257-loop-obscene-hardware-memory-data-mined.md) |
| 2026-09-01 | [(/loop) - error-code sweep completed: 5/6 rejects-* checks match hardware (D438 cont.)](worklog/258-loop-error-code-sweep-completed-5-6.md) |
| 2026-09-01 | [(/loop) - file error codes now the measured errnos (D439 cont.)](worklog/259-loop-file-error-codes-now-the-measured.md) |
| 2026-09-01 | [(/loop) - GNM dispatch header now byte-matches hardware (D439 cont.)](worklog/260-loop-gnm-dispatch-header-now-byte.md) |
| 2026-09-01 | [(/loop, user task) - orbistoun-fs test isolation + clippy debt fixed](worklog/261-loop-user-task-orbistoun-fs-test.md) |
| 2026-09-01 | [Shared-allocator diagnosis: wrong titles, real gap is `Configured` (D442)](worklog/262-shared-allocator-diagnosis-wrong-titles.md) |
| 2026-09-01 | [proc-param reader built; the mem-param oracle falsified the budget premise](worklog/263-proc-param-reader-built-the-mem-param.md) |
| 2026-09-01 | [PPSA02664 goes FURTHER (233->1541 calls): policy region was on the guest's heap](worklog/264-ppsa02664-goes-further-233-1541-calls.md) |
| 2026-09-01 | [obSCEne is the oracle; flexible-memory fixed byte-exact against hardware](worklog/265-obscene-is-the-oracle-flexible-memory.md) |
| 2026-09-01 | [stack read-ahead guard; obSCEne runs its whole suite under orbistoun](worklog/266-stack-read-ahead-guard-obscene-runs-its.md) |
| 2026-09-01 | [sceKernelVirtualQuery sees the image and stack (obSCEne vq-text/vq-stack pass)](worklog/267-scekernelvirtualquery-sees-the-image.md) |
| 2026-09-01 | [sysctlbyname implemented; kern.osrelease answers (obSCEne osrelease passes)](worklog/268-sysctlbyname-implemented-kern-osrelease.md) |
| 2026-09-01 | [obSCEne oracle mined out for clean fixes; pivot to a real title next](worklog/269-obscene-oracle-mined-out-for-clean.md) |
| 2026-09-01 | [found PPSA02664 regression (1541->234); mechanism understood, fix deferred (net-zero turn)](worklog/270-found-ppsa02664-regression-1541-234.md) |
| 2026-09-01 | [(/loop) PPSA02664's fault site is non-deterministic; the "regression" was a thread race](worklog/271-ppsa02664-fault-site-is-non.md) |
| 2026-09-01 | [(/loop) PPSA21564 boots: the sceLibcMspace allocator family + bcmp](worklog/272-ppsa21564-boots-mspace-family.md) |
| 2026-09-01 | [(/loop) scePthreadGetthreadid; PPSA21564 now runs at 0% stubs](worklog/273-scepthreadgetthreadid-0pct-stubs.md) |
| 2026-09-01 | [(/loop) POSIX TLS keys unblock Unity's TBB scheduler; PPSA21564 stops aborting/racing](worklog/274-posix-tls-keys-tbb-unblocked.md) |
| 2026-09-01 | [(/loop) time + fgets: PPSA21564 (Astro's Playroom) reaches main(), prints, parses args](worklog/275-time-and-fgets-astro-reaches-main.md) |
| 2026-09-01 | [(/loop) POSIX unnamed semaphores; the Cond.cpp wall is not a traced HLE call](worklog/276-posix-unnamed-semaphores.md) |
| 2026-09-01 | [(/loop) Fault reports name privileged instructions + distinguish emulator bugs](worklog/277-fault-reports-name-privileged-instructions.md) |
| 2026-09-01 | [(/loop) Fault reports name the null base register automatically](worklog/278-fault-reports-name-the-null-base.md) |
| 2026-09-01 | [(/loop) Execute breakpoints: capture a guest value where it is used](worklog/279-execute-breakpoints.md) |
| 2026-09-01 | [(/loop) The call trace records what each call answered](worklog/280-trace-records-return-values.md) |
| 2026-09-01 | [(/loop) Map direct memory into an existing reservation; crack image+0xafcc08](worklog/281-map-commits-into-a-reserved-range.md) |
| 2026-09-01 | [(/loop) The reserve-then-map fix advanced three titles, not one](worklog/282-map-fix-blast-radius.md) |
| 2026-09-01 | [(/loop) Overnight crunch: putchar and std::random_device](worklog/283-standard-libc-functions-putchar-random-device.md) |
| 2026-09-01 | [(/loop) PPSA04263's map wall: a rigorous narrowing, not yet a fix](worklog/284-ppsa04263-map-failure-investigation.md) |
| 2026-09-01 | [(/loop) Made reservation failures legible, then cracked the collision they hid](worklog/285-mapping-arena-collision-fixed.md) |
| 2026-09-02 | [(/loop) strcpy_s implemented; new-thread TLS gap diagnosed (the real wall)](worklog/286-strcpy-s-and-new-thread-tls-diagnosis.md) |
| 2026-09-02 | [(/loop) Per-thread TLS for spawned guest threads: PPSA04263 10k -> 333k calls](worklog/287-per-thread-tls-implemented.md) |
| 2026-09-02 | [(/loop) Blocking WaitEventFlag (kills a 304k spin) + atan2f/sincosf](worklog/288-wait-event-flag-and-math.md) |
| 2026-09-02 | [(/loop) PPSA25872's unnamed spin (naming-gated) + pthread attr setters](worklog/289-ppsa25872-spin-and-attr-setters.md) |
| 2026-09-02 | [(/loop) The last clearly-standard stubs: vsprintf_s, SetVirtualRangeName](worklog/290-last-standard-stubs.md) |
| 2026-09-02 | [(/loop) The last clean functions; the oracle-free crunch is complete](worklog/291-last-clean-functions-and-crunch-summary.md) |
| 2026-09-02 | [(/loop) Packed typed-buffer formats begin: UINT single-word load, GPU-verified](worklog/292-packed-uint-buffer-load.md) |
| 2026-09-02 | [(/loop) Packed typed-buffer SINT load: sign extension, GPU-verified](worklog/293-packed-sint-buffer-load.md) |
| 2026-09-02 | [(/loop) Packed typed-buffer UNORM load: the first converting kind, GPU-verified](worklog/294-packed-unorm-buffer-load.md) |
| 2026-09-02 | [(/loop) Packed typed-buffer SNORM load: the clamp, GPU-verified](worklog/295-packed-snorm-buffer-load.md) |
| 2026-09-02 | [(/loop) Packed typed-buffer FLOAT16 load: driver-widened halves, GPU-verified](worklog/296-packed-float16-buffer-load.md) |
| 2026-09-02 | [(/loop) The ctype tables, measured off hardware: table right, wall unchanged](worklog/297-ctype-tables-from-hardware.md) |
| 2026-09-02 | [(/loop) The `_Getpctype` wall is a mislabel: measured, and it reframes four decisions](worklog/298-the-wall-is-not-getpctype.md) |
| 2026-09-02 | [(/loop) Every report today was a file from 01:50; the ctype work had moved the wall](worklog/299-a-stale-trace-was-being-reported-as-this-run.md) |
| 2026-09-02 | [(/loop) The reporter could not survive an execute fault; fixed, and the ctype work turns out to be a 7x advance](worklog/300-the-ctype-work-was-a-7x-advance.md) |
| 2026-09-02 | [(/loop) 452 documented functions were knowable all along; the work list is now inventory-driven](worklog/301-the-work-list-is-now-inventory-driven.md) |
| 2026-09-02 | [(/loop) Bulk port batch 1: the bounded string and memory functions](worklog/302-batch-1-bounded-string-and-memory.md) |
| 2026-09-02 | [(/loop) Bulk port batch 2: the runtime's out-of-line atomics, conversions and assert](worklog/303-batch-2-runtime-internals.md) |
| 2026-09-02 | [(/loop) Bulk port batch 3: the standard streams, real recursive locks, and the C++ runtime](worklog/304-batch-3-streams-locks-and-the-cxx-runtime.md) |
| 2026-09-02 | [(/loop) Bulk port batch 4: 20 POSIX delegations, and the arity check that refused two](worklog/305-batch-4-posix-delegation-and-the-environment.md) |
| 2026-09-02 | [(/loop) Bulk port batch 5: the single-precision math family and byte order](worklog/306-batch-5-math-and-byte-order.md) |
| 2026-09-02 | [(/loop) A partial marker, and the inconsistency the audit found](worklog/307-the-partial-marker.md) |
| 2026-09-02 | [(/loop) Bulk port batch 7: scatter/gather, sync, and the ones refused instead](worklog/308-batch-7-descriptor-and-mapping-calls.md) |
| 2026-09-02 | [(/loop) Bulk port batch 8: the `posix_`-prefixed family, 69 in one go](worklog/309-batch-8-the-posix-prefixed-family.md) |
| 2026-09-02 | [(/loop) Bulk port batch 9: the attribute accessors, and three guards that fired](worklog/310-batch-9-the-attribute-accessors.md) |
| 2026-09-02 | [(/loop) Bulk port batch 10: `getsockopt`, and what "missing" actually means](worklog/311-batch-10-getsockopt-and-a-near-duplication.md) |
| 2026-09-02 | [(/loop) Bulk port batch 11: two inits resolved, and the cheap wins are exhausted](worklog/312-batch-11-the-cheap-wins-are-exhausted.md) |
| 2026-09-02 | [(/loop) Bulk port batch 12: the lock-attribute families - and my stop recommendation was wrong](worklog/313-batch-12-and-a-recommendation-that-was-wrong.md) |
| 2026-09-02 | [(/loop) Bulk port batch 13: the timed condition wait and `pthread_once`](worklog/314-batch-13-timed-wait-and-once.md) |
| 2026-09-02 | [(/loop) Bulk port batch 14: the timed acquisitions, and a guard that checked the wrong table](worklog/315-batch-14-deadlines-and-a-guard-on-the-wrong-table.md) |
| 2026-09-02 | [(/loop) R0: the red gates cleared, and the knowledge file catches up](worklog/316-r0-the-red-gates-cleared.md) |
| 2026-09-02 | [(/loop) R1: the measure count is not a bug, and the name oracle already answered](worklog/317-r1-the-measure-count-is-not-a-bug.md) |
| 2026-09-02 | [(/loop) R5: the hardware records finally get read, and the two runs disagree](worklog/318-r5-the-hardware-records-finally-get-read.md) |
| 2026-09-02 | [(/loop) R6: measurements become a work queue with a completion condition](worklog/319-r6-measurements-become-a-work-queue.md) |
| 2026-09-02 | [(/loop) R7: a live oracle at last, and it found four bugs in twenty-four cases](worklog/320-r7-the-differential-finds-four-bugs-on-day-one.md) |
| 2026-09-02 | [(/loop) More differential cases, and the parallel run finds a real concurrency bug](worklog/321-more-differential-cases-and-a-spurious-wakeup.md) |
| 2026-09-02 | [(/loop) The differential calls back into guest code, and `qsort` agrees](worklog/322-qsort-and-bsearch-call-back-into-guest-code.md) |
| 2026-09-02 | [(/loop) The sign-extension divergence was mine, not the console's](worklog/323-a-claim-i-made-was-wrong-and-is-withdrawn.md) |
| 2026-09-02 | [(/loop) `strtok` needs a sequence, and the mutex type mapping becomes a test](worklog/324-strtok-sequences-and-the-mutex-types-pinned.md) |
| 2026-09-02 | [(/loop) Four outstanding items that were never hard, and the overlapping moves](worklog/325-the-memory-query-flags-and-the-overlap-cases.md) |
| 2026-09-02 | [(/loop) The loader's answers were already right; the real gap is exports](worklog/326-the-loader-answers-were-already-right-and-the-real-gap-is-exports.md) |
| 2026-09-02 | [(/loop) Exports exist now, and the title's own module answers the wall](worklog/327-exports-exist-now-and-the-title-module-answers-the-wall.md) |
| 2026-09-02 | [(/loop) Three firmware directories, and a test that cannot tell them apart](worklog/328-three-firmware-directories-and-a-test-that-cannot-see-them.md) |
| 2026-09-03 | [(/loop) A title's module is found by name, and `fakelib/` proved the rule right](worklog/329-a-title-module-is-found-by-name-and-fakelib-proves-it.md) |
| 2026-09-03 | [(/loop) The wall resolves to a real address, and `libc` turns out to be the title's](worklog/330-the-wall-resolves-and-libc-turns-out-to-be-the-titles.md) |
| 2026-09-03 | [(/loop) The stub tables are global, so there is one of them (D484)](worklog/331-the-stub-tables-are-global-so-there-is-one-of-them.md) |
| 2026-09-03 | [(/loop) R9 was already closed, and twenty-one new differential cases](worklog/332-r9-was-already-closed-and-twenty-one-new-differential-cases.md) |
| 2026-09-03 | [(/loop) A third capture added 186 measurements and took two claims away](worklog/333-a-third-capture-took-two-claims-away.md) |
| 2026-09-03 | [(/loop) One stub table with a range per module](worklog/334-one-stub-table-with-a-range-per-module.md) |
| 2026-09-03 | [(/loop) The float environment, and the bit that is not configuration](worklog/335-the-float-environment-and-the-bit-that-is-not-configuration.md) |
| 2026-09-03 | [(/loop) The title's own modules relocate](worklog/336-the-titles-own-modules-relocate.md) |
| 2026-09-03 | [(/loop) The worker links the title, and the verdict turns out to be noisy](worklog/337-the-worker-links-the-title-and-the-verdict-is-noisy.md) |
| 2026-09-03 | [(/loop) The decision log had the answer](worklog/338-the-decision-log-had-the-answer.md) |
| 2026-09-03 | [(/loop) The guest runs its own code](worklog/339-the-guest-runs-its-own-code.md) |
| 2026-09-03 | [(/loop) The trace was naming the wrong functions](worklog/340-the-trace-was-naming-the-wrong-functions.md) |
| 2026-09-03 | [(/loop) The instruction bytes answered it, and the tags refused the answer](worklog/341-the-instruction-bytes-answered-it-and-the-tags-refused-the-answer.md) |
| 2026-09-03 | [(/loop) A `nid` verb, and a hypothesis that did not survive it](worklog/342-a-nid-verb-and-a-hypothesis-that-did-not-survive-it.md) |
| 2026-09-03 | [(/loop) `.bss`, and a blind spot of our own making](worklog/343-bss-and-a-blind-spot-of-our-own-making.md) |
| 2026-09-03 | [(/loop) The instrument existed, and I mis-grepped it](worklog/344-the-instrument-existed-and-i-mis-grepped-it.md) |
| 2026-09-03 | [(/loop) The guest says which modules to start, and we do not start them](worklog/345-the-guest-says-which-modules-to-start.md) |
| 2026-09-03 | [(/loop) Six absences, and the tag that made them mean something](worklog/346-six-absences-and-the-tag-that-made-them-mean-something.md) |
| 2026-09-03 | [(/loop) 24 claims, 24 non-claims, and a restore that broke a mutex](worklog/347-24-claims-24-non-claims-and-a-restore-that-broke-a-mutex.md) |
| 2026-09-03 | [(/loop) a conformance bug, a stale work item, and a lying instrument](worklog/348-a-conformance-bug-a-stale-work-item-and-a-lying-instrument.md) |
| 2026-09-03 | [(/loop) The oscillation is one allocation round, and the backlog is now the whole list](worklog/349-the-oscillation-is-one-allocation-round.md) |
| 2026-09-03 | [(/loop) What the 96% does not say, a stale red phase, and three more libc functions](worklog/350-what-the-96-percent-does-not-say.md) |
| 2026-09-03 | [(/loop) The citations were already here, and R9 was already closed](worklog/351-the-citations-were-already-here.md) |
| 2026-09-03 | [(/loop) The queue had permanent residents, and two asks were about the run](worklog/352-the-queue-had-permanent-residents.md) |
| 2026-09-03 | [(/loop) Skeletons for the gaps: 61 graphics names and 49 recorded imports](worklog/353-skeletons-for-the-gaps.md) |
| 2026-09-03 | [(/loop) Twenty-eight libraries, and a coverage figure that got worse on purpose](worklog/354-twenty-eight-libraries-and-an-honest-denominator.md) |
| 2026-09-03 | [(/loop) The encoder measurements say the opposite, and thirty names go back](worklog/355-the-encoder-measurements-say-the-opposite.md) |
| 2026-09-03 | [(/loop) A second capture settles the third query field](worklog/356-a-second-capture-settles-a-field.md) |
| 2026-09-03 | [(/loop) A masked register, and the width where a shim shows itself](worklog/357-a-masked-register-and-single-precision.md) |
| 2026-09-03 | [(/loop) Eight bytes where documentation said four](worklog/358-eight-bytes-and-a-round-trip.md) |
| 2026-09-03 | [(/loop) A closed gap that both surfaces still called open](worklog/359-a-closed-gap-that-both-surfaces-called-open.md) |
| 2026-09-03 | [(/loop) Two formatter bugs, in the padding nobody had tested](worklog/360-two-formatter-bugs.md) |
| 2026-09-03 | [(/loop) `vsnprintf` closes the list, and the break names the cases carrying it](worklog/361-vsnprintf-closes-the-list.md) |
| 2026-09-03 | [(/loop) The oscillation is settled: it was the guest's own allocator](worklog/362-the-oscillation-is-settled.md) |
| 2026-09-03 | [(/loop) The wall named: a module that was loaded, placed, and never started](worklog/363-the-wall-is-a-module-never-started.md) |
| 2026-09-03 | [(/loop) The wall is down: the guest reached its frame loop](worklog/364-the-wall-is-down.md) |
| 2026-09-03 | [(/loop) Nothing is ever pending: the guest left its frame loop](worklog/365-nothing-is-ever-pending.md) |
| 2026-09-03 | [(/loop) `dlsym` never knew the guest's own exports; the wall did not move](worklog/366-dlsym-and-a-wall-that-did-not-move.md) |
| 2026-09-03 | [(/loop) One writer, thirty-seven readers, and a guard that never passes](worklog/367-one-writer-thirty-seven-readers.md) |
| 2026-09-03 | [(/loop) I blamed a guard that never ran](worklog/368-i-blamed-a-guard-that-never-ran.md) |
| 2026-09-03 | [(/loop) Starting every module early is not the missing ordering](worklog/369-starting-everything-early-is-not-it.md) |
| 2026-09-03 | [(/loop) No branch avoids the read: half the open question is closed](worklog/370-no-branch-avoids-the-read.md) |
| 2026-09-03 | [(/loop) A fault now says what its registers point at, and the label is "None"](worklog/371-a-fault-says-what-it-points-at.md) |
| 2026-09-03 | [(/loop) One thread call implemented, one deliberately not](worklog/372-one-implemented-one-deliberately-not.md) |
| 2026-09-03 | [(/loop) The event queue exists so a handle means something](worklog/373-the-event-queue.md) |
| 2026-09-03 | [(/loop) The vendor `stat` is not the POSIX one under another name](worklog/374-the-vendor-stat.md) |
| 2026-09-03 | [(/loop) A positioned read took the guest into the GPU](worklog/375-a-positioned-read-into-the-gpu.md) |
| 2026-09-03 | [(/loop) The wall is `sceAgcCreateShader`, and a cap was hiding it](worklog/376-the-wall-is-a-shader.md) |
| 2026-09-03 | [(/loop) Two walls that need a measurement, routed to the mechanism that asks](worklog/377-routing-two-walls-to-the-console.md) |
| 2026-09-04 | [(/loop) The record format already carried what it was said to lack](worklog/378-the-format-already-carried-it.md) |
| 2026-09-04 | [(/loop) The wide family verified, and the case that bites](worklog/379-the-wide-family-verified.md) |
| 2026-09-04 | [(/loop) The interleaved sequence, and the sixteen cases that were blind](worklog/380-the-interleaved-sequence.md) |
| 2026-09-04 | [(/loop) `strftime`, and the question that found it](worklog/381-strftime-and-the-question.md) |
| 2026-09-04 | [(/loop) libm splits in two, and only half belongs here](worklog/382-libm-splits-in-two.md) |
| 2026-09-04 | [(/loop) Two rounding rules that pin each other](worklog/383-two-rounding-rules.md) |
| 2026-09-04 | [(/loop) `strdup` covered, and a missing terminator is only caught by luck](worklog/384-caught-by-luck.md) |
| 2026-09-04 | [(/loop) One clock, two names, and one origin](worklog/385-one-clock-two-names.md) |
| 2026-09-04 | [(/loop) The reasoning was in the code, and the record said nothing](worklog/386-the-record-said-nothing.md) |
| 2026-09-04 | [(/loop) Two thirds of the ask list is forty sentences](worklog/387-two-thirds-of-the-ask-list.md) |
| 2026-09-04 | [(/loop) One question, written a hundred and forty-nine ways](worklog/388-one-question-written-149-ways.md) |
| 2026-09-04 | [(/loop) The ask list was asking for a function that does not exist](worklog/389-asking-for-a-function-that-does-not-exist.md) |
| 2026-09-04 | [(/loop) A measurement was sitting in the list of things nobody knows](worklog/390-a-measurement-in-the-list-of-unknowns.md) |
| 2026-09-04 | [(/loop) Six records said nothing was known while holding a measurement](worklog/391-six-records-and-the-axis-closes.md) |
| 2026-09-04 | [(/loop) `dlsym` succeeds where the console refuses](worklog/392-dlsym-succeeds-where-the-console-refuses.md) |
| 2026-09-04 | [(/loop) Forty-seven measured values that no test looked at](worklog/393-measured-values-nothing-looked-at.md) |
| 2026-09-04 | [(/loop) The relations are testable where the numbers are not](worklog/394-relations-testable-where-numbers-are-not.md) |
| 2026-09-04 | [(/loop) A permanent resident of the work queue](worklog/395-a-permanent-resident-of-the-queue.md) |
| 2026-09-04 | [(/loop) The relation needed a mount, not a guest](worklog/396-the-relation-needed-a-mount.md) |
| 2026-09-04 | [(/loop) The wall is an experiment, and there is no guest to run](worklog/397-the-wall-is-an-experiment.md) |
| 2026-09-04 | [(/loop) The oracle the roadmap said did not exist](worklog/398-the-oracle-the-roadmap-said-did-not-exist.md) |
| 2026-09-04 | [(/loop) The oracle draws](worklog/399-the-oracle-draws.md) |
| 2026-09-04 | [(/loop) The export was already decoded, and its blocker had half expired](worklog/400-the-export-was-already-decoded.md) |
| 2026-09-04 | [(/loop) The fragment path does not need the feature](worklog/401-the-fragment-path-does-not-need-the-feature.md) |
| 2026-09-04 | [(/loop) The translator draws](worklog/402-the-translator-draws.md) |
| 2026-09-04 | [(/loop) The oracle carries a varying](worklog/403-the-oracle-carries-a-varying.md) |
| 2026-09-04 | [(/loop) The wall was real, the record was twelve days stale, VINTRP translates](worklog/404-the-wall-was-real.md) |
| - | [405. The out-parameter is the whole of the wall](worklog/405-the-out-parameter-is-the-whole-wall.md) |
| - | [406. A rung for the first frame, and a prop that was never counted](worklog/406-a-rung-for-the-first-frame.md) |
| - | [407. The Agc probe set, aimed from the guest's own arguments](worklog/407-the-agc-probe-set.md) |
| - | [408. The flip completion had no reader](worklog/408-the-flip-completion-had-no-reader.md) |
| - | [409. The pthread family, and a prediction kept](worklog/409-the-pthread-family-and-a-prediction-kept.md) |
| - | [410. The tail, and what was decided not to do](worklog/410-the-tail-and-what-was-decided-not-to-do.md) |
| - | [411. The record could not see the work](worklog/411-the-record-could-not-see-the-work.md) |
| - | [412. The whole corpus was twelve days stale](worklog/412-the-whole-corpus-was-twelve-days-stale.md) |
| - | [413. libSceUlt, and the quiet four gigabytes](worklog/413-libsceult-and-the-quiet-four-gigabytes.md) |
| - | [414. The first command packets came back](worklog/414-the-first-command-packets-came-back.md) |
| - | [415. The constructor was the wrong question](worklog/415-the-constructor-was-the-wrong-question.md) |
| - | [416. Seventy-eight percent of every call](worklog/416-seventy-eight-percent-of-every-call.md) |
| - | [417. A placeholder that says who](worklog/417-a-placeholder-that-says-who.md) |
| - | [418. The classification failed, and found two bugs on the way](worklog/418-the-classification-failed-and-found-two-bugs.md) |
| - | [419. Six arguments, and a hypothesis that was wrong](worklog/419-six-arguments-and-a-refuted-hypothesis.md) |
| - | [420. The ring is circular, and the trade was false](worklog/420-the-ring-is-circular.md) |
| - | [421. The futex had a name, and the wait now waits](worklog/421-the-futex-had-a-name-and-the-wait-now-waits.md) |
| - | [422. The stack the collector scans](worklog/422-the-stack-the-collector-scans.md) |
| - | [423. The fault message was guessing, and it cost a finding](worklog/423-the-fault-message-was-guessing.md) |
| - | [424. The protection call learns about the other half of the map](worklog/424-the-protection-call-learns-the-other-half.md) |
| - | [425. What the guest opened, and the two devices it wanted](worklog/425-what-the-guest-opened.md) |
| - | [426. The wall was the crash reporter, and the file path it died on has a name now](worklog/426-the-wall-was-the-crash-reporter.md) |
| - | [427. Reading the command buffer, and the two tools that had to be fixed to do it](worklog/427-reading-the-command-buffer.md) |
| - | [428. The determinism bug had two halves and a third that is architecture](worklog/428-the-determinism-bug-had-two-halves.md) |
| - | [429. Autodebugging starts by not believing the run](worklog/429-autodebugging-starts-by-not-believing-the-run.md) |
| - | [430. The hardware answered, and the loop reads structures by itself](worklog/430-the-hardware-answered-and-the-loop-reads-structures.md) |
| - | [431. The path was globalgamemanagers, and yesterday's finding was wrong](worklog/431-the-path-was-globalgamemanagers.md) |
| - | [432. The bytes were not what it wanted](worklog/432-the-bytes-were-not-what-it-wanted.md) |
| - | [433. The guest was telling us all along](worklog/433-the-guest-was-telling-us-all-along.md) |
| - | [434. The answer was in the title directory](worklog/434-the-answer-was-in-the-title-directory.md) |
| - | [435. The library was guest code all along](worklog/435-the-library-was-guest-code-all-along.md) |
| - | [436. Stepping through the construction](worklog/436-stepping-through-the-construction.md) |
| - | [437. Fifty-eight, not three](worklog/437-fifty-eight-not-three.md) |
| - | [438. Four hundred and two bytes](worklog/438-four-hundred-and-two-bytes.md) |
| - | [439. Opened, never touched](worklog/439-opened-never-touched.md) |
| - | [440. Untouched, not zero](worklog/440-untouched-not-zero.md) |
| - | [441. The other title](worklog/441-the-other-title.md) |
| - | [442. A different program](worklog/442-a-different-program.md) |
| - | [443. Re-derived](worklog/443-re-derived.md) |
| - | [444. One branch](worklog/444-one-branch.md) |
| - | [445. The head of the sequence](worklog/445-the-head-of-the-sequence.md) |
| - | [446. A conflict nobody stepped past](worklog/446-a-conflict-nobody-stepped-past.md) |
| - | [447. The largest record nobody read](worklog/447-the-largest-record-nobody-read.md) |
| - | [448. Five files of twenty-eight](worklog/448-five-files-of-twenty-eight.md) |
| - | [449. The probe runs here too](worklog/449-the-probe-runs-here-too.md) |
| - | [450. The probe was not calling](worklog/450-the-probe-was-not-calling.md) |
| - | [451. The encoding fell out of a pairing](worklog/451-the-encoding-fell-out-of-a-pairing.md) |
| - | [452. The generous window was zero](worklog/452-the-generous-window-was-zero.md) |
| - | [453. The guest was carrying the answer](worklog/453-the-guest-was-carrying-the-answer.md) |
| - | [454. The handle was never consulted](worklog/454-the-handle-was-never-consulted.md) |
| - | [455. The biggest number on the board](worklog/455-the-biggest-number-on-the-board.md) |
| - | [456. Advice that cannot succeed](worklog/456-advice-that-cannot-succeed.md) |
| - | [457. One function, two answers](worklog/457-one-function-two-answers.md) |
| - | [458. The ratchet cannot say worse](worklog/458-the-ratchet-cannot-say-worse.md) |
| - | [459. The branch that printed nothing](worklog/459-the-branch-that-printed-nothing.md) |
| - | [460. A record older than the repository](worklog/460-a-record-older-than-the-repository.md) |
| - | [461. A hypothesis tested and rejected](worklog/461-a-hypothesis-tested-and-rejected.md) |
| - | [462. An answer needs somebody who asked](worklog/462-an-answer-needs-somebody-who-asked.md) |
| - | [463. The eboot was never stuck](worklog/463-the-eboot-was-never-stuck.md) |
| - | [464. Twenty blank pages](worklog/464-twenty-blank-pages.md) |
| - | [465. Every import accounted for](worklog/465-every-import-accounted-for.md) |
| - | [466. Four numbers](worklog/466-four-numbers.md) |
| - | [467. Two tables that answer each other](worklog/467-two-tables-that-answer-each-other.md) |
| - | [468. A third title at the same wall](worklog/468-a-third-title-at-the-same-wall.md) |
| - | [469. The binding reached the relocation](worklog/469-the-binding-reached-the-relocation.md) |
| - | [470. The guest signals itself](worklog/470-the-guest-signals-itself.md) |
| - | [471. The report can see a guest sitting still](worklog/471-the-report-can-see-a-guest-sitting-still.md) |
| - | [472. The canary had a name](worklog/472-the-canary-had-a-name.md) |
| - | [473. Every request came back at once](worklog/473-every-request-came-back-at-once.md) |
| - | [474. The target is not the caller](worklog/474-the-target-is-not-the-caller.md) |
| - | [475. The report learned to name threads](worklog/475-the-report-learned-to-name-threads.md) |
| - | [476. The handler ran](worklog/476-the-handler-ran.md) |
| - | [477. Two readers, one corpus](worklog/477-two-readers-one-corpus.md) |
| - | [478. The differential was measuring itself](worklog/478-the-differential-was-measuring-itself.md) |
| - | [479. The symbol level](worklog/479-the-symbol-level.md) |
| - | [480. FURTHER](worklog/480-further.md) |
| - | [481. Mirrors, and a new wall](worklog/481-mirrors-and-a-new-wall.md) |
| - | [482. The guest was talking all along](worklog/482-the-guest-was-talking-all-along.md) |
| - | [483. The guest is the only oracle left](worklog/483-the-guest-is-the-only-oracle-left.md) |
| - | [484. The corpus has names](worklog/484-the-corpus-has-names.md) |
| - | [485. Three roots](worklog/485-three-roots.md) |
| - | [486. The differential was comparing two sandboxes](worklog/486-the-differential-was-comparing-two-sandboxes.md) |
| - | [487. Prospero, Trinity, Orbis, Neo](worklog/487-prospero-trinity-orbis-neo.md) |
| - | [488. An origin list](worklog/488-an-origin-list.md) |
| - | [489. The index was never mine to edit](worklog/489-the-index-was-never-mine-to-edit.md) |
| - | [490. Orbis, not neo](worklog/490-orbis-not-neo.md) |
| - | [491. The vendor spelling of a socket, and the flag that hung the guest](worklog/491-the-vendor-spelling-of-a-socket.md) |
| - | [492. orbistoun was calling itself a payload](worklog/492-orbistoun-was-calling-itself-a-payload.md) |
| - | [493. The refusal a guest could not see](worklog/493-the-refusal-a-guest-could-not-see.md) |
| - | [494. The pad structure had been measured](worklog/494-the-pad-structure-had-been-measured.md) |
| - | [495. The audio drain was arithmetic](worklog/495-the-audio-drain-was-arithmetic.md) |
| - | [496. A weak symbol orbistoun says exists](worklog/496-a-weak-symbol-orbistoun-says-exists.md) |
| - | [497. The comparison ran on the wrong machine](worklog/497-the-comparison-ran-on-the-wrong-machine.md) |
| - | [498. Weak undefined symbols bind to zero](worklog/498-weak-undefined-symbols-bind-to-zero.md) |
| - | [499. The corpus is GPU-bound, and a real header agreed](worklog/499-the-corpus-is-gpu-bound-and-a-real-header.md) |
| - | [500. The frontier, fully mapped, and the two things that block it](worklog/500-the-frontier-fully-mapped-and-what-blocks-it.md) |
| - | [501. The naming avenue is closed, not untried - and obSCEne's AGC names are already ours](worklog/501-the-naming-avenue-is-closed-not-untried.md) |
| - | [502. The memory-read capture is hardened shut, and obSCEne pivoted to calling the builders](worklog/502-the-memory-read-capture-is-hardened-shut-and-obscene-pivoted.md) |
| - | [503. The wall moves mechanically — and the next one is a kernel struct, not GPU-blocked](worklog/503-the-wall-moves-mechanically-and-the-next-one-is-not-gpu-blocked.md) |
| - | [504. The mapper refuses cold, and the guest wants the fill not the code](worklog/504-the-mapper-refuses-cold-and-the-guest-wants-the-fill.md) |
| - | [505. Measured refusals do not move out-parameter walls — and Earthion gates on the mapper](worklog/505-measured-refusals-dont-move-out-parameter-walls.md) |
| - | [506. The AGC builders run as pure encoders, the decoder holds, and one builder is new](worklog/506-the-agc-builders-run-as-pure-encoders-and-the-decoder-holds.md) |
| - | [507. create-shader succeeded on hardware, and the fill is held for the object model](worklog/507-create-shader-succeeded-on-hardware-and-the-fill-is-held-for-the-object-model.md) |
| - | [508. 3c5e answered: the shader object is guest-adjacent - the cheap case, held ready](worklog/508-3c5e-answered-shader-object-is-guest-adjacent-the-cheap-case.md) |
| - | [509. b7e2 = case (b): the mapper is AGC-linked, and Earthion's chain is one context](worklog/509-the-mapper-is-agc-linked-earthions-chain-is-one-context.md) |
| - | [510. Actioned 4b1a: obSCEne's compute-dispatch stream decodes whole; execution is the gap](worklog/510-actioned-4b1a-the-compute-dispatch-stream-decodes-whole.md) |
| - | [511. e4f1: fixed a transcribed opcode name obSCEne's disassembly caught; linkage HLE held](worklog/511-e4f1-fixed-a-transcribed-opcode-name-linkage-functions-held.md) |
| - | [512. sceAgcCreateShader implemented from the measured model - Earthion moved, for real](worklog/512-sceagccreateshader-implemented-earthion-moved-on-measured-data.md) |
| - | [513. Diagnostic scope: Earthion registers 62 shaders; the mapper is the real gate](worklog/513-diagnostic-scope-earthion-registers-62-shaders-mapper-is-the-real-gate.md) |
| - | [514. 9a41: the shader-linkage functions, Type 0 queue accept, and draw-stream decode](worklog/514-9a41-linkage-functions-queue-accept-and-type0-draw-decode.md) |
| - | [515. 7b3c actioned faithfully; Earthion's abort is a game-engine branch, not a firmware gap](worklog/515-7b3c-actioned-earthion-abort-is-a-game-engine-branch.md) |
| - | [516. The mapper abort branch, disassembled: a hard must-succeed gate reading four filled qwords](worklog/516-the-mapper-abort-branch-disassembled-a-hard-must-succeed-gate.md) |
| - | [517. PPSA02664 past common-dialog to the AGC render-state wall; and the AGC-wiring debt cleared](worklog/517-ppsa02664-past-common-dialog-to-the-agc-render-state-wall.md) |
| - | [518. Testing titles: GTA V's direct-memory anomaly and ASTRO BOT's bundled-module fault](worklog/518-testing-titles-gta-memory-anomaly-and-astro-bot.md) |
| - | [519. The APR slot-layout experiment, and Terminator's real wall (int 0x41, not APR)](worklog/519-the-apr-slot-experiment-and-terminators-real-wall.md) |
| - | [520. GTA V's direct-memory refusal: no allocator bug - a budget gap](worklog/520-gta-direct-memory-no-allocator-bug-a-budget-gap.md) |
| - | [521. The retail frontier is characterized; what remains is deep, not loop-tick work](worklog/521-the-retail-frontier-is-exhausted-at-the-loop-cadence.md) |
| - | [522. Terminator's int 0x41, precisely: a Unity TempOverflow OOM on a stack address used as a size](worklog/522-terminators-int-0x41-is-a-tempoverflow-oom-on-a-garbage-size.md) |
| - | [523. SPIR-V extended-instruction support: the unlock for min/max and the transcendentals](worklog/523-spirv-extended-instruction-support-the-unlock-for-min-max-transcendentals.md) |
| - | [524. v_min_f32/v_max_f32 wired through the extended set, and the fixture toolchain pinned](worklog/524-v-min-max-wired-through-ext-inst-and-the-fixture-toolchain.md) |
| - | [525. The transcendental float expansion plan and the allocator out-parameter trace](worklog/525-the-transcendental-alu-expansion-and-out-parameter-trace-plan.md) |
| - | [526. Unary and transcendental vector float ALU wired through the extended set](worklog/526-unary-transcendental-alu-expansion-landed.md) |
| - | [527. v_sin_f32/v_cos_f32 take revolutions, not radians - correcting worklog 526](worklog/527-v-sin-cos-take-revolutions-not-radians.md) |
| - | [528. Terminator reaches flipped: the TempOverflow was graphics cascade; now unified at AGC render-state](worklog/528-terminator-reaches-flipped-unified-at-agc-render-state.md) |
| - | [529. AGC render-state sub-object model cracked: exact correspondence to obSCEne 0x2c measurement](worklog/529-agc-render-state-sub-object-model-cracked.md) |
| - | [530. The shader-capture pipeline: from a command stream to the census corpus](worklog/530-shader-capture-pipeline-command-stream-to-census-corpus.md) |
| - | [531. The DCB-writer-handle request, and a second witness for the packet walker](worklog/531-dcb-writer-handle-request-and-a-second-packet-witness.md) |
| - | [532. PPSA02664's flip-wait is a symptom; the wall is the Dcb builders (FLIP_TO_ALL confirms)](worklog/532-the-flip-wait-is-a-symptom-the-wall-is-the-dcb-builders.md) |
| - | [533. Fourteen AGC packet sizes measured, and what a rival's table agreed and disagreed with](worklog/533-measured-agc-packet-sizes-and-the-prosper-cross-check.md) |
| - | [534. Ten AGC builders measured as whole packets, and a specific prosper claim refuted](worklog/534-ten-measured-pm4-packets-and-a-refuted-claim.md) |
| - | [535. The GL cube renders on the GPU: five measured fixes in oops-gl's AGC path](worklog/535-the-gl-cube-renders-on-the-gpu-five-measured-fixes.md) |
| - | [536. The ten measured packets, implemented as encoders - and why they are not yet wired](worklog/536-the-ten-measured-packets-implemented.md) |
| - | [537. The worklog numbering race, closed](worklog/537-the-worklog-numbering-race.md) |
| - | [538. The DCB writer handle, and eight builders wired through it](worklog/538-the-dcb-writer-handle-and-eight.md) |
| - | [539. The GPU badge is a measurement, the frame hashes are stable, and textures sample](worklog/539-the-gpu-badge-is-a-measurement-the.md) |
| - | [540. The builder return is a pointer, and the run that showed it](worklog/540-the-builder-return-is-a-pointer-and-the.md) |
| - | [541. Prior-art audit: four public PS5 AGC repositories against the GL cube's measurements](worklog/541-prior-art-audit-four-public-ps5-agc.md) |
| - | [542. The corpus's hottest import is a probe artefact, and `worklist` cannot say so](worklog/542-the-corpus-hottest-import-is-a-probe.md) |
| - | [543. Syscall 1 bound to nothing, because a rename named nothing](worklog/543-syscall-1-bound-to-nothing-because-a.md) |
| - | [544. The `Exited` rung, and where it had to sit](worklog/544-the-exited-rung-and-where-it-had-to.md) |
| - | [545. The GL cube oracle through the submission pipeline: m0 lands, five names and one base are missing](worklog/545-the-gl-cube-oracle-through-the.md) |
| - | [546. Exited beats faulted, and the verdict that proved it](worklog/546-exited-beats-faulted-and-the-verdict.md) |
| - | [547. Records update on not-worse-and-different, and three wrong diagnoses](worklog/547-records-update-on-not-worse-and.md) |
| - | [548. The five the vertex program needed: named by the reference, four translated, one blocked](worklog/548-the-five-the-vertex-program-needed.md) |
| - | [549. The image family named and solved, and a mask that solved one bit too wide](worklog/549-the-image-family-named-and-solved-and-a.md) |
| - | [550. The census stopped claiming shaders complete that do not translate](worklog/550-the-census-stopped-claiming-shaders.md) |
| - | [551. The parameter move was blocked on the wrong obstacle](worklog/551-the-parameter-move-was-blocked-on-the.md) |
| - | [552. A console shader translates: the no-base code was wrong and the stage was never passed](worklog/552-a-console-shader-translates-the-no-base.md) |
| - | [553. The AGC patch family is one producer and its amendments](worklog/553-the-agc-patch-family-is-a-producer-and.md) |
| - | [554. The console's pixel shader draws, and five validation errors nobody was watching](worklog/554-the-console-s-pixel-shader-draws-and.md) |
| - | [555. Terminator's recorded flip does not reproduce, and it is not a regression](worklog/555-terminator-s-recorded-flip-does-not.md) |
| - | [556. Modules declare the sixteen-bit capabilities only where they use them](worklog/556-modules-declare-the-sixteen-bit.md) |
| - | [557. A mesh stage stands up, hand-assembled, before anything is translated into it](worklog/557-a-mesh-stage-stands-up-hand-assembled.md) |
| - | [558. The console's primitive shader translates into a mesh module](worklog/558-the-console-s-primitive-shader.md) |
| - | [559. The translated mesh module validates, draws nothing, and the window says why not](worklog/559-the-translated-mesh-module-validates.md) |
| - | [560. A minimal translated primitive shader draws, so the fault is the console's own](worklog/560-a-minimal-translated-primitive-shader.md) |
| - | [561. The window has no base, so a guest address reads nothing](worklog/561-the-window-has-no-base-so-a-guest.md) |
| - | [562. None of the cheap missing imports are walls](worklog/562-none-of-the-cheap-missing-imports-are.md) |
| - | [563. The window's base draws the geometry, and the wall moves to the colour parameter](worklog/563-the-window-s-base-draws-the-geometry.md) |
| - | [564. Two builders wired, six refused, and the rule that decides which](worklog/564-two-builders-wired-six-refused-and-the.md) |
| - | [565. A console's two shaders draw together, and a silently dropped offset is why they could not](worklog/565-a-console-s-two-shaders-draw-together.md) |
| - | [566. A texture reaches a device, and the level a guest asks for turns out to be a different instruction](worklog/566-a-texture-reaches-a-device-and-the.md) |
| - | [567. Three gates were red, and the one that had gone blind had shipped what it exists to stop](worklog/567-three-gates-were-red-and-the-one-that.md) |
| - | [568. The console's textured pixel shader translates, and the blocked list is empty again](worklog/568-the-console-s-textured-pixel-shader.md) |
| - | [569. a window carries its own length](worklog/569-a-window-carries-its-own.md) |
| - | [570. A guest's shader changes guest memory, and one span had to cover both buffers](worklog/570-a-guest-s-shader-changes-guest-memory.md) |
| - | [571. The lane model ignored the window it was handed, and nothing said so](worklog/571-the-lane-model-ignored-the-window-it.md) |
| - | [572. The plain sampling form needed nothing new, and the census said so](worklog/572-the-plain-sampling-form-needed-nothing.md) |
| - | [573. A texel fetch needed no binding of its own, and the store still does](worklog/573-a-texel-fetch-needed-no-binding-of-its.md) |
| - | [574. Grand Theft Auto asks for more memory than the console has](worklog/574-grand-theft-auto-asks-for-more-memory.md) |
| - | [575. The last image instruction that needed no measurement, and the format it refuses to invent](worklog/575-the-last-image-instruction-that-needed.md) |
| - | [576. The dimensionality was in the instruction all along, and nothing was reading it](worklog/576-the-dimensionality-was-in-the.md) |
| - | [577. An image instruction is not always eight bytes, and the decoder believed it was](worklog/577-an-image-instruction-is-not-always.md) |
| - | [578. Eleven of fourteen was one gap and two refusals, and the tool could not say which](worklog/578-eleven-of-fourteen-was-one-gap-and-two.md) |
| - | [579. A flag written nowhere was refused, and `null` means less rather than unsupported](worklog/579-a-flag-written-nowhere-was-refused-and.md) |
| - | [580. The level operand was unobservable until the texture had two of them](worklog/580-the-level-operand-was-unobservable.md) |
| - | [581. The shader the image subsystem was built for draws, and it reads two attributes](worklog/581-the-shader-the-image-subsystem-was.md) |
| - | [582. A gate for the newline a formatter never had to collapse](worklog/582-a-gate-for-the-newline-a-formatter.md) |
| - | [583. Record B's frame path runs end to end, with nothing hand-written in it](worklog/583-record-b-s-frame-path-runs-end-to-end.md) |
| - | [584. A captured command stream draws a frame, which is the last join in the GPU path](worklog/584-a-captured-command-stream-draws-a-frame.md) |
| - | [585. The stream says three vertices and the shader says three, from opposite ends](worklog/585-the-stream-says-three-vertices-and-the.md) |
| - | [586. The scaled kinds are the normalised ones with the division removed](worklog/586-the-scaled-kinds-are-the-normalised.md) |
| - | [587. A packed element wider than a word, and the straddle nobody has to guess at](worklog/587-a-packed-element-wider-than-a-word-and.md) |
| - | [588. The family was five of eight and nothing said so](worklog/588-the-family-was-five-of-eight-and.md) |
| - | [589. The AGC surface is sixteen builders, not nothing](worklog/589-the-agc-surface-is-sixteen-not.md) |
| - | [590. A rung the corpus cannot reach](worklog/590-a-rung-the-corpus-cannot.md) |
| - | [591. Stuck and working no longer share an outcome](worklog/591-stuck-and-working-no-longer-share-an.md) |
| - | [592. The deferred cost of a child process comes due, and is cheaper than it was booked at](worklog/592-the-deferred-cost-of-a-child-process.md) |
| - | [593. Load-time initialisation is ruled out, and the wall it was asked about is gone](worklog/593-load-time-initialisation-is-ruled-out.md) |
| - | [594. A fault in host code gets a name that survives a reboot](worklog/594-a-fault-in-host-code-gets-a-name-that.md) |
| - | [595. The cited C++ ABI name list buys nothing, measured three ways](worklog/595-the-cited-c-abi-name-list-buys-nothing.md) |
| - | [596. Surface layout gets a roadmap entry, and half of it was never blocked](worklog/596-surface-layout-gets-a-roadmap-entry-and.md) |
| - | [597. Step 8 measured: more clock buys nothing, and input is one hardware run away](worklog/597-step-8-measured-more-clock-buys-nothing.md) |
| - | [598. Step 9 is unreachable: nothing in the corpus calls any of the four](worklog/598-step-9-is-unreachable-nothing-in-the.md) |
| - | [599. `statfs` answers a real measurement, and the netctl loop was neither](worklog/599-statfs-answers-a-real-measurement-and.md) |
| - | [600. The patch-family producer is wired, and the retail wall is a cluster](worklog/600-the-patch-family-producer-is-wired-and.md) |
| - | [601. The retail direct-memory pool is twelve gibibytes, and it moves Grand Theft Auto](worklog/601-the-retail-direct-memory-pool-is-twelve.md) |
| - | [602. The vertex buffer was never in the register stream, and one resolution was a printed table](worklog/602-the-vertex-buffer-was-never-in-the.md) |
| - | [603. Grand Theft Auto's ceiling is an int 0x41 trap after a path it cannot resolve](worklog/603-grand-theft-auto-s-ceiling-is-an-int.md) |
| - | [604. A software interrupt names its own vector, and says the gap is ours](worklog/604-a-software-interrupt-names-its-own.md) |
| - | [605. The fault taxonomy gains a kernel-entry class, and it names itself in the finding](worklog/605-the-fault-taxonomy-gains-a-kernel-entry.md) |
| - | [606. The null-deref finding routes to the call that answered the pointer - as a lead, not a verdict](worklog/606-the-null-deref-finding-routes-to-the.md) |
| - | [607. Two more measured AGC builders wired; the retail wall is now purely the patch family](worklog/607-two-more-measured-agc-builders-wired.md) |
| - | [608. The interrupt-dispatch mechanism, ready for the `int 0x41` handler the moment it lands](worklog/608-the-interrupt-dispatch-mechanism-ready.md) |
| - | [609. The scriptingGetMem "wall" is not a wall; Terminator dies at `int 0x41`, like Grand Theft Auto](worklog/609-the-scriptinggetmem-wall-is-not-a-wall.md) |
| - | [610. The give-up finding names the call the guest gated on, so the reader stops blaming the far one](worklog/610-the-give-up-finding-names-its-own-gate.md) |
| - | [611. `int 0x41` is measured: a fatal trap, not a kernel entry to implement](worklog/611-int-0x41-is-a-measured-fatal-trap-not-a-kernel-entry.md) |
| - | [612. Three more AGC builders wired from the a70f sweep, and why only three](worklog/612-three-more-agc-builders-wired-from-the-a70f-sweep.md) |
| - | [613. The NGG split is measured, and the split obSCEne stated is refuted by its own draw](worklog/613-the-ngg-split-is-measured-and-the-split.md) |
| - | [614. The Cx-indirect patch answers its measured 0x0, and two Unity titles reach 222 imports](worklog/614-the-cx-indirect-patch-returns-its-measured-0x0.md) |
| - | [615. Opening a directory returned ENOENT on Windows, and it walled Grand Theft Auto at `int 0x41`](worklog/615-opening-a-directory-failed-on-windows-and-walled-gta.md) |
| - | [616. The whole `sceAgc*Patch*` family answers its measured 0x0, and the wall is now the builders](worklog/616-the-whole-agc-patch-family-answers-its-measured-0x0.md) |
| - | [617. The untyped buffer family was two of eight, found the same way MTBUF was](worklog/617-the-untyped-buffer-family-was-two-of.md) |
| - | [618. The a70f builder cluster wired, and the proof that PPSA02664's wall is not the placeholders](worklog/618-the-a70f-builder-cluster-wired-and-what-it-ruled-out.md) |
| - | [619. The markers and WaitRegMem close the builder cluster, and going BACK confirms where the wall is](worklog/619-the-markers-and-waitregmem-close-the-cluster-and-confirm-the-wall.md) |
| - | [620. Both `int 0x41` titles are il2cpp assertions, and the gate file opens are faithful - so the wall is a Unity invariant, not a value to name](worklog/620-both-int-0x41-titles-are-il2cpp-assertions-not-the-file-opens.md) |
| - | [621. A null-ish fault in orbistoun's own code is the guest's libc pointer, not an "EMULATOR BUG"](worklog/621-a-null-ish-fault-in-orbistouns-code-is-the-guests-libc-pointer.md) |
| - | [622. The tables gate caught the stale probe already and said the wrong thing about it](worklog/622-the-tables-gate-caught-the-stale-probe.md) |
| - | [623. The four libraries in the stall finding are called zero times, and one excuse had rotted](worklog/623-the-four-libraries-in-the-stall-finding.md) |
| - | [624. Case-sensitivity was a host leak, and why having the FreeBSD oracle did not prevent it](worklog/624-case-sensitivity-a-host-leak-and-why-the-oracle-was-not-the-gap.md) |
| - | [625. Inferring import signatures from how the guest calls its own imports](worklog/625-inferring-import-signatures-from-how-the-guest-calls-them.md) |
| - | [626. The fs host-leak audit, measured: two exotic leaks reproduce, and none is on the `0x41` titles' path](worklog/626-the-fs-host-leak-audit-is-measured-and-mostly-negative.md) |
| - | [627. Nine decisions were written and never indexed, and the generator was one directory up](worklog/627-nine-decisions-were-written-and-never.md) |
| - | [628. Indexed draws are decoded from their own measured body, not dropped for "separate state"](worklog/628-indexed-draws-are-decoded-from-their-own-body.md) |
| - | [629. strftime gains the twelve-hour clock, the weekday numbers, and the C-locale forms](worklog/629-strftime-gains-the-twelve-hour-and-c-locale-specifiers.md) |
| - | [630. The render executor's blocker is precise: the backend trait cannot reach a shader's bytes](worklog/630-the-render-executor-is-blocked-on-a-module-delivery-mechanism.md) |
| - | [631. The resource-residency seam, and its shader arm: a translated module now loads on a real device](worklog/631-the-resource-residency-seam-and-its-shader-arm.md) |
| - | [632. The thin driver, and the first command that actually executes: a compute dispatch runs through the backend](worklog/632-the-thin-driver-and-the-first-executed-command.md) |
| - | [633. A compute dispatch runs against a resident guest buffer, and the throwaway width is gone for it](worklog/633-a-compute-dispatch-runs-against-a-resident-guest-buffer.md) |
| - | [634. The V# decoder: the first piece of the frontend's guest-buffer decode](worklog/634-the-v-sharp-decoder-the-first-piece-of-the-frontend-buffer-decode.md) |
| - | [635. Compute dispatches are decoded, and what the translator's memory model means for the V# work](worklog/635-compute-dispatches-are-decoded-and-the-frontend-buffer-model-is-a-window.md) |
| - | [636. The first graphics frame through the executor: a bound vertex+fragment pipeline draws](worklog/636-the-first-graphics-frame-through-the-executor.md) |
| - | [637. The colour target's dimensions are decoded from CB_COLOR0_ATTRIB2](worklog/637-the-colour-target-dimensions-are-decoded-from-cb-color0-attrib2.md) |
| - | [638. A draw renders into the guest's target size, not the interim square](worklog/638-a-draw-renders-into-the-guest-s-target-size.md) |
| - | [639. A draw issues the guest's decoded vertex count, not a fixed three](worklog/639-a-draw-issues-the-guest-s-vertex-count.md) |
| - | [640. The executor draws a guest's mesh geometry, routed by the module not the command](worklog/640-the-executor-draws-a-guest-s-mesh-geometry.md) |
| - | [641. The guest-memory window is fed from guest memory through the executor](worklog/641-the-guest-memory-window-is-fed-through-the-executor.md) |
| - | [642. The image-descriptor (T#) decode: the first, measured piece of the texture path](worklog/642-the-image-descriptor-decode-the-first-piece-of-the-texture-path.md) |
| - | [643. Indexed draws execute through the mesh path, refused only on a vertex pipeline](worklog/643-indexed-draws-execute-through-the-mesh-path.md) |
| - | [644. SetViewport restricts a draw to its rectangle - the backend half of the viewport](worklog/644-set-viewport-restricts-a-draw-to-its-rectangle.md) |
| - | [645. The guest-memory window is uploaded once and bound directly, not seeded per draw](worklog/645-the-guest-memory-window-is-bound-once-not-seeded-per-draw.md) |
| - | [646. The viewport frontend decode: a stream's scissor reaches the backend as a SetViewport](worklog/646-the-viewport-frontend-decode-closes-the-viewport-path.md) |
| - | [647. A compute dispatch reads back guest memory, where a guest's result lives](worklog/647-a-compute-dispatch-reads-back-guest-memory.md) |
| - | [648. A whole frame composes through the driver: target, viewport, shaders, draw](worklog/648-a-whole-frame-composes-through-the-driver.md) |
| - | [649. The texture detile is blocked on a disputed swizzle - do not implement it yet](worklog/649-the-texture-detile-is-blocked-on-a-disputed-swizzle.md) |
| - | [650. The packed floats decode, and finding out cost a latent width-order bug](worklog/650-the-packed-floats-decode-and-finding.md) |
| - | [651. The scripted pad exists, and it deliberately stops short of the byte it would be written into](worklog/651-the-scripted-pad-exists-and-it.md) |
| - | [652. The emulator settles the swizzle dispute but does not measure it - the detile stays blocked](worklog/652-the-emulator-settles-the-swizzle-dispute-but-does-not-measure-it.md) |
| - | [653. The texture detile lands, anchored on obSCEne's measured texel (15,15)](worklog/653-the-texture-detile-lands-anchored-on-obscenes-measured-texel.md) |
| - | [654. A capture correlates each draw with the shaders live at it](worklog/654-a-capture-correlates-each-draw-with-the-shaders-live-at-it.md) |
| - | [655. The colour target decode pairs its base and extent - the piece the detile needs](worklog/655-the-colour-target-decode-pairs-its-base-and-extent.md) |
| - | [656. The colour write mask decodes; the tiling-mode citation turned out not to be solid](worklog/656-the-colour-write-mask-decodes-and-the-tiling-mode-citation-is-not-solid.md) |
| - | [657. The tiling mode decodes - the citation held up, and the colour target is now fully described](worklog/657-the-tiling-mode-decodes-and-the-colour-target-is-now-fully-described.md) |
| - | [658. The detile gets its consumer: a colour target out of guest memory](worklog/658-the-detile-gets-its-consumer-a-colour-target-out-of-guest-memory.md) |
| - | [659. A texture declares its own tiling: the T# swizzle mode decodes](worklog/659-a-texture-declares-its-own-tiling-the-t-sharp-swizzle-mode-decodes.md) |
| - | [660. detile_texture completes the texture detile side, sharing a core with the colour target](worklog/660-detile-texture-completes-the-texture-detile-side.md) |
| - | [661. The packed float store lands, and its bits come back from the device](worklog/661-the-packed-float-store-lands-and-runs-on-the-device.md) |
| - | [662. The AGC shader container note points at SELFish's clean-room home](worklog/662-the-agc-shader-container-note-points-at-selfishs-clean-room-home.md) |
| - | [663. The AGC builder doc called two wired encoders "refused" - corrected, and 4059 closes](worklog/663-the-agc-builder-doc-called-two-wired-encoders-refused.md) |
| - | [664. The prose gate caught two `\`-continued strings I introduced in tiling.rs](worklog/664-the-prose-gate-caught-two-offenders-i-introduced-in-tiling.md) |
| - | [665. Two corpus-reached AGC builders `-4059` missed: `ResetQueue` wired, `WaitUntilSafeForRendering` refused](worklog/665-two-corpus-reached-agc-builders-one-wired-one-refused.md) |
| - | [666. `pthread_create` honours the measured thread-attribute block](worklog/666-pthread-create-honours-the-measured-thread-attribute-block.md) |
| - | [667. The texture detile refuses a non-32-bpp format instead of mis-tiling it](worklog/667-the-texture-detile-refuses-a-non-32-bpp-format.md) |
| - | [668. `sceAgcDcbWaitUntilSafeForRendering` is wired as a measured no-op, reversing worklog 665's refusal](worklog/668-wait-until-safe-for-rendering-is-wired-as-a-measured-no-op.md) |
| - | [669. `DB_DEPTH_CONTROL` decodes the depth- and stencil-test state](worklog/669-db-depth-control-decodes-the-depth-and-stencil-test-state.md) |
| - | [670. `DB_STENCIL_CONTROL` completes the depth-stencil test/op state](worklog/670-db-stencil-control-completes-the-depth-stencil-state.md) |
| - | [671. `CB_BLEND0_CONTROL` decodes the blend state, completing 6e78's decode side](worklog/671-cb-blend0-control-completes-6e78s-decode-side.md) |
| - | [672. The leading titles' frontier after the AGC fix: a null object in command-buffer build](worklog/672-the-leading-titles-frontier-after-the-agc-fix-a-null-object-in-command-build.md) |
| - | [673. `sceAppContentTemporaryDataMount2` answered, and a PPSA25872 regression to flag](worklog/673-app-content-temporary-data-mount2-and-a-ppsa25872-regression-flag.md) |
| - | [674. The detile equation is hardware-validated on two points and a full-block bijection](worklog/674-the-detile-equation-is-hardware-validated-on-two-points-and-a-full-block-bijection.md) |
| - | [675. The `Submission` carries the pipeline state its decodes produced](worklog/675-the-submission-carries-the-pipeline-state-its-decodes-produced.md) |
| - | [676. The phase-6 roadmap no longer tells sessions to wait for arrived measurements](worklog/676-the-phase-6-roadmap-no-longer-tells-sessions-to-wait-for-arrived-measurements.md) |
| - | [677. A vendor-library capture joins the packet-vocabulary check](worklog/677-a-vendor-library-capture-joins-the-packet-vocabulary-check.md) |
| - | [678. `sceVideoOutRegisterBuffers` keeps the addresses a frame lives at](worklog/678-video-out-keeps-the-buffer-addresses-a-frame-lives-at.md) |
| - | [679. The non-existent `sceAgcSubmitDcb` is corrected in shipped help text](worklog/679-the-non-existent-sceagcsubmitdcb-is-corrected-in-shipped-help-text.md) |
| - | [680. Two "it is empty" comments corrected, and two implementations that had no knowledge entry](worklog/680-two-self-refuting-empty-comments-and-two-unrecorded-implementations.md) |
| - | [681. The published rung legend names all seven rungs, and a flip counter stops calling itself presented](worklog/681-the-rung-legend-names-seven-and-a-flip-counter-stops-claiming-a-picture.md) |
| - | [682. `compat markdown --check` — the two generated docs `status --check` never covered](worklog/682-compat-markdown-gains-a-check-arm-so-a-hand-edit-cannot-hide.md) |
| - | [683. Eight doc claims corrected against the code they describe](worklog/683-eight-doc-claims-that-understated-the-code.md) |
| - | [684. The two leading titles re-run under the current build — the wall holds, the answered count climbs](worklog/684-the-two-leading-titles-re-run-under-the-current-build.md) |
| - | [685. The 166-agc draw and state packets decode, and an indexed-draw stream now walks in a test](worklog/685-the-166-agc-draw-and-state-packets-decode-and-an-indexed-draw-stream-walks.md) |
| - | [686. The primitive-draw oracle is in the tree, and tiling.rs now detiles a whole console frame](worklog/686-the-primitive-draw-oracle-brought-in-and-detiled-against-a-console-frame.md) |
| - | [687. The handover: `sceAgcDriverSubmitDcb` reads a guest command buffer, walks it, and reports](worklog/687-the-handover-a-guest-command-buffer-is-read-walked-and-reported.md) |
| - | [688. The submission report reaches the run report](worklog/688-the-submission-report-reaches-the-run-report.md) |
| - | [689. What completes driver work, decided: nothing yet, and the report now says why](worklog/689-what-completes-driver-work-decided-nothing-yet-and-the-report-says-why.md) |
| - | [690. The detiling module, and why it models one mode and refuses the rest](worklog/690-the-detiling-module-and-why-it-models-one-mode.md) |
| - | [691. The detile's provenance, corrected: one measured byte, not two points and a whole frame](worklog/691-the-detile-provenance-corrected-one-measured-byte-not-two-points-and-a-frame.md) |
| - | [692. The handover resolves shader addresses against the guest's regions, and bounds-checks the descriptor](worklog/692-the-handover-resolves-shader-addresses-against-the-guests-regions.md) |
| - | [693. Five statements the code beside them contradicted](worklog/693-five-statements-the-code-contradicted.md) |
| - | [694. Seven stale facts corrected, and the flip builder's measurement recorded](worklog/694-seven-stale-facts-and-a-flip-builder-recorded.md) |
| - | [695. Video-out: the two undeclared calls named, the attribute kept, the flipped buffer readable](worklog/695-video-out-attribute-kept-and-flipped-buffer-readable.md) |
| - | [696. The presented rung gets its arm, awarded by a framebuffer readback](worklog/696-presented-rung-awarded-by-a-framebuffer-readback.md) |
| - | [697. The doc gate was red behind an earlier one, on stale intra-doc links](worklog/697-the-doc-gate-was-red-behind-an-earlier-one.md) |
| - | [698. The findings engine recognises the placeholder D670 moved](worklog/698-the-findings-engine-recognises-the-post-d670-placeholder.md) |
| - | [699. The frame crossing D695 decided gets built](worklog/699-the-frame-crossing-d695-decided-gets-built.md) |
| - | [700. The triangle record, its target and both shaders, brought into the tree](worklog/700-the-triangle-record-and-its-shaders-brought-in.md) |
| - | [701. The console's triangle, rendered through the backend and compared with its own frame](worklog/701-the-console-triangle-rendered-and-compared.md) |
| - | [702. The primitive topology, decoded and carried - and where the render gap really is](worklog/702-the-primitive-topology-decoded-and-where-the-render-gap-really-is.md) |
| - | [703. The compatibility scale now sees a missing picture](worklog/703-the-compat-scale-now-sees-a-missing-picture.md) |
| - | [704. Run records name the build that made them, and the stamp follows commits](worklog/704-run-records-name-their-build-and-the-stamp-follows-commits.md) |
| - | [705. The SRT provenance seam its premise asks for is contradicted by measurement](worklog/705-the-srt-provenance-seam-is-contradicted-by-measurement.md) |
| - | [706. The Escape Hatch: the four conditions an autonomous loop halts itself on](worklog/706-the-escape-hatch-the-loop-halts-itself-on.md) |
| - | [707. The worker's host-only code no longer reads as dead off Windows](worklog/707-the-worker-host-only-code-no-longer-reads-as-dead-off-windows.md) |
| - | [708. CI tests macOS as x86_64-apple-darwin, so the sysv64 wall stops walling it](worklog/708-ci-tests-macos-as-x86-64-not-aarch64.md) |
| - | [709. Three unused dependencies removed, the machete lint job green](worklog/709-three-unused-dependencies-removed-machete-green.md) |
| - | [710. The red CI is three root causes, not four - confirmed against the run logs](worklog/710-the-ci-failures-are-three-roots-not-four-confirmed-against-the-logs.md) |
| - | [711. The first green push was incomplete - two more roots the run surfaced](worklog/711-the-first-green-push-was-incomplete-two-more-roots-from-the-run.md) |
| - | [712. The mesh output assembles the primitive the stream set, not always a triangle](worklog/712-the-mesh-output-assembles-the-primitive-the-stream-set.md) |
| - | [713. The point record renders pixel-exact as a point, and the assumption is now measured](worklog/713-the-point-record-renders-pixel-exact-as-a-point.md) |
| - | [714. A real submission reaches a constructed backend on the run path](worklog/714-a-real-submission-reaches-a-constructed-backend-on-the-run-path.md) |
| - | [715. The rendered frame crosses the 7f1b route, end to end](worklog/715-the-rendered-frame-crosses-the-7f1b-route.md) |
| - | [716. PPSA02664, hardened to a sharper diagnosis: the null is not the import's return](worklog/716-ppsa02664-hardened-to-a-sharper-diagnosis-the-null-is-not-the-imports-return.md) |
| - | [717. The NID mining method is validated, and PPSA02664's producer stays unnamed everywhere reachable](worklog/717-the-nid-mining-method-is-validated-and-ppsa02664s-producer-is-unnamed-everywhere-reachable.md) |
| - | [718. The scripted-input route reaches a run, and a played script is counted](worklog/718-the-scripted-input-route-reaches-a-run-and-is-counted.md) |
| - | [719. PPSA02664's wall is external data, confirmed against obSCEne's delivered measurements](worklog/719-ppsa02664s-wall-is-external-data-confirmed-against-obscenes-delivered-measurements.md) |
| - | [720. PPSA02664's CreateWorkload: TODO marker in the guest, but the title runs on hardware](worklog/720-ppsa02664s-createworkload-is-todo-marked-in-the-titles-own-code.md) |
| - | [721. A wall is orbistoun's until hardware proves it the title's](worklog/721-a-wall-is-orbistouns-until-hardware-proves-it-the-titles.md) |
| - | [722. The candidate-causes list pays off; return-forcing the graphics stubs is ruled out](worklog/722-the-candidate-causes-list-pays-off-return-forcing-ruled-out.md) |
| - | [723. Six for six: PPSA02664's CreateWorkload path is all out-of-line SDK inlines](worklog/723-six-for-six-the-createworkload-path-is-all-out-of-line-sdk-inlines.md) |
| - | [724. A guest-memory peek at a fault, and what it immediately found](worklog/724-a-guest-memory-peek-at-a-fault-and-what-it-immediately-found.md) |
| - | [725. Tracing the null container: the phantom is a GetSize, not the producer](worklog/725-tracing-the-null-container-the-phantom-is-a-getsize-not-the-producer.md) |
| - | [726. Ingesting a6aa: the indirect-register packet is measured in shape, not yet in body](worklog/726-ingesting-a6aa-the-indirect-register-packet-is-measured-in-shape-not-yet-in-body.md) |
| - | [727. Serving the klog syscall, and fixing the census that misattributed it](worklog/727-serving-the-klog-syscall-and-fixing-the-census-that-misattributed-it.md) |
| - | [728. Correcting a dangling citation and an over-claimed phantom size](worklog/728-correcting-a-fabricated-citation-and-an-over-claimed-phantom-size.md) |
| - | [729. The null-slot theory, tested and disproven: the phantom must stay callable](worklog/729-the-null-slot-theory-tested-and-disproven-the-phantom-must-stay-callable.md) |
| - | [730. The fault report learns to read heap objects, and the null container is a zeroed allocation](worklog/730-the-fault-report-learns-to-read-heap-objects-and-the-null-container-is-a-zeroed-allocation.md) |
| - | [731. Mapping the CreateWorkload path: AGC-driver calls ruled out, sceAgcInit is load-bearing](worklog/731-mapping-the-createworkload-path-agc-driver-calls-ruled-out-sceagcinit-is-load-bearing.md) |
| - | [732. The flagged AGC-driver setters: verified unimplementable as 0x0, held for a3f0](worklog/732-the-flagged-agc-driver-setters-verified-unimplementable-as-0x0-held-for-a3f0.md) |
| - | [733. The run report stops sending handled calls to a vocabulary search](worklog/733-the-run-report-stops-sending-handled-calls-to-a-vocabulary-search.md) |
| - | [734. libSceAmpr ruled out as the wall cause: the apr path is not the graphics path](worklog/734-libsceampr-ruled-out-as-the-wall-cause-the-apr-path-is-not-the-graphics-path.md) |
| - | [735. authoritative measurement of sceagcinit clears a3f0 and confirms createworkload wall](worklog/735-authoritative-measurement-of-sceagcinit.md) |
| - | [736. Verifying the concurrent sceAgcInit work, and clearing the gate it left red](worklog/736-verifying-the-concurrent-sceagcinit-work-and-clearing-the-gate-it-left-red.md) |
| - | [737. The caller of 0x42d90 found: the memcpy is gated, and the container field is at +0x3c8](worklog/737-the-caller-of-0x42d90-found-the-memcpy-is-gated-and-the-container-field-is-at-0x3c8.md) |
| - | [738. The null container is a descriptor data pointer, and 0x42d90 is a reusable copier](worklog/738-the-null-container-is-a-descriptor-data-pointer-and-0x42d90-is-a-reusable-copier.md) |
| - | [739. The descriptor comes from a collection, and the copy pattern repeats](worklog/739-the-descriptor-comes-from-a-collection-and-the-copy-pattern-repeats.md) |
| - | [740. The fault source is a null-base offset, not a count — and the trace now shows copy operands](worklog/740-the-fault-source-is-a-null-base-offset-not-a-count-and-the-trace-now-shows-copy-operands.md) |
| - | [741. A copy from a null source is now a named fault finding, not just a trace operand](worklog/741-a-copy-from-a-null-source-is-now-a-named-fault-finding-not-just-a-trace-operand.md) |
| - | [742. The improved report shows the register-group loop, and clears the AGC patch call of the null](worklog/742-the-improved-report-shows-the-register-group-loop-and-clears-the-agc-patch-call-of-the-null.md) |
| - | [743. An indirect peek breaks the ASLR barrier, and reads the fault's heap objects](worklog/743-an-indirect-peek-breaks-the-aslr-barrier-and-reads-the-fault-s-heap-objects.md) |
| - | [744. The descriptor access pattern, confirmed from live bytes — and the limit of hand-decode](worklog/744-the-descriptor-access-pattern-confirmed-from-live-bytes-and-the-limit-of-hand-decode.md) |
| - | [745. A disassembler corrects the wall: the src is a stored `0xa8`, not a null-container read](worklog/745-a-disassembler-corrects-the-wall-the-src-is-a-stored-0xa8-not-a-null-container-read.md) |
| - | [746. The phantom is a DMA-packet sizer, ruled out as the fault's source](worklog/746-the-phantom-is-a-dma-packet-sizer-ruled-out-as-the-faults-source.md) |
| - | [747. The faulting descriptor is a "1234" shader header with two unrelocated fields](worklog/747-the-faulting-descriptor-is-a-1234-shader-header-with-two-raw-relative-fields.md) |
| - | [748. The wall is a relocation-only bug — the group 0 register data is present, only its pointer is raw](worklog/748-the-wall-is-a-relocation-only-bug-the-group-0-register-data-is-present.md) |
| - | [749. Two routes to the relocation loop close, pointing at the shader asset](worklog/749-two-routes-to-the-relocation-loop-close-pointing-at-the-shader-asset.md) |
| - | [750. Consolidating the wall — a Unity shader relocation short by two, and what is left to do](worklog/750-consolidating-the-wall-a-unity-shader-relocation-short-by-two-and-what-is-left.md) |
| - | [751. create_shader relocates the full group array under the guard](worklog/751-create-shader-relocates-the-full-group-array-under-the-guard.md) |
| - | [752. The probe question that checks the create_shader fix](worklog/752-the-probe-question-that-checks-the-create-shader-fix.md) |
| - | [753. The title is heavily threaded, so the determinism path closes — and an oracle opens](worklog/753-the-title-is-heavily-threaded-so-determinism-closes-and-an-oracle-opens.md) |
| - | [754. The oracle consulted — a related but unsolved Unity shader fault, and what it is worth](worklog/754-the-oracle-consulted-a-related-but-unsolved-unity-shader-fault.md) |
| - | [755. The phantom's return gates the register-group path, and confirms the relocation wall](worklog/755-the-phantom-return-gates-the-register-group-path-and-confirms-the-relocation-wall.md) |
| - | [756. Error-dialog init is served, and the stale surfaces are regenerated](worklog/756-error-dialog-init-is-served-and-the-stale-surfaces-are-regenerated.md) |
| - | [757. Two system-service actions are served, and a shared-static test race is closed](worklog/757-two-system-service-actions-are-served-and-a-shared-static-test-race-is-closed.md) |
| - | [758. GTAV's rpf.cache was misrouted, not missing - serving it clears the int 0x41 wall](worklog/758-gtav-s-rpf-cache-was-misrouted-not-missing-and-serving-it-moves-the-wall.md) |
| - | [759. GTA's next wall is a null-vtable virtual call, not the mutex the report guessed](worklog/759-gta-s-next-wall-is-a-null-vtable-virtual-call-not-the-mutex-the-report-guessed.md) |
| - | [760. GTA's null-vtable is a static constructor that never runs - narrowed to init or relocation](worklog/760-gta-s-null-vtable-is-a-static-ctor-that-never-runs-narrowed-to-init-or-relocation.md) |
| - | [761. sceKernelFstat was unwired, and GTA's null-vtable is a runtime-init short-circuit](worklog/761-sce-kernel-fstat-was-unwired-and-the-null-vtable-is-a-runtime-init-short-circuit.md) |
| - | [762. The null-vtable gate hunt rules out three stubs and points at startup construction](worklog/762-the-null-vtable-gate-hunt-rules-out-three-stubs-and-points-at-startup-construction.md) |
| - | [763. No stub gates GTA's object construction - the null-vtable is a non-stub startup value](worklog/763-no-stub-gates-gta-s-object-construction-the-null-vtable-is-a-non-stub-startup-value.md) |
| - | [764. The ctor hunt lands - the null-vtable is an unconstructed global handler the title itself null-guards](worklog/764-the-ctor-hunt-lands-the-null-vtable-is.md) |
| - | [765. GTA's stale handler field is a static-init relocation the redirect-registration never overwrote; init-array is the axis and GetProcParam is the next gap](worklog/765-gta-s-stale-handler-field-is-a-static.md) |
| - | [766. The eboot has no init-array, so 765's axis was wrong; the redirect is runtime input-init that runs after the poll, and the trigger is the real question](worklog/766-the-eboot-has-no-init-array-so-765-s.md) |
| - | [767. GTA polls input without opening a pad; the handler bring-up has one caller, an input-manager-init that never runs before the poll](worklog/767-gta-polls-input-without-opening-a-pad.md) |
| - | [768. GTA's input-manager-init is never entered, not a thread race; it is upstream-gated by int 0x41 resource checks, so the wall is a startup-reachability problem to bank while the loop broadens](worklog/768-gta-s-input-manager-init-is-never.md) |
| - | [769. The retail frontier survey: every title is at a hard wall; PPSA28061 needs sceAgcGetRegisterDefaults2 measured, so an obSCEne request is filed](worklog/769-the-retail-frontier-survey-every-title.md) |
| - | [770. PPSA02664 and PPSA03416 share a clean AGC-descriptor wall the frontier hid as a VCRUNTIME diagnostic artifact; the root NID is unnamed and a trace is requested](worklog/770-ppsa02664-and-ppsa03416-share-a-clean.md) |
| - | [771. Terminator's clean wall is the known int 0x41 chain, not the recorded AGC site; the one active stub is sceUserServiceGetAgeLevel, a real export now probed](worklog/771-terminator-s-clean-wall-is-the-known.md) |
| - | [772. PPSA25872's flagged regression narrows to commit a8b2d74, and it is not the int 0x41 reclassification but a real path change into the error branch](worklog/772-ppsa25872-s-flagged-regression-narrows.md) |
| - | [773. Terminator's int 0x41 is a fatal memory-resource assertion from a pre-set-fatal template, raised through a deep chain with an /app0 file path on the stack](worklog/773-terminator-s-int-0x41-is-a-fatal-memory.md) |
| - | [774. Terminator's wall is an allocation from an already-bad memory pool; the root is the pool creation, not in the call stack, and Il2Cpp string tables block finding it statically](worklog/774-terminator-s-wall-is-an-allocation-from.md) |
| - | [775. The Terminator pool-name watchpoint was never touched and the string identity was conflated; the grind pauses for Il2Cpp metadata tooling and the loop rotates](worklog/775-the-terminator-pool-name-watchpoint-was.md) |
| - | [776. The retail survey is complete: all six titles at deep walls; ASTRO BOT is a null-object deref inside a C++ static constructor and, being native, is the most RE-tractable grind target](worklog/776-the-retail-survey-is-complete-all-six.md) |
| - | [777. ASTRO BOT's null deref traces to a specific eboot global (image+0xe553bd0) that should hold a constructed object but is null, loaded and passed through a thunk to the checking module fn](worklog/777-astro-bot-s-null-deref-traces-to-a.md) |
| - | [778. ASTRO BOT's null global is a memory-arena manager whose construction block is never reached - a 2GB arena setup that orbistoun's startup does not run](worklog/778-astro-bot-s-null-global-is-a-memory.md) |
| - | [779. ASTRO BOT's arena-init has no direct caller; it is one entry in a custom startup init-table of init-fn records that orbistoun never walks](worklog/779-astro-bot-s-arena-init-has-no-direct.md) |
| - | [780. ASTRO BOT's init-table entry is never read at runtime, disproving the walker hypothesis; the arena-init's invocation is elusive, so the grind pauses like the others](worklog/780-astro-bot-s-init-table-entry-is-never.md) |
| - | [781. The ASTRO BOT init-table was a misread of the RELA relocation table; the arena-init is placed at a function pointer by a RELATIVE reloc in a RELRO segment, pointing the wall at reloc/RELRO handling](worklog/781-the-astro-bot-init-table-was-a-misread.md) |
| - | [782. orbistoun applies all of ASTRO BOT's relocations including the arena-init pointer, disproving the RELRO-loader hypothesis; the wall is the same indirect-caller obstacle as the others, so the grind pauses](worklog/782-orbistoun-applies-all-of-astro-bot-s.md) |
| - | [783. The reloc graph follows ASTRO BOT's indirection one level but the pointer-to-table is reached by computed addressing with no static ref; the invocation is untraceable statically, exhausting the retail RE approaches](worklog/783-the-reloc-graph-follows-astro-bot-s.md) |
| - | [784. Running module inits does not make ASTRO BOT's arena-init run, disproving the crt-init hypothesis; the retail deep-RE phase is exhausted and the loop shifts to a heartbeat pending a tool-build or steer](worklog/784-running-module-inits-does-not-make.md) |
| - | [785. ASTRO BOT's invoker never reads the arena-init pointer, closing the runtime approach; the loop redirects to the buildable AGC synthesis path for the leading titles](worklog/785-astro-bot-s-invoker-never-reads-the.md) |
| - | [786. PPSA28061's GetRegisterDefaults2 return structure is decoded (records at +0x30, count at +0x38); a count-zero struct lets the guest skip the register search gracefully, a testable path to move the wall](worklog/786-ppsa28061-s-getregisterdefaults2-return.md) |
| - | [787. PPSA28061's register-defaults descriptors are answered with reserved count-zero regions, shipped as `assumed` knowledge; the wall moves twice (GetRegisterDefaults2 -> Internal -> abort), +67 calls](worklog/787-ppsa28061-register-defaults-descriptors.md) |
| - | [788. PPSA28061's `sceAgcGetIsTrinityMode` is answered from the presented machine (base -> not Trinity), removing a placeholder; measured, it does not gate the libSceAmpr abort, which stays the hardware-faithful frontier](worklog/788-ppsa28061-getis-trinity-mode-answered.md) |
| - | [789. PPSA02664's `image+0x3f8f0` null-deref is a count-gated copy loop storing through `[r15+0x18]`; zero-filling the descriptor `0x71040c4df8235e1d` does not move it, so the empty-workload escape that cleared PPSA28061 does not apply here](worklog/789-ppsa02664-null-deref-decoded-count-loop.md) |
| - | [790. PPSA02664's `image+0x3f8f0` null base is traced to a Unity workload-array element (`array[1]+0x18`), filled by the engine, not the AGC descriptor; the trace premise on the bus is corrected](worklog/790-ppsa02664-workload-null-traced-to-unity-array.md) |
| - | [791. PPSA03416 loads a working fakelib `libSceAmpr` and still faults identically at `image+0x3f8f0`, ruling out the Ampr command-buffer construction as the cause; the `array[1]+0x18` null is neither the descriptor nor Ampr](worklog/791-ppsa03416-fakelib-ampr-still-faults.md) |
| - | [792. PPSA04263's `image+0x19676d7` wall is a virtual call through a null vtable — an object allocated but never constructed; `START_MODULES=all` goes BACK, so it is the same computed-invocation un-run-construction wall as the other native titles](worklog/792-ppsa04263-null-vtable-construction-wall.md) |
| - | [793. PPSA04263's null vtable is a sub-object, not the manager: `array[0]` is constructed and zeroes its own fields, but the object it later points to at `+0x2d0` is allocated-and-zeroed with no constructor run; the construction site evades a write-watchpoint](worklog/793-ppsa04263-subobject-unconstructed.md) |
| - | [794. PPSA04263's null-vtable object is static bss that is never touched, not a failed heap construction; its constructor never runs, and orbistoun runs init arrays only under `START_MODULES` (too early) — the crt-init timing question, potentially buildable](worklog/794-ppsa04263-static-object-unrun-constructor.md) |
| - | [795. PPSA04263's eboot has no `DT_INIT_ARRAY` (size 0), disproving the init-array-timing lead: the static object's constructor is runtime code orbistoun's execution never reaches, so it is the same execution-divergence wall as the other native titles, not a buildable global-constructor fix](worklog/795-ppsa04263-no-init-array-runtime-construction.md) |
| - | [796. A full-code scan finds zero static references to PPSA04263's object, confirming computed-dispatch (tracer-bound); with the retail frontier exhaustively tool/hardware-bound, the loop shifts from wall-moving to honest hardening of real exports](worklog/796-frontier-tool-bound-shift-to-hardening.md) |
| - | [797. `scePthreadGetaffinity` is implemented — the read D523 anticipated, now that PPSA04263 asks for it; standing rises, the wall does not move, and that is the honest shape of the hardening pivot](worklog/797-scepthreadgetaffinity-implemented.md) |
| - | [798. `sceCoredumpRegisterCoredumpHandler` is accepted honestly — the crash-handler registration succeeds rather than answering a placeholder; PPSA04263 standing rises 5→4 stubs, the second honest export of the hardening pivot](worklog/798-scecoredumpregister-accepted.md) |
| - | [799. `sceKernelGetdirentries` is the next honest export but needs a directory-descriptor kind, not an accept stub; the clean accept-pattern hardening is done, and directory support is scoped as the next FS build](worklog/799-getdirentries-needs-directory-descriptors.md) |
| - | [800. `sceKernelGetdirentries` is a 43-match-site directory-descriptor feature, disproportionate to its single call, so it is deferred; the session's accessible work is delivered and the loop drops to a heartbeat pending an obSCEne result or an operator's priority for the larger builds](worklog/800-getdirentries-deferred-loop-to-heartbeat.md) |
| - | [801. The nightly gap-analysis inbox was going unread (the loop polled the outbox); reading it cleared four stale-fact corrections this session's own changes created, and surfaced the execution-tracer as a filed request](worklog/801-inbox-review-gap-analysis-corrections.md) |
| - | [802. The oops-mesa/oops-gl cube apps now render 3D on hardware and are orbistoun's own source end to end — recorded as the fully-owned baseline the tool-bound retail frontier lacks (backlog 037)](worklog/802-fully-owned-cube-apps-baseline-direction.md) |
| - | [803. The gl1-cube baseline's first run through orbistoun hits the D247 loader gap: a properly-built module carries only the SCE dynamic tags, and orbistoun's linker reads the standard ones the retail corpus happens to also carry](worklog/803-gl1-cube-baseline-hits-d247-sce-dynamic-tags.md) |
| - | [804. The D247 fix lands: gl1-cube now links, resolves its imports and enters guest execution — the first fully-owned guest to run, and it faults in startup at an address nothing mapped](worklog/804-gl1-cube-links-resolves-imports-and-enters-guest-execution.md) |
| - | [805. orbistoun serves the console's own syscall gadget at the fixed address a first-party payload falls back to, and gl1-cube runs its startup: nine system calls dispatch, it prints its own banner, and the next wall is the display device `/dev/dce`](worklog/805-console-syscall-gadget-served-gl1-cube-runs-its-startup.md) |
| - | [806. `sceKernelBatchMap` maps the display's scanout buffers, and gl1-cube's display bringup walks one wall further — to `sceVideoOutSetBufferAttribute2`, which is an open rendering-cluster request the cube has now reached](worklog/806-batchmap-maps-the-display-buffers-gl1-cube-reaches-videoout-attributes.md) |
| - | [807. `sceVideoOutSetBufferAttribute2` is implemented, the registered block is decoded into a flipped buffer's shape, and inbox request `-a6b3` is closed — the first rendering-cluster request the fully-owned cube drove to resolution](worklog/807-videoout-set-buffer-attribute2-implemented-closes-req-a6b3.md) |
| - | [808. `sceVideoOutRegisterBuffers2` gets its own handler — it reads a `SceVideoOutBuffer` array, not v1's raw addresses — and the cube's display goes ready: it enters its 3D render loop and reaches `flipped`, 32,988 calls up from 28](worklog/808-register-buffers2-struct-array-cube-reaches-flipped.md) |
| - | [809. `sceAgcDriverCreateQueue` hands the guest a queue handle, so the cube turns on its hardware pipeline and runs the real draw path — 150,996 calls, and the wall moves from "no hardware pipeline" to an unnamed submit hash](worklog/809-agc-create-queue-hands-a-handle-cube-turns-on-hardware.md) |
| - | [810. Neverball (`NVRB00001`) joins the corpus as the second fully-owned baseline — a real game that boots in-game on hardware — and its first run reaches `flipped` with 7,242 frames](worklog/810-neverball-joins-the-corpus-as-a-fully-owned-game-baseline.md) |
| - | [811. The clock and call-budget endings now report what the guest asked for — and Neverball's data wall names itself: `/app0/./data`](worklog/811-the-clock-and-budget-endings-report-what-the-guest-asked-for.md) |
| - | [812. A `.` path component names the directory it sits in, so Neverball finds its own data: 55 files open, the title music and level geometry load, and the event-pump spin gives way to real painting](worklog/812-a-dot-component-names-its-own-directory-and-neverball-loads-its-data.md) |
| - | [813. `sceAgcDriverSubmitCommandBuffer` is named and implemented, and both fully-owned baselines move to the wall that was always there: the guest waits for a GPU completion orbistoun cannot yet produce — verdict BACK, kept on purpose](worklog/813-submit-command-buffer-named-and-the-wall-moves-to-gpu-completion.md) |
| - | [814. A submission reaches the backend on the endings a stalled guest actually takes — and the cube's clear self-test walks to zero packets, so the first blocker to execution is that orbistoun cannot see the command buffer, not speed](worklog/814-a-stalled-guest-submission-reaches-the-backend-and-walks-to-nothing.md) |
| - | [815. A submit reads guest memory as the run stands, not as it stood at entry — and the cube's clear self-test walks from 0 packets to 192](worklog/815-a-submit-reads-guest-memory-live-and-the-cube-s-clear-walks-to-192-packets.md) |
| - | [816. The command processor's memory work executes at submit, and the GL clear self-test passes on both baselines — every pixel matched, the fence written by work that ran — verdict FURTHER](worklog/816-the-command-processor-executes-its-memory-work-and-the-clear-self-test-passes.md) |
| - | [817. Syscalls save their registers to the calling thread's own stack, not one shared buffer — the race that nulled libvorbis's table is gone, and Neverball's audio thread streams the title music](worklog/817-syscalls-save-to-the-calling-thread-s-stack-and-neverball-s-audio-thread-lives.md) |
| - | [818. The backend's refusals are tallied by reason and the run report says why a shader did not translate — Neverball's 450 refused draws are one untranslated fragment shader, stopped at one unnamed opcode](worklog/818-a-refusal-says-its-reason-and-a-shader-that-did-not-translate-says-why.md) |
| - | [819. `s_wqm_b32` and eleven more of the textured pixel shader's opcodes are named by the reference, whole-quad mode translates for 32 lanes, and Neverball's fragment shader now translates to its last seven words](worklog/819-s-wqm-b32-and-eleven-more-named-and-the-textured-shader-translates-to-its-export.md) |
| - | [820. The half-float pack and the compressed export translate, and Neverball's first frame reaches the backend whole — 454 commands carried out, none refused, a 1920x1080 frame](worklog/820-the-half-float-pack-and-the-compressed-export-translate-and-neverball-s-first-frame-draws.md) |
| - | [821. The rendered frame is written, not dropped — `REQ-...1f07` closed — and the first frames both fully-owned baselines produce are uniformly black](worklog/821-the-rendered-frame-is-written-not-dropped-and-both-baselines-draw-black.md) |
| - | [822. A frame's draws accumulate on their target instead of each starting from black — and a full census shows the baselines' draws cover no pixel at all](worklog/822-a-frame-s-draws-accumulate-on-their-target-and-the-baselines-draws-cover-nothing.md) |
| - | [823. The render log lists a submission's commands — and a live submission's shaders have no window onto their own vertices, so every triangle collapses](worklog/823-the-render-log-lists-a-submission-s-commands-and-a-live-submission-has-no-window-onto-its-vertices.md) |
| - | [824. A live submission's window is placed at the base its vertex shader forms, and the cube draws its first pixels — a Gouraud-shaded face, 114,282 of them](worklog/824-the-window-is-placed-from-the-vertex-shader-and-the-cube-draws-its-first-face.md) |
| - | [825. Each draw carries its own user data — the cube's twelve per-draw vertex positions in its buffer and its texture table reach the command list; the backend refuses them by name until a shader reads them](worklog/825-each-draw-carries-its-own-user-data-to-the-command-list.md) |
| - | [826. Translated shaders read their user data at entry and the backend pushes it per draw — the cube draws all twelve faces, and Neverball's draws cover the screen](worklog/826-shaders-read-their-user-data-and-the-cube-draws-all-twelve-faces.md) |
| - | [827. The textures a submission's draws name are measured — Neverball's title screen names one, a 16x128 linear RGBA8 image, which needs no detiling to bind](worklog/827-the-textures-a-submission-names-are-measured-and-neverball-s-is-one-linear-rgba8-image.md) |
| - | [828. Each draw samples the guest's own texture — Neverball's screen goes from the default white to a colour of its own, and the next wall is the blend state no draw applies](worklog/828-each-draw-samples-its-own-texture-and-neverball-s-screen-turns-from-white-to-its-own-colour.md) |
| - | [829. The guest's blend state is applied per draw — and Neverball's one-colour frame turns out not to be a blending artefact](worklog/829-the-guest-s-blend-state-is-applied-per-draw-and-neverball-s-one-colour-is-not-a-blending-artefact.md) |
| - | [830. Neverball's first frame is its space background, rendered correctly — the one colour is the asset's own, and the wall moves from rendering to the frame after it](worklog/830-neverball-s-first-frame-is-its-space-background-rendered-correctly.md) |
| - | [831. Whole-surface 64KB_R_X tiling, both directions, on a display-size hardware readback](worklog/831-whole-surface-64kb-r-x-tiling-on-a-measured-readback.md) |
| - | [832. Draws run at submit and are written back into the guest's target, and both baselines reach their next frame](worklog/832-draws-run-at-submit-and-are-written-back-and-both-baselines-reach-their-next-frame.md) |
| - | [833. An invented mip level lost the device, and Neverball draws five frames of its title scene](worklog/833-an-invented-mip-level-lost-the-device-and-neverball-draws-five-frames-of-its-title-scene.md) |
| - | [834. The output clamp translates by the stage's DX10_CLAMP mode, and Neverball draws thirteen frames](worklog/834-the-output-clamp-translates-by-the-stage-s-dx10-clamp-mode-and-neverball-draws-thirteen-frames.md) |
| - | [835. Each draw binds the shader in force at its own packet](worklog/835-each-draw-binds-the-shader-in-force-at-its-own-packet.md) |
| - | [836. The vertex program is read from PGM_LO_ES, and Neverball's spikes are gone](worklog/836-the-vertex-program-is-read-from-pgm-lo-es-and-neverball-s-spikes-are-gone.md) |
| - | [837. The guest's viewport transform is applied, and a GL frame is many submissions](worklog/837-the-guest-s-viewport-transform-is-applied-and-a-gl-frame-is-many-submissions.md) |
| - | [838. Host reads come from cached memory, and a submission draws four times faster](worklog/838-host-reads-come-from-cached-memory-and-a-submission-draws-four-times-faster.md) |
| - | [839. Draws land on a resident attachment](worklog/839-draws-land-on-a-resident-attachment.md) |
| - | [840. Two textures per draw, and Neverball's title and menu render](worklog/840-two-textures-per-draw-and-neverball-s-title-and-menu-render.md) |
| - | [841. The running title is shown in the window](worklog/841-the-running-title-is-shown-in-the-window.md) |
| - | [842. SeaShell lists the library and launches from it](worklog/842-seashell-lists-the-library-and-launches-from-it.md) |
| - | [843. Draws reuse their pipelines and stop waiting per draw](worklog/843-draws-reuse-their-pipelines-and-stop-waiting-per-draw.md) |
| - | [844. A performance overlay, and what it found](worklog/844-a-performance-overlay-and-what-it-found.md) |
| - | [845. The frame stays on the device until the flip](worklog/845-the-frame-stays-on-the-device-until-the-flip.md) |
| - | [846. Every oops-apps app is in the corpus](worklog/846-every-oops-app-is-in-the-corpus.md) |
| - | [847. A window ring, and the GPU's own clock](worklog/847-a-window-ring-and-the-gpu-s-own-clock.md) |
| - | [848. An orphaned worker ends with its parent](worklog/848-an-orphaned-worker-ends-with-its.md) |
| - | [849. A DMA that reads a pending target writes it back first](worklog/849-a-dma-that-reads-a-pending-target-writes-it-back-first.md) |
| - | [850. Lazy readback copies](worklog/850-lazy-readback-copies.md) |
| - | [851. One device thread, and write-watched guest memory](worklog/851-one-device-thread-and-write-watched-guest-memory.md) |
| - | [852. Where the time goes, measured, and draws batched](worklog/852-where-the-time-goes-measured-and-draws-batched.md) |
| - | [853. Decodes kept by their bytes, and a sweep that stops allocating](worklog/853-decodes-kept-by-their-bytes-and-a-sweep.md) |
| - | [854. Neverball from three frames a second to ten](worklog/854-neverball-from-three-frames-a-second-to.md) |
| - | [855. Flipped frames written back when read, and shown from the device](worklog/855-flipped-frames-written-back-when-read.md) |
| - | [856. A colour target write-protected while trusted unchanged](worklog/856-a-colour-target-write-protected-while.md) |
| - | [857. A uniform target skips its detile, and the window is compared, not hashed](worklog/857-a-uniform-target-skips-its-detile-and.md) |
| - | [858. The per-submission line behind ORBISTOUN_TRACE_SUBMITS](worklog/858-the-per-submission-line-behind.md) |
| - | [859. Pad input recorded and replayed by flips, and faults name their page's protection](worklog/859-pad-input-recorded-and-replayed-by.md) |
| - | [860. A direct-memory alias forgotten with its memory, and the in-game crash](worklog/860-a-direct-memory-alias-forgotten-with.md) |
| - | [861. Input capture and playback on the toolbar, never automatic](worklog/861-input-capture-and-playback-on-the.md) |
| - | [862. A signed-off capture takes Neverball in-game headless](worklog/862-a-signed-off-capture-takes-neverball-in.md) |
| - | [863. A cleared target seeded on the device](worklog/863-a-cleared-target-seeded-on-the.md) |
| - | [864. Neverball plays, staged with a writable /app0](worklog/864-neverball-plays-staged-with-a-writable.md) |
| - | [865. Neverball ran four times fast in the window](worklog/865-neverball-ran-four-times-fast-in-the.md) |
| - | [866. A guest's first argument names its module under /app0, not the host path](worklog/866-a-guest-s-first-argument-names-its-module.md) |
| - | [867. PPSA25872's trap traced to the APR resolve's unfilled size](worklog/867-ppsa25872-s-trap-traced-to-the-apr-resolve-s.md) |
| - | [868. sceKernelVirtualQuery answers the whole 72-byte structure the console writes](worklog/868-sce-kernel-virtual-query-answers-the-whole.md) |
| - | [869. pthread_setschedparam and pthread_getschedparam served under their POSIX names](worklog/869-pthread-setschedparam-served-under-its-posix.md) |
| - | [870. The APR resolve answers its measured contract, and PPSA25872 moves](worklog/870-the-apr-resolve-answers-its-measured-contract.md) |
