//! Structured control flow: a guest program's blocks as nested selections and loops (D972).
//!
//! The dispatch loop ([`crate::control`]) runs every block as an arm of one switch on a program
//! counter, which is valid however tangled the guest's flow, but leaves the host compiler a single
//! loop in which any block may follow any other. A compiler-made program is almost always
//! reducible and nested, so its blocks are laid out here as the selections and loops they already
//! are: a conditional branch becomes a selection that merges at the branch's immediate
//! post-dominator, a natural loop a loop with its own merge and continue target, and a jump out of
//! one a break or a continue.
//!
//! A jump the nesting cannot express directly - the program's end from inside a loop, a continue
//! or break of an outer loop from an inner one - writes where it goes into the program counter,
//! breaks out of the inner loop, and the loop's merge routes on the counter. A wave that ended
//! holds the exit value there, as in the dispatch loop, so a later segment of a group's program
//! skips it. A segment whose flow is irreducible, or does not nest, keeps the dispatch loop.
//!
//! Every block is placed exactly once, or the plan is refused before anything is emitted: the
//! module then has exactly the guest's edges, only drawn as structure.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use orbistoun_shader::{Decode, Instruction};
use orbistoun_spirv::{Id, op};

use crate::TranslateError;
use crate::blocks::{self, Block, Terminator};
use crate::control;
use crate::model::Model;

/// The counter value a loop with routed exits starts from: no block's index, and not the exit.
const NEUTRAL: u32 = u32::MAX;

thread_local! {
    /// A test's choice of shape, over the process's.
    static FORCED: Cell<Option<bool>> = const { Cell::new(None) };
}

/// The process's choice: structure where it nests, or the dispatch loop for every program.
static CHOSEN: AtomicBool = AtomicBool::new(true);

/// Chooses for the process: structure where it nests (`true`, the default), or the dispatch loop
/// for every program (`false`, `ORBISTOUN_CONTROL_FLOW=dispatch`), to compare the two.
pub fn set_enabled(structured: bool) {
    CHOSEN.store(structured, Ordering::Relaxed);
}

/// Whether programs are laid out as structure, where they can be: the process's choice, unless this
/// thread forced one.
#[must_use]
pub fn enabled() -> bool {
    FORCED
        .get()
        .unwrap_or_else(|| CHOSEN.load(Ordering::Relaxed))
}

/// Runs `f` with structure chosen (`true`) or the dispatch loop (`false`) on this thread, so a test
/// can translate one program both ways.
pub fn with_shape<T>(structured: bool, f: impl FnOnce() -> T) -> T {
    let before = FORCED.replace(Some(structured));
    let result = f();
    FORCED.set(before);
    result
}

static STRUCTURED: AtomicUsize = AtomicUsize::new(0);
static DISPATCHED: AtomicUsize = AtomicUsize::new(0);

/// Segments laid out as structure, and as a dispatch loop, since the process started: how often
/// the fallback is taken.
#[must_use]
pub fn shapes() -> (usize, usize) {
    (
        STRUCTURED.load(Ordering::Relaxed),
        DISPATCHED.load(Ordering::Relaxed),
    )
}

/// Counts one segment laid out as a dispatch loop.
pub(crate) fn note_dispatched() {
    DISPATCHED.fetch_add(1, Ordering::Relaxed);
}

/// Where an edge goes, among a segment's blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum To {
    /// A block of the segment, by its index within the segment.
    Node(usize),
    /// The program's end: `s_endpgm`.
    End,
    /// Past the segment's last block, into the next segment.
    Leave,
}

/// How a block ends.
#[derive(Debug, Clone, Copy)]
enum Ends {
    /// To one place.
    Goto(To),
    /// To `taken` where its branch's condition holds, else to `fall`.
    Branch { taken: To, fall: To },
}

/// A jump that leaves the construct it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Jump {
    /// Back to the innermost loop's header; `branch` is the offset of the instruction that jumps,
    /// which names the loop if it is cut (D949).
    Continue { branch: u32 },
    /// Out of the innermost loop to the block `code` indexes; `next` where that is the block the
    /// loop's merge runs on into, which needs no routing.
    Break { code: u32, next: bool },
    /// The program's end.
    Exit,
    /// Into the next segment.
    Leave,
    /// Forward past the join of the selection it is in, to the block `code` indexes: the counter
    /// holds `code`, the innermost selection's merge is taken, and each [`Node::Guard`] for it on
    /// the way skips what it holds.
    Escape { code: u32 },
}

/// One element of a structured sequence.
#[derive(Debug, Clone)]
pub(crate) enum Node {
    /// A guest block's instructions, by its index in the program.
    Block(usize),
    /// A selection on the branch ending block `at`, whose condition sends it to `taken`, else to
    /// `fall`. `merged` where the two meet again after it; otherwise each leaves by a jump.
    If {
        at: usize,
        taken: Vec<Node>,
        fall: Vec<Node>,
        merged: bool,
        /// Whether the counter is cleared first, because an arm may escape past the merge
        /// ([`Jump::Escape`]) and a value left by an earlier pass must not read as one.
        reset: bool,
    },
    /// `body`, run unless the counter holds `code`: what lies between a selection's join and the
    /// block one of its arms escaped to.
    Guard { code: u32, body: Vec<Node> },
    /// A loop around `body`, which leaves only by jumps. Its merge tests the program counter for
    /// each of `routes` - a block index and the jump that reaches it from outside the loop - and
    /// otherwise runs on into the sequence where `next`.
    Loop {
        body: Vec<Node>,
        routes: Vec<(u32, Jump)>,
        next: bool,
        /// Whether the counter is cleared on entry, so a value an earlier run of the loop left
        /// there routes nothing: where its merge tests the counter for a block.
        reset: bool,
    },
    /// A loop's merge choosing among the blocks of the region it left for, by the counter: the
    /// first of `arms` whose block index the counter holds, else `otherwise`. `merged` as for
    /// [`Node::If`].
    Select {
        arms: Vec<(u32, Vec<Node>)>,
        otherwise: Vec<Node>,
        merged: bool,
    },
    /// A jump; the sequence ends with it.
    Jump(Jump),
}

/// A segment's structure, ready to emit.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    seq: Vec<Node>,
}

/// Where a region runs on after a construct it placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    /// Into this block.
    At(usize),
    /// Nowhere: the region ends with the construct.
    Ends,
}

impl From<Option<usize>> for Then {
    fn from(next: Option<usize>) -> Self {
        next.map_or(Self::Ends, Self::At)
    }
}

/// A loop's exits as its region sees them: the region's blocks it leaves for, and the jumps its
/// merge routes, each with the block index the counter holds for it.
type Exits = (Vec<usize>, Vec<(u32, Jump)>);

/// Where a target falls, seen from inside a region.
enum Class {
    /// The region's end: the merge it runs on to.
    Stop,
    /// A block the region places.
    Local(usize),
    /// A jump out of the region.
    Jump(Jump),
}

