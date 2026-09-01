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
`tools/split-doc.sh --index orbistoun WORKLOG 2 worklog`.

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