/// The segment's blocks as a graph, and what is known of its shape.
struct Shaper {
    /// Index of the segment's first block in the program.
    base: usize,
    ends: Vec<Ends>,
    /// Offset of each block's last instruction.
    branch_at: Vec<u32>,
    reachable: Vec<bool>,
    /// Each block's immediate dominator; the entry's is itself.
    idom: Vec<usize>,
    /// Order in a reverse postorder from the entry.
    rpo_index: Vec<usize>,
    /// For each loop header, which blocks its natural loop holds.
    body: Vec<Option<Vec<bool>>>,
    /// Each node's immediate post-dominator in the acyclic graph ([`Self::post_dominators`]).
    ipdom: Vec<usize>,
    /// Each loop header's node in that graph, which stands for the loop's whole run.
    loop_node: Vec<usize>,
    /// For each loop being placed, the block its merge runs on into.
    next_of: Vec<Option<usize>>,
    placed: Vec<u8>,
    /// Why the plan was refused, where it was: what a census counts.
    refused: Option<String>,
    /// The blocks the selections being placed escape to, outermost first ([`Jump::Escape`]).
    escapes: Vec<usize>,
}

impl Shaper {
    fn len(&self) -> usize {
        self.ends.len()
    }

    fn successors(&self, node: usize) -> impl Iterator<Item = To> + '_ {
        let (first, second) = match self.ends[node] {
            Ends::Goto(to) => (to, None),
            Ends::Branch { taken, fall } => (taken, Some(fall)),
        };
        std::iter::once(first).chain(second)
    }

    fn node_successors(&self, node: usize) -> impl Iterator<Item = usize> + '_ {
        self.successors(node).filter_map(|to| match to {
            To::Node(v) => Some(v),
            To::End | To::Leave => None,
        })
    }

    fn dominates(&self, a: usize, mut b: usize) -> bool {
        loop {
            if a == b {
                return true;
            }
            let up = self.idom[b];
            if up == b {
                return false;
            }
            b = up;
        }
    }

    fn in_body(&self, header: usize, node: usize) -> bool {
        self.body[header].as_ref().is_some_and(|body| body[node])
    }

    fn is_header(&self, node: usize) -> bool {
        self.body[node].is_some()
    }

    /// Whether `from -> to` returns to a loop's header.
    fn is_back_edge(&self, from: usize, to: usize) -> bool {
        self.is_header(to) && self.dominates(to, from)
    }

    /// The reachable blocks, their dominators, and a reverse postorder; `None` where the graph is
    /// irreducible.
    fn dominators(&mut self) -> Option<()> {
        let n = self.len();
        // An iterative depth-first walk recording the postorder.
        let mut state = vec![0_u8; n]; // 0 unseen, 1 on the stack, 2 done
        let mut postorder = Vec::with_capacity(n);
        let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
        state[0] = 1;
        let mut retreating = Vec::new();
        while let Some(&mut (node, ref mut next)) = stack.last_mut() {
            let successors: Vec<usize> = self.node_successors(node).collect();
            if let Some(&to) = successors.get(*next) {
                *next += 1;
                match state[to] {
                    0 => {
                        state[to] = 1;
                        stack.push((to, 0));
                    }
                    1 => retreating.push((node, to)),
                    _ => {}
                }
            } else {
                state[node] = 2;
                postorder.push(node);
                stack.pop();
            }
        }
        self.reachable = state.iter().map(|&s| s == 2).collect();
        let rpo: Vec<usize> = postorder.iter().rev().copied().collect();
        self.rpo_index = vec![usize::MAX; n];
        for (i, &node) in rpo.iter().enumerate() {
            self.rpo_index[node] = i;
        }
        let mut predecessors = vec![Vec::new(); n];
        for &node in &rpo {
            for to in self.node_successors(node) {
                predecessors[to].push(node);
            }
        }
        // Dominators by iterating to a fixed point over the reverse postorder.
        self.idom = vec![usize::MAX; n];
        self.idom[0] = 0;
        let mut changed = true;
        while changed {
            changed = false;
            for &node in rpo.iter().skip(1) {
                let mut new = usize::MAX;
                for &p in &predecessors[node] {
                    if self.idom[p] == usize::MAX {
                        continue;
                    }
                    new = if new == usize::MAX {
                        p
                    } else {
                        self.intersect(p, new)
                    };
                }
                if new != self.idom[node] {
                    self.idom[node] = new;
                    changed = true;
                }
            }
        }
        // Reducible: every edge back to a node on the walk's stack goes to a node that dominates
        // its source.
        if retreating
            .iter()
            .any(|&(from, to)| !self.dominates(to, from))
        {
            return None;
        }
        // Natural loops: each header's blocks, those that reach a back edge to it without passing
        // through it.
        self.body = vec![None; n];
        for &(from, to) in &retreating {
            let body = self.body[to].get_or_insert_with(|| {
                let mut body = vec![false; n];
                body[to] = true;
                body
            });
            let mut work = vec![from];
            while let Some(node) = work.pop() {
                if body[node] {
                    continue;
                }
                body[node] = true;
                work.extend(predecessors[node].iter().copied());
            }
        }
        Some(())
    }

    fn intersect(&self, mut a: usize, mut b: usize) -> usize {
        while a != b {
            while self.rpo_index[a] > self.rpo_index[b] {
                a = self.idom[a];
            }
            while self.rpo_index[b] > self.rpo_index[a] {
                b = self.idom[b];
            }
        }
        a
    }

    /// Immediate post-dominators over the acyclic graph that has each loop's back edges sent to a
    /// node of its own, which goes on to the loop's exits, and every end to one sink. A loop then
    /// stands for its whole run, and a branch merges where its paths meet after it. `None` where
    /// the graph has a cycle that is not a natural loop.
    fn post_dominators(&mut self) -> Option<()> {
        let n = self.len();
        let sink = n;
        // The loops' nodes, numbered after the sink.
        let mut loop_node = vec![usize::MAX; n];
        let mut count = n + 1;
        for (header, slot) in loop_node.iter_mut().enumerate() {
            if self.is_header(header) && self.reachable[header] {
                *slot = count;
                count += 1;
            }
        }
        let mut successors: Vec<Vec<usize>> = vec![Vec::new(); count];
        let map = |shaper: &Self, from: usize, to: To| match to {
            To::End | To::Leave => sink,
            To::Node(v) if shaper.is_back_edge(from, v) => loop_node[v],
            To::Node(v) => v,
        };
        for node in (0..n).filter(|&node| self.reachable[node]) {
            let mut out: Vec<usize> = self
                .successors(node)
                .map(|to| map(self, node, to))
                .collect();
            out.dedup();
            successors[node] = out;
        }
        for header in (0..n).filter(|&h| loop_node[h] != usize::MAX) {
            let mut exits = Vec::new();
            for node in (0..n).filter(|&node| self.reachable[node] && self.in_body(header, node)) {
                for to in self.successors(node) {
                    if let To::Node(v) = to
                        && !self.in_body(header, v)
                    {
                        let mapped = map(self, node, to);
                        if !exits.contains(&mapped) {
                            exits.push(mapped);
                        }
                    }
                }
            }
            if exits.is_empty() {
                exits.push(sink);
            }
            successors[loop_node[header]] = exits;
        }
        // A postorder of the acyclic graph: each node after all it reaches.
        let mut state = vec![0_u8; count];
        let mut postorder = Vec::with_capacity(count);
        let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
        state[0] = 1;
        while let Some(&mut (node, ref mut next)) = stack.last_mut() {
            if let Some(&to) = successors[node].get(*next) {
                *next += 1;
                match state[to] {
                    0 => {
                        state[to] = 1;
                        stack.push((to, 0));
                    }
                    1 => return None,
                    _ => {}
                }
            } else {
                state[node] = 2;
                postorder.push(node);
                stack.pop();
            }
        }
        let mut ipdom = vec![usize::MAX; count];
        let mut depth = vec![0_usize; count];
        ipdom[sink] = sink;
        for &node in &postorder {
            if node == sink {
                continue;
            }
            let mut meet: Option<usize> = None;
            for &to in &successors[node] {
                meet = Some(match meet {
                    None => to,
                    Some(at) => {
                        let (mut a, mut b) = (at, to);
                        while a != b {
                            if depth[a] >= depth[b] {
                                a = ipdom[a];
                            } else {
                                b = ipdom[b];
                            }
                        }
                        a
                    }
                });
            }
            let meet = meet.unwrap_or(sink);
            ipdom[node] = meet;
            depth[node] = depth[meet] + 1;
        }
        self.ipdom = ipdom;
        self.loop_node = loop_node;
        Some(())
    }

    /// Whether `from` reaches `target` by forward edges within the loop `context`.
    fn reaches(&self, from: usize, target: usize, context: Option<usize>) -> bool {
        let mut seen = vec![false; self.len()];
        let mut work = vec![from];
        while let Some(node) = work.pop() {
            if node == target {
                return true;
            }
            if seen[node] {
                continue;
            }
            seen[node] = true;
            for to in self.node_successors(node) {
                let inside = context.is_none_or(|h| to != h && self.in_body(h, to));
                if inside && !self.is_back_edge(node, to) {
                    work.push(to);
                }
            }
        }
        false
    }

    fn classify(
        &self,
        to: To,
        context: Option<usize>,
        stop: Option<usize>,
        branch: u32,
    ) -> Option<Class> {
        let v = match to {
            To::End => return Some(Class::Jump(Jump::Exit)),
            // A segment ends at a barrier, which no loop holds (D892).
            To::Leave => return context.is_none().then_some(Class::Jump(Jump::Leave)),
            To::Node(v) => v,
        };
        if Some(v) == stop {
            return Some(Class::Stop);
        }
        if let Some(header) = context {
            if v == header {
                return Some(Class::Jump(Jump::Continue { branch }));
            }
            if !self.in_body(header, v) {
                return Some(Class::Jump(Jump::Break {
                    code: u32::try_from(self.base + v).ok()?,
                    next: self.next_of[header] == Some(v),
                }));
            }
        }
        if self.escapes.contains(&v) {
            return Some(Class::Jump(Jump::Escape {
                code: u32::try_from(self.base + v).ok()?,
            }));
        }
        Some(Class::Local(v))
    }

    fn place(&mut self, node: usize) -> Option<()> {
        self.placed[node] = self.placed[node].saturating_add(1);
        if self.placed[node] == 1 {
            return Some(());
        }
        self.refuse(format!(
            "block {} is reached from two places",
            self.base + node
        ))
    }

    /// Refuses the plan, saying why.
    fn refuse<T>(&mut self, why: String) -> Option<T> {
        self.refused.get_or_insert(why);
        None
    }

    /// The structure from `first` until control reaches `stop`, inside the loop `context`.
    fn region(
        &mut self,
        first: usize,
        stop: Option<usize>,
        context: Option<usize>,
    ) -> Option<Vec<Node>> {
        let mut seq = Vec::new();
        let mut current = first;
        loop {
            let next = if self.is_header(current) && context != Some(current) {
                self.place_loop(current, (context, stop), &mut seq)?
            } else {
                self.place_block(current, (context, stop), &mut seq)?
            };
            match next {
                Then::At(next) if Some(next) != stop => current = next,
                Then::At(_) | Then::Ends => return Some(seq),
            }
        }
    }

    /// Places the loop at `header` and what its merge chooses among; answers the block the region
    /// runs on into after it, if any.
    fn place_loop(
        &mut self,
        header: usize,
        (context, stop): (Option<usize>, Option<usize>),
        seq: &mut Vec<Node>,
    ) -> Option<Then> {
        let (locals, routes) = self.exits(header, context, stop)?;
        // The block the merge runs on into without a test: the first the loop leaves for.
        let next = locals.iter().copied().min();
        self.next_of[header] = next;
        let body = self.region(header, None, Some(header))?;
        let reset =
            (!routes.is_empty() && (next.is_some() || routes.len() > 1)) || locals.len() > 1;
        seq.push(Node::Loop {
            body,
            routes,
            next: next.is_some(),
            reset,
        });
        if locals.len() < 2 {
            return Some(next.into());
        }
        let merge = self.merge_from(self.loop_node[header], &locals, context, stop);
        let mut arms = Vec::new();
        for &to in locals.iter().filter(|&&to| Some(to) != next) {
            let code = u32::try_from(self.base + to).ok()?;
            arms.push((code, self.arm(To::Node(to), merge, context, 0)?));
        }
        let otherwise = self.arm(To::Node(next?), merge, context, 0)?;
        seq.push(Node::Select {
            arms,
            otherwise,
            merged: merge.is_some(),
        });
        Some(merge.into())
    }

    /// Places the block `node` and how it ends; answers the block the region runs on into after it,
    /// if any.
    fn place_block(
        &mut self,
        node: usize,
        (context, stop): (Option<usize>, Option<usize>),
        seq: &mut Vec<Node>,
    ) -> Option<Then> {
        self.place(node)?;
        seq.push(Node::Block(self.base + node));
        let at = self.branch_at[node];
        match self.ends[node] {
            Ends::Goto(to) => match self.classify(to, context, stop, at)? {
                Class::Stop => Some(Then::Ends),
                Class::Jump(jump) => {
                    seq.push(Node::Jump(jump));
                    Some(Then::Ends)
                }
                Class::Local(v) => Some(Then::At(v)),
            },
            Ends::Branch { taken, fall } => {
                self.place_branch(node, (taken, fall), (context, stop), seq)
            }
        }
    }

    /// Places the selection on the branch ending `node`; answers the block the region runs on into
    /// after it, if any.
    fn place_branch(
        &mut self,
        node: usize,
        (taken, fall): (To, To),
        (context, stop): (Option<usize>, Option<usize>),
        seq: &mut Vec<Node>,
    ) -> Option<Then> {
        let at = self.branch_at[node];
        let merge = self.merge_of(node, context, stop);
        // Where the arms first meet, where that is before the merge: an arm that reaches the merge
        // without passing there escapes past it.
        let join = self
            .join_of((taken, fall), merge, context, stop)
            .filter(|&join| Some(join) != merge);
        let Some(join) = join else {
            let taken = self.arm(taken, merge, context, at)?;
            let fall = self.arm(fall, merge, context, at)?;
            seq.push(Node::If {
                at: self.base + node,
                taken,
                fall,
                merged: merge.is_some(),
                reset: false,
            });
            return Some(merge.into());
        };
        let escape = merge;
        self.escapes.extend(escape);
        let arms = self
            .arm(taken, Some(join), context, at)
            .and_then(|taken| Some((taken, self.arm(fall, Some(join), context, at)?)));
        if escape.is_some() {
            self.escapes.pop();
        }
        let (taken, fall) = arms?;
        let Some(target) = escape else {
            seq.push(Node::If {
                at: self.base + node,
                taken,
                fall,
                merged: true,
                reset: false,
            });
            return Some(Then::At(join));
        };
        let code = u32::try_from(self.base + target).ok()?;
        seq.push(Node::If {
            at: self.base + node,
            taken: guard_after(taken, code),
            fall: guard_after(fall, code),
            merged: true,
            reset: true,
        });
        let body = self.region(join, Some(target), context)?;
        seq.push(Node::Guard { code, body });
        Some(Then::At(target))
    }

    /// The first block, in reverse postorder, that both of a branch's arms reach by forward edges
    /// inside the loop `context` without passing `merge` or `stop`.
    fn join_of(
        &self,
        (taken, fall): (To, To),
        merge: Option<usize>,
        context: Option<usize>,
        stop: Option<usize>,
    ) -> Option<usize> {
        let reach = |to: To| -> Vec<bool> {
            let mut seen = vec![false; self.len()];
            let start = match self.classify(to, context, stop, 0) {
                Some(Class::Local(v)) if Some(v) != merge => v,
                _ => return seen,
            };
            let mut work = vec![start];
            while let Some(node) = work.pop() {
                if std::mem::replace(&mut seen[node], true) {
                    continue;
                }
                for to in self.node_successors(node) {
                    let inside = context.is_none_or(|h| to != h && self.in_body(h, to));
                    if inside
                        && !self.is_back_edge(node, to)
                        && Some(to) != stop
                        && Some(to) != merge
                        && !self.escapes.contains(&to)
                    {
                        work.push(to);
                    }
                }
            }
            seen
        };
        let (a, b) = (reach(taken), reach(fall));
        (0..self.len())
            .filter(|&v| a[v] && b[v])
            .min_by_key(|&v| self.rpo_index[v])
    }

    fn arm(
        &mut self,
        to: To,
        merge: Option<usize>,
        context: Option<usize>,
        branch: u32,
    ) -> Option<Vec<Node>> {
        match self.classify(to, context, merge, branch)? {
            Class::Stop => Some(Vec::new()),
            Class::Jump(jump) => Some(vec![Node::Jump(jump)]),
            Class::Local(v) => self.region(v, merge, context),
        }
    }

    /// Where the branch ending `node` merges inside its region: its immediate post-dominator where
    /// the region holds it, else the region's own end where the branch can reach it, else nowhere,
    /// each way then leaving by a jump.
    fn merge_of(&self, node: usize, context: Option<usize>, stop: Option<usize>) -> Option<usize> {
        self.merge_from(node, &[node], context, stop)
    }

    /// As [`Self::merge_of`], for the node `at` of the acyclic graph, whose paths start at
    /// `sources`.
    fn merge_from(
        &self,
        at: usize,
        sources: &[usize],
        context: Option<usize>,
        stop: Option<usize>,
    ) -> Option<usize> {
        let m = self.ipdom[at];
        let local = m < self.len()
            && matches!(
                self.classify(To::Node(m), context, stop, 0),
                Some(Class::Local(_) | Class::Stop)
            );
        let reaches_stop = stop.is_some_and(|s| {
            sources
                .iter()
                .any(|&source| self.reaches(source, s, context))
        });
        if local {
            match stop {
                // Past the region's end: merge at the end, as far as this region sees.
                Some(s) if m != s && self.reaches(s, m, context) => reaches_stop.then_some(s),
                _ => Some(m),
            }
        } else if reaches_stop {
            stop
        } else {
            None
        }
    }

    /// The loop at `header`'s exits, seen from the region it sits in: the blocks of that region it
    /// leaves for, and the jumps routed from its merge.
    fn exits(&self, header: usize, context: Option<usize>, stop: Option<usize>) -> Option<Exits> {
        let mut locals: Vec<usize> = Vec::new();
        let mut routes: Vec<(u32, Jump)> = Vec::new();
        for node in (0..self.len()).filter(|&node| self.reachable[node]) {
            if !self.in_body(header, node) {
                continue;
            }
            for to in self.successors(node) {
                let To::Node(v) = to else { continue };
                if self.in_body(header, v) {
                    continue;
                }
                match self.classify(to, context, stop, self.branch_at[node])? {
                    Class::Stop | Class::Local(_) => {
                        if !locals.contains(&v) {
                            locals.push(v);
                        }
                    }
                    Class::Jump(jump) => {
                        let code = u32::try_from(self.base + v).ok()?;
                        if !routes.iter().any(|(c, _)| *c == code) {
                            routes.push((code, jump));
                        }
                    }
                }
            }
        }
        Some((locals, routes))
    }
}

/// Lays out the blocks `range` names as structure, or `None` where their flow does not nest.
///
/// # Errors
///
/// A successor that is not the start of any block, as the dispatch loop refuses it.
pub(crate) fn plan(
    decode: &Decode,
    all: &[Block],
    range: std::ops::Range<usize>,
) -> Result<Option<Plan>, TranslateError> {
    Ok(shape(decode, all, range)?.ok())
}

/// A segment's plan, or why its flow does not nest.
pub(crate) type Shaped = Result<Plan, String>;

/// [`plan`], with why a segment's flow does not nest where it does not.
///
/// # Errors
///
/// As [`plan`].
pub(crate) fn shape(
    decode: &Decode,
    all: &[Block],
    range: std::ops::Range<usize>,
) -> Result<Shaped, TranslateError> {
    let base = range.start;
    let n = range.len();
    let mut ends = Vec::with_capacity(n);
    let mut branch_at = Vec::with_capacity(n);
    for block in &all[range.clone()] {
        let to = |offset: u32| -> Result<To, TranslateError> {
            let index = blocks::block_at(all, offset).ok_or(TranslateError::Unsupported {
                offset: block.start,
                detail: "a successor is not the start of any block",
            })?;
            Ok(if range.contains(&index) {
                To::Node(index - base)
            } else {
                To::Leave
            })
        };
        ends.push(match block.terminator {
            Terminator::End => Ends::Goto(To::End),
            Terminator::Jump { target } | Terminator::Fallthrough { next: target } => {
                Ends::Goto(to(target)?)
            }
            Terminator::Branch {
                target,
                fallthrough,
            } => {
                let (taken, fall) = (to(target)?, to(fallthrough)?);
                if taken == fall {
                    Ends::Goto(taken)
                } else {
                    Ends::Branch { taken, fall }
                }
            }
        });
        branch_at.push(decode.instructions[block.end - 1].offset);
    }
    let mut shaper = Shaper {
        base,
        ends,
        branch_at,
        reachable: Vec::new(),
        idom: Vec::new(),
        rpo_index: Vec::new(),
        body: Vec::new(),
        ipdom: Vec::new(),
        loop_node: Vec::new(),
        next_of: vec![None; n],
        placed: vec![0; n],
        refused: None,
        escapes: Vec::new(),
    };
    if shaper.dominators().is_none() {
        return Ok(Err("a cycle is entered at more than one block".to_owned()));
    }
    if shaper.post_dominators().is_none() {
        return Ok(Err("a cycle is not a natural loop".to_owned()));
    }
    let Some(seq) = shaper.region(0, None, None) else {
        let why = shaper.refused.take();
        return Ok(Err(why.unwrap_or_else(|| "a jump does not nest".to_owned())));
    };
    // Every block the entry reaches placed once, and so every edge drawn once.
    if let Some(node) = (0..n).find(|&node| shaper.placed[node] != u8::from(shaper.reachable[node]))
    {
        return Ok(Err(format!("block {} is not placed", base + node)));
    }
    Ok(Ok(Plan { seq }))
}

/// `seq` with what follows each element that may escape to the block `code` indexes held under a
/// [`Node::Guard`] for it, at every depth outside loops: an escape skips the rest of each sequence it
/// leaves on its way out.
fn guard_after(seq: Vec<Node>, code: u32) -> Vec<Node> {
    let mut out = Vec::with_capacity(seq.len());
    let mut rest = seq.into_iter();
    while let Some(node) = rest.next() {
        let node = match node {
            Node::If {
                at,
                taken,
                fall,
                merged,
                reset,
            } => Node::If {
                at,
                taken: guard_after(taken, code),
                fall: guard_after(fall, code),
                merged,
                reset,
            },
            Node::Select {
                arms,
                otherwise,
                merged,
            } => Node::Select {
                arms: arms
                    .into_iter()
                    .map(|(c, arm)| (c, guard_after(arm, code)))
                    .collect(),
                otherwise: guard_after(otherwise, code),
                merged,
            },
            Node::Guard { code: own, body } => Node::Guard {
                code: own,
                body: guard_after(body, code),
            },
            other => other,
        };
        let escapes = escapes_to(&node, code);
        out.push(node);
        if escapes {
            let body: Vec<Node> = rest.collect();
            if !body.is_empty() {
                out.push(Node::Guard {
                    code,
                    body: guard_after(body, code),
                });
            }
            break;
        }
    }
    out
}

/// Whether `node` may escape to the block `code` indexes, through a selection's merge or a loop's
/// routing.
fn escapes_to(node: &Node, code: u32) -> bool {
    let any = |seq: &[Node]| seq.iter().any(|node| escapes_to(node, code));
    match node {
        Node::Jump(Jump::Escape { code: to }) => *to == code,
        Node::Jump(_) | Node::Block(_) => false,
        Node::If { taken, fall, .. } => any(taken) || any(fall),
        Node::Select {
            arms, otherwise, ..
        } => arms.iter().any(|(_, arm)| any(arm)) || any(otherwise),
        Node::Guard { body, .. } => any(body),
        Node::Loop { routes, .. } => routes
            .iter()
            .any(|(_, jump)| *jump == Jump::Escape { code }),
    }
}

/// Whether a jump to the segment's end sits inside a construct of the top level, which then needs
/// a loop around the segment to break out of.
fn needs_wrapper(seq: &[Node], tail: bool, budgeted: bool) -> bool {
    let last = seq.len().saturating_sub(1);
    seq.iter().enumerate().any(|(i, node)| match node {
        Node::Jump(Jump::Exit | Jump::Leave) => !(tail && i == last),
        Node::Jump(_) | Node::Block(_) => false,
        Node::If { taken, fall, .. } => {
            needs_wrapper(taken, false, budgeted) || needs_wrapper(fall, false, budgeted)
        }
        Node::Select {
            arms, otherwise, ..
        } => {
            arms.iter()
                .any(|(_, arm)| needs_wrapper(arm, false, budgeted))
                || needs_wrapper(otherwise, false, budgeted)
        }
        Node::Guard { body, .. } => needs_wrapper(body, false, budgeted),
        // Its merge routes the program's end on, unless that is all it does, last in the segment.
        Node::Loop { body, next, .. } => {
            (budgeted || exits(body)) && (*next || !(tail && i == last))
        }
    })
}

/// Whether `seq` holds a jump to the program's end, at any depth.
fn exits(seq: &[Node]) -> bool {
    seq.iter().any(|node| match node {
        Node::Jump(jump) => *jump == Jump::Exit,
        Node::Block(_) => false,
        Node::If { taken, fall, .. } => exits(taken) || exits(fall),
        Node::Select {
            arms, otherwise, ..
        } => arms.iter().any(|(_, arm)| exits(arm)) || exits(otherwise),
        Node::Guard { body, .. } | Node::Loop { body, .. } => exits(body),
    })
}

/// A construct being emitted.
enum Frame {
    Loop {
        merge: Id,
        cont: Id,
        /// Whether anything in it jumped to the program's end.
        exited: bool,
    },
    /// A selection, whose merge an escape takes ([`Jump::Escape`]).
    Selection { merge: Id },
}

struct Emitter<'a, F> {
    decode: &'a Decode,
    blocks: &'a [Block],
    family_of: &'a F,
    exit: u32,
    /// Where the segment's end is reached.
    done: Id,
    frames: Vec<Frame>,
    translated: usize,
}

/// Emits `plan`, and leaves the builder in a block after it. `first` where the segment is the
/// program's first; a later one runs only where the wave has not ended.
///
/// # Errors
///
/// An instruction the model refuses.
pub(crate) fn emit<M: Model + ?Sized>(
    model: &mut M,
    decode: &Decode,
    (blocks, plan, first): (&[Block], &Plan, bool),
    exit: u32,
    family_of: &impl Fn(&Instruction) -> Option<String>,
) -> Result<usize, TranslateError> {
    STRUCTURED.fetch_add(1, Ordering::Relaxed);
    let skip = (!first).then(|| {
        let (bool_type, end) = (model.bool_type(), model.constant(exit));
        let current = load_counter(model);
        let b = model.builder();
        let running = b.id();
        b.function(op::INOT_EQUAL, &[bool_type.0, running.0, current.0, end.0]);
        let (body, merge) = (b.id(), b.id());
        b.function(op::SELECTION_MERGE, &[merge.0, 0]);
        b.function(op::BRANCH_CONDITIONAL, &[running.0, body.0, merge.0]);
        b.function(op::LABEL, &[body.0]);
        merge
    });
    let budgeted = model.loop_budget().is_some();
    let done = model.builder().id();
    let wrapper = needs_wrapper(&plan.seq, true, budgeted).then(|| {
        // A loop that runs once, whose merge every jump to the segment's end breaks to.
        let b = model.builder();
        let (header, body, cont) = (b.id(), b.id(), b.id());
        b.function(op::BRANCH, &[header.0]);
        b.function(op::LABEL, &[header.0]);
        b.function(op::LOOP_MERGE, &[done.0, cont.0, 0]);
        b.function(op::BRANCH, &[body.0]);
        b.function(op::LABEL, &[body.0]);
        (header, cont)
    });
    let mut emitter = Emitter {
        decode,
        blocks,
        family_of,
        exit,
        done,
        frames: Vec::new(),
        translated: 0,
    };
    let open = emitter.sequence(model, &plan.seq)?;
    if open {
        model.builder().function(op::BRANCH, &[done.0]);
    }
    let b = model.builder();
    if let Some((header, cont)) = wrapper {
        // Never reached: the body breaks out every way.
        b.function(op::LABEL, &[cont.0]);
        b.function(op::BRANCH, &[header.0]);
    }
    b.function(op::LABEL, &[done.0]);
    if let Some(merge) = skip {
        b.function(op::BRANCH, &[merge.0]);
        b.function(op::LABEL, &[merge.0]);
    }
    Ok(emitter.translated)
}

impl<F: Fn(&Instruction) -> Option<String>> Emitter<'_, F> {
    /// Emits `seq`; answers whether control runs on past it.
    fn sequence<M: Model + ?Sized>(
        &mut self,
        model: &mut M,
        seq: &[Node],
    ) -> Result<bool, TranslateError> {
        for node in seq {
            match node {
                Node::Block(index) => self.block(model, *index)?,
                Node::If {
                    at,
                    taken,
                    fall,
                    merged,
                    reset,
                } => {
                    if *reset {
                        let neutral = model.constant(NEUTRAL);
                        store_counter(model, neutral);
                    }
                    let last = &self.decode.instructions[self.blocks[*at].end - 1];
                    let condition = control::branch_condition_value(model, last)?;
                    let b = model.builder();
                    let merge = b.id();
                    let label = |b: &mut orbistoun_spirv::Builder, arm: &[Node]| {
                        if arm.is_empty() { merge } else { b.id() }
                    };
                    let (on_taken, on_fall) = (label(b, taken), label(b, fall));
                    b.function(op::SELECTION_MERGE, &[merge.0, 0]);
                    b.function(
                        op::BRANCH_CONDITIONAL,
                        &[condition.0, on_taken.0, on_fall.0],
                    );
                    self.frames.push(Frame::Selection { merge });
                    for (arm, label) in [(taken, on_taken), (fall, on_fall)] {
                        if arm.is_empty() {
                            continue;
                        }
                        model.builder().function(op::LABEL, &[label.0]);
                        if self.sequence(model, arm)? {
                            model.builder().function(op::BRANCH, &[merge.0]);
                        }
                    }
                    self.frames.pop();
                    model.builder().function(op::LABEL, &[merge.0]);
                    if !merged {
                        model.builder().function(op::UNREACHABLE, &[]);
                        return Ok(false);
                    }
                }
                Node::Guard { code, body } => {
                    let (bool_type, value) = (model.bool_type(), model.constant(*code));
                    let current = load_counter(model);
                    let b = model.builder();
                    let runs = b.id();
                    b.function(op::INOT_EQUAL, &[bool_type.0, runs.0, current.0, value.0]);
                    let (start, merge) = (b.id(), b.id());
                    b.function(op::SELECTION_MERGE, &[merge.0, 0]);
                    b.function(op::BRANCH_CONDITIONAL, &[runs.0, start.0, merge.0]);
                    b.function(op::LABEL, &[start.0]);
                    self.frames.push(Frame::Selection { merge });
                    if self.sequence(model, body)? {
                        model.builder().function(op::BRANCH, &[merge.0]);
                    }
                    self.frames.pop();
                    model.builder().function(op::LABEL, &[merge.0]);
                }
                Node::Loop {
                    body,
                    routes,
                    next,
                    reset,
                } => {
                    if *reset {
                        let neutral = model.constant(NEUTRAL);
                        store_counter(model, neutral);
                    }
                    if !self.looped(model, body, routes, *next)? {
                        return Ok(false);
                    }
                }
                Node::Select {
                    arms,
                    otherwise,
                    merged,
                } => {
                    if !self.select(model, arms, otherwise, *merged)? {
                        return Ok(false);
                    }
                }
                Node::Jump(jump) => {
                    self.jump(model, *jump);
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn block<M: Model + ?Sized>(
        &mut self,
        model: &mut M,
        index: usize,
    ) -> Result<(), TranslateError> {
        let block = &self.blocks[index];
        model.enter_block();
        let before = model.instructions();
        let family_of = self.family_of;
        let instructions = self.decode.instructions[block.first..block.end]
            .iter()
            .filter(|instruction| !control::is_terminator(instruction, family_of));
        control::emit_block(model, instructions)?;
        self.translated += model.instructions() - before;
        Ok(())
    }

    /// Emits a loop and its merge's routing; answers whether control runs on past it.
    fn looped<M: Model + ?Sized>(
        &mut self,
        model: &mut M,
        body: &[Node],
        routes: &[(u32, Jump)],
        next: bool,
    ) -> Result<bool, TranslateError> {
        let b = model.builder();
        let (header, start, cont, merge) = (b.id(), b.id(), b.id(), b.id());
        b.function(op::BRANCH, &[header.0]);
        b.function(op::LABEL, &[header.0]);
        b.function(op::LOOP_MERGE, &[merge.0, cont.0, 0]);
        b.function(op::BRANCH, &[start.0]);
        b.function(op::LABEL, &[start.0]);
        self.frames.push(Frame::Loop {
            merge,
            cont,
            exited: false,
        });
        let open = self.sequence(model, body)?;
        let exited = matches!(self.frames.pop(), Some(Frame::Loop { exited: true, .. }));
        let b = model.builder();
        if open {
            // A body runs to a jump every way; one that did not would continue.
            b.function(op::BRANCH, &[cont.0]);
        }
        b.function(op::LABEL, &[cont.0]);
        b.function(op::BRANCH, &[header.0]);
        b.function(op::LABEL, &[merge.0]);

        let mut all: Vec<(u32, Jump)> = Vec::new();
        if exited {
            all.push((self.exit, Jump::Exit));
        }
        all.extend_from_slice(routes);
        let last = all.len().saturating_sub(1);
        for (i, (code, jump)) in all.iter().enumerate() {
            if i == last && !next {
                self.jump(model, *jump);
                return Ok(false);
            }
            let (bool_type, value) = (model.bool_type(), model.constant(*code));
            let current = load_counter(model);
            let b = model.builder();
            let hit = b.id();
            b.function(op::IEQUAL, &[bool_type.0, hit.0, current.0, value.0]);
            let (arm, after) = (b.id(), b.id());
            b.function(op::SELECTION_MERGE, &[after.0, 0]);
            b.function(op::BRANCH_CONDITIONAL, &[hit.0, arm.0, after.0]);
            b.function(op::LABEL, &[arm.0]);
            self.frames.push(Frame::Selection { merge: after });
            self.jump(model, *jump);
            self.frames.pop();
            model.builder().function(op::LABEL, &[after.0]);
        }
        if !next && all.is_empty() {
            model.builder().function(op::UNREACHABLE, &[]);
            return Ok(false);
        }
        Ok(true)
    }

    /// Emits a choice among `arms` by the counter, else `otherwise`, as one selection inside the
    /// other; answers whether control runs on past it.
    fn select<M: Model + ?Sized>(
        &mut self,
        model: &mut M,
        arms: &[(u32, Vec<Node>)],
        otherwise: &[Node],
        merged: bool,
    ) -> Result<bool, TranslateError> {
        let Some(((code, arm), rest)) = arms.split_first() else {
            return self.sequence(model, otherwise);
        };
        let (bool_type, value) = (model.bool_type(), model.constant(*code));
        let current = load_counter(model);
        let b = model.builder();
        let hit = b.id();
        b.function(op::IEQUAL, &[bool_type.0, hit.0, current.0, value.0]);
        let merge = b.id();
        let on_hit = if arm.is_empty() { merge } else { b.id() };
        let on_miss = if rest.is_empty() && otherwise.is_empty() {
            merge
        } else {
            b.id()
        };
        b.function(op::SELECTION_MERGE, &[merge.0, 0]);
        b.function(op::BRANCH_CONDITIONAL, &[hit.0, on_hit.0, on_miss.0]);
        self.frames.push(Frame::Selection { merge });
        if on_hit != merge {
            model.builder().function(op::LABEL, &[on_hit.0]);
            if self.sequence(model, arm)? {
                model.builder().function(op::BRANCH, &[merge.0]);
            }
        }
        if on_miss != merge {
            model.builder().function(op::LABEL, &[on_miss.0]);
            if self.select(model, rest, otherwise, merged)? {
                model.builder().function(op::BRANCH, &[merge.0]);
            }
        }
        self.frames.pop();
        model.builder().function(op::LABEL, &[merge.0]);
        if !merged {
            model.builder().function(op::UNREACHABLE, &[]);
        }
        Ok(merged)
    }

    /// Emits `jump`, which ends the current block.
    fn jump<M: Model + ?Sized>(&mut self, model: &mut M, jump: Jump) {
        match jump {
            Jump::Continue { branch } => {
                if let Some(budget) = model.loop_budget() {
                    let cut = count_jump(model, budget);
                    model.note_loop_cut(cut, branch);
                    let b = model.builder();
                    let (stop, on) = (b.id(), b.id());
                    b.function(op::SELECTION_MERGE, &[on.0, 0]);
                    b.function(op::BRANCH_CONDITIONAL, &[cut.0, stop.0, on.0]);
                    b.function(op::LABEL, &[stop.0]);
                    // Cut: the program's end, as `s_endpgm` (D949).
                    self.jump(model, Jump::Exit);
                    model.builder().function(op::LABEL, &[on.0]);
                }
                let (_, cont) = self
                    .innermost_loop()
                    .expect("a continue sits inside a loop");
                model.builder().function(op::BRANCH, &[cont.0]);
            }
            Jump::Break { code, next } => {
                if !next {
                    let value = model.constant(code);
                    store_counter(model, value);
                }
                let (merge, _) = self.innermost_loop().expect("a break sits inside a loop");
                model.builder().function(op::BRANCH, &[merge.0]);
            }
            Jump::Exit => {
                let end = model.constant(self.exit);
                store_counter(model, end);
                let target = self.frames.iter_mut().rev().find_map(|frame| match frame {
                    Frame::Loop { merge, exited, .. } => {
                        *exited = true;
                        Some(*merge)
                    }
                    Frame::Selection { .. } => None,
                });
                model
                    .builder()
                    .function(op::BRANCH, &[target.unwrap_or(self.done).0]);
            }
            Jump::Leave => {
                model.builder().function(op::BRANCH, &[self.done.0]);
            }
            Jump::Escape { code } => {
                let value = model.constant(code);
                store_counter(model, value);
                let merge = match self.frames.last() {
                    Some(Frame::Selection { merge }) => *merge,
                    _ => unreachable!("an escape sits directly inside a selection"),
                };
                model.builder().function(op::BRANCH, &[merge.0]);
            }
        }
    }

    /// The innermost loop's merge and continue target.
    fn innermost_loop(&self) -> Option<(Id, Id)> {
        self.frames.iter().rev().find_map(|frame| match frame {
            Frame::Loop { merge, cont, .. } => Some((*merge, *cont)),
            Frame::Selection { .. } => None,
        })
    }
}

/// Counts one backward jump against the invocation's budget, and answers whether it passed the cap
/// (D949).
fn count_jump<M: Model + ?Sized>(model: &mut M, budget: Id) -> Id {
    let (u32_type, bool_type) = (model.u32_type(), model.bool_type());
    let one = model.constant(1);
    let cap = model.loop_cap();
    let cap = model.constant(cap);
    let b = model.builder();
    let before = b.id();
    b.function(op::LOAD, &[u32_type.0, before.0, budget.0]);
    let after = b.id();
    b.function(op::IADD, &[u32_type.0, after.0, before.0, one.0]);
    b.function(op::STORE, &[budget.0, after.0]);
    let cut = b.id();
    b.function(op::UGREATER_THAN, &[bool_type.0, cut.0, after.0, cap.0]);
    cut
}

fn load_counter<M: Model + ?Sized>(model: &mut M) -> Id {
    let (pointer, u32_type) = (model.program_counter(), model.u32_type());
    let b = model.builder();
    let value = b.id();
    b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
    value
}

fn store_counter<M: Model + ?Sized>(model: &mut M, value: Id) {
    let pointer = model.program_counter();
    model.builder().function(op::STORE, &[pointer.0, value.0]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbistoun_shader::{EncodingTable, OperandTable, decode};
    use std::fmt::Write as _;

    /// `s_branch` and friends: SOPP, opcode at bit 16, a signed word offset in the low half, from
    /// the instruction at `at` to the one at `to`, every instruction one word.
    fn branch(opcode: u32, at: usize, to: usize) -> u32 {
        let offset = i16::try_from(to as isize - (at as isize + 1)).expect("a short branch");
        0xBF80_0000 | (opcode << 16) | u32::from(offset as u16)
    }
    const JUMP: u32 = 2;
    const SCC1: u32 = 5;
    const EXECZ: u32 = 8;
    const ENDPGM: u32 = 0xBF81_0000;
    /// `v_mov_b32_e32 v0, 0`: a block's body.
    const NOP: u32 = 0x7E00_0280;

    /// The plan for `words`, as a line naming its shape: `B<n>` a block, `if(..|..)` a selection
    /// (`if!` one with no merge), `loop[..]` a loop with its routes, and the jumps.
    fn shape(words: &[u32]) -> Option<String> {
        let table = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &table, &operands);
        assert!(decoded.is_trustworthy());
        let names: Vec<String> = table.encodings().iter().map(|e| e.name.clone()).collect();
        let family_of =
            |i: &Instruction| i.encoding.and_then(|e| names.get(usize::from(e)).cloned());
        let all = blocks::split(&decoded, family_of).expect("split");
        let plan = plan(&decoded, &all, 0..all.len()).expect("plan")?;
        Some(describe(&plan.seq))
    }

    fn describe(seq: &[Node]) -> String {
        let parts: Vec<String> = seq
            .iter()
            .map(|node| match node {
                Node::Block(n) => format!("B{n}"),
                Node::If {
                    taken,
                    fall,
                    merged,
                    ..
                } => format!(
                    "if{}({}|{})",
                    if *merged { "" } else { "!" },
                    describe(taken),
                    describe(fall)
                ),
                Node::Guard { code, body } => format!("unless{code}({})", describe(body)),
                Node::Select {
                    arms,
                    otherwise,
                    merged,
                } => format!(
                    "sel{}({}{})",
                    if *merged { "" } else { "!" },
                    arms.iter().fold(String::new(), |mut out, (code, arm)| {
                        let _ = write!(out, "@{code}:{}|", describe(arm));
                        out
                    }),
                    describe(otherwise)
                ),
                Node::Loop {
                    body, routes, next, ..
                } => format!(
                    "loop[{}]{}{}",
                    describe(body),
                    routes.iter().fold(String::new(), |mut out, (code, jump)| {
                        let _ = write!(out, "@{code}:{}", jump_name(*jump));
                        out
                    }),
                    if *next { "" } else { "!" }
                ),
                Node::Jump(jump) => jump_name(*jump),
            })
            .collect();
        parts.join(" ")
    }

    fn jump_name(jump: Jump) -> String {
        match jump {
            Jump::Continue { .. } => "continue".to_owned(),
            Jump::Break { code, next: true } => format!("break{code}"),
            Jump::Break { code, next: false } => format!("break{code}*"),
            Jump::Exit => "exit".to_owned(),
            Jump::Leave => "leave".to_owned(),
            Jump::Escape { code } => format!("escape{code}"),
        }
    }

    /// Each guest program in the directory `ORBISTOUN_SHAPE_PROGRAMS` names, laid out or refused with
    /// why. A guest's code is never kept in the tree, so without the variable this is skipped.
    #[test]
    fn guest_programs_report_their_shape() {
        let Some(directory) = std::env::var_os("ORBISTOUN_SHAPE_PROGRAMS") else {
            println!("[shapes] SKIPPED - ORBISTOUN_SHAPE_PROGRAMS names no directory");
            return;
        };
        let table = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let names: Vec<String> = table.encodings().iter().map(|e| e.name.clone()).collect();
        let family_of =
            |i: &Instruction| i.encoding.and_then(|e| names.get(usize::from(e)).cloned());
        for entry in std::fs::read_dir(directory).expect("the directory reads") {
            let path = entry.expect("an entry").path();
            let bytes = std::fs::read(&path).expect("the program reads");
            let decoded = orbistoun_shader::decode_program(&bytes, &table, &operands);
            let all = blocks::split(&decoded, family_of).expect("split");
            let found = super::shape(&decoded, &all, 0..all.len()).expect("shape");
            println!(
                "[shapes] {}: {} blocks, {}",
                path.display(),
                all.len(),
                match found {
                    Ok(plan) => format!("structured: {}", describe(&plan.seq)),
                    Err(why) => format!("refused: {why}"),
                }
            );
        }
    }

    #[test]
    fn a_straight_program_is_one_block_and_its_end() {
        assert_eq!(shape(&[NOP, ENDPGM]).as_deref(), Some("B0 exit"));
    }

    #[test]
    fn a_skip_is_a_selection_merging_at_its_target() {
        // B0 skips B1 to B2.
        let words = [NOP, branch(EXECZ, 1, 3), NOP, ENDPGM];
        assert_eq!(shape(&words).as_deref(), Some("B0 if(|B1) B2 exit"));
    }

    #[test]
    fn an_if_else_is_a_selection_with_two_arms() {
        //   0 scc1 -> 3; 1 nop; 2 jump -> 4; 3 nop; 4 end
        let words = [branch(SCC1, 0, 3), NOP, branch(JUMP, 2, 4), NOP, ENDPGM];
        assert_eq!(shape(&words).as_deref(), Some("B0 if(B2|B1) B3 exit"));
    }

    #[test]
    fn a_backward_branch_is_a_loop_merging_after_it() {
        //   0 nop; 1 nop; 2 scc1 -> 1; 3 end
        let words = [NOP, NOP, branch(SCC1, 2, 1), ENDPGM];
        assert_eq!(
            shape(&words).as_deref(),
            Some("B0 loop[B1 if!(continue|break2)] B2 exit")
        );
    }

    #[test]
    fn a_loop_left_for_the_end_breaks_and_its_merge_runs_on() {
        //   0 nop; 1 nop (loop); 2 scc1 -> 6; 3 nop; 4 scc1 -> 1; 5 end; 6 end
        let words = [
            NOP,
            NOP,
            branch(SCC1, 2, 6),
            NOP,
            branch(SCC1, 4, 1),
            ENDPGM,
            ENDPGM,
        ];
        // The early end is a second block the loop leaves for: the first it runs on into, the
        // other its merge selects by the counter, which the break to it writes.
        assert_eq!(
            shape(&words).as_deref(),
            Some("B0 loop[B1 if!(break4*|B2 if!(continue|break3))] sel!(@4:B4 exit|B3 exit)")
        );
    }

    #[test]
    fn a_loop_leaving_early_for_where_it_runs_out_breaks_there() {
        //   0 nop; 1 nop (loop); 2 scc1 -> 4; 3 scc1 -> 1; 4 end
        let words = [NOP, NOP, branch(SCC1, 2, 4), branch(SCC1, 3, 1), ENDPGM];
        assert_eq!(
            shape(&words).as_deref(),
            Some("B0 loop[B1 if!(break3|B2 if!(continue|break3))] B3 exit")
        );
    }

    #[test]
    fn an_arm_skipping_past_the_join_escapes_and_the_join_is_guarded() {
        //   0 scc1 -> 5         the else arm
        //   1 nop
        //   2 execz -> 7        past the join, to the merge
        //   3 nop
        //   4 jump -> 6         to the join
        //   5 nop               the else arm
        //   6 nop               the join
        //   7 end               the merge
        let words = [
            branch(SCC1, 0, 5),
            NOP,
            branch(EXECZ, 2, 7),
            NOP,
            branch(JUMP, 4, 6),
            NOP,
            NOP,
            ENDPGM,
        ];
        assert_eq!(
            shape(&words).as_deref(),
            Some("B0 if(B3|B1 if(escape5|B2)) unless5(B4) B5 exit")
        );
    }

    #[test]
    fn an_irreducible_cycle_has_no_plan() {
        //   0 scc1 -> 3; 1 nop (A); 2 nop; 3 nop (B); 4 jump -> 1
        let words = [
            branch(SCC1, 0, 3),
            NOP,
            NOP,
            NOP,
            branch(JUMP, 4, 1),
            ENDPGM,
        ];
        assert_eq!(shape(&words), None);
    }

    #[test]
    fn an_inner_loop_continuing_the_outer_routes_from_its_merge() {
        //   0 nop
        //   1 scc1 -> 7       outer header: leave
        //   2 nop
        //   3 nop             inner header
        //   4 scc1 -> 1       continue the outer loop
        //   5 jump -> 3       continue the inner
        //   6 (unreachable) nop
        //   7 end
        let words = [
            NOP,
            branch(SCC1, 1, 7),
            NOP,
            NOP,
            branch(SCC1, 4, 1),
            branch(JUMP, 5, 3),
            NOP,
            ENDPGM,
        ];
        assert_eq!(
            shape(&words).as_deref(),
            Some(
                "B0 loop[B1 if!(break6|B2 loop[B3 if!(break1*|B4 continue)]@1:continue!)] B6 exit"
            )
        );
    }
}
