//! Static analysis of node chains: length bounds and determinism.
//!
//! The results decide how a quantified group is compiled and whether a look-behind is legal, and
//! the arithmetic wraps like Java's `int`, because both are observable.

use super::node::{Kind, Node, NodeId, Quantifier, MAX_REPS};

/// What is known about a chain of nodes.
#[derive(Clone, Copy, Debug)]
pub(super) struct TreeInfo {
    pub(super) min_length: i32,
    pub(super) max_length: i32,
    pub(super) max_valid: bool,
    pub(super) deterministic: bool,
}

impl TreeInfo {
    /// The facts about an empty chain.
    pub(super) fn new() -> TreeInfo {
        TreeInfo { min_length: 0, max_length: 0, max_valid: true, deterministic: true }
    }

    fn add_length(&mut self, min: i32, max: i32) {
        self.min_length = self.min_length.wrapping_add(min);
        self.max_length = self.max_length.wrapping_add(max);
    }
}

/// Accumulates into `info` the facts about the chain starting at `start`.
pub(super) fn study_chain(nodes: &[Node], start: NodeId, info: &mut TreeInfo) {
    let mut before_branch = Vec::new();
    let mut id = start;
    while let Some(node) = nodes.get(id as usize) {
        match &node.kind {
            Kind::Accept | Kind::LastAccept | Kind::BranchConn => break,
            Kind::Char(_) => info.add_length(1, 1),
            Kind::CharRun { min, .. } => {
                info.min_length = info.min_length.wrapping_add(*min);
                if info.max_valid {
                    info.max_length = info.max_length.wrapping_add(MAX_REPS);
                }
                info.deterministic = false;
            }
            Kind::Slice { text, .. } => {
                let length = i32::try_from(text.len()).unwrap_or(MAX_REPS);
                info.add_length(length, length);
            }
            Kind::BackRef { .. } => info.max_valid = false,
            Kind::LineEnding => info.add_length(1, 2),
            Kind::Grapheme => {
                info.min_length = info.min_length.wrapping_add(1);
                info.deterministic = false;
            }
            Kind::Branch { atoms, conn } => {
                before_branch.push(study_alternatives(nodes, atoms, info));
                *info = TreeInfo::new();
                id = nodes.get(*conn as usize).map_or(*conn, |conn| conn.next);
                continue;
            }
            Kind::Ques { atom, quantifier } => {
                if *quantifier == Quantifier::Atomic {
                    study_chain(nodes, *atom, info);
                } else {
                    let min_length = info.min_length;
                    study_chain(nodes, *atom, info);
                    info.min_length = min_length;
                    info.deterministic = false;
                }
            }
            Kind::Curly { atom, min, max, .. } | Kind::GroupCurly { atom, min, max, .. } => {
                study_repetition(nodes, *atom, *min, *max, info)
            }
            Kind::LoopEntry { .. } | Kind::Loop { .. } => {
                info.max_valid = false;
                info.deterministic = false;
                break;
            }
            _ => {}
        }
        id = node.next;
    }
    for (min, max, max_valid) in before_branch.into_iter().rev() {
        info.add_length(min, max);
        info.max_valid &= max_valid;
        info.deterministic = false;
    }
}

/// Studies the alternatives of a branch; returns the totals up to and including it.
fn study_alternatives(nodes: &[Node], atoms: &[Option<NodeId>], info: &TreeInfo) -> (i32, i32, bool) {
    let mut shortest = i32::MAX;
    let mut longest = -1;
    let mut max_valid = info.max_valid;
    for atom in atoms {
        let mut alternative = TreeInfo::new();
        if let Some(atom) = atom {
            study_chain(nodes, *atom, &mut alternative);
        }
        shortest = shortest.min(alternative.min_length);
        longest = longest.max(alternative.max_length);
        max_valid &= alternative.max_valid;
    }
    (info.min_length.wrapping_add(shortest), info.max_length.wrapping_add(longest), max_valid)
}

fn study_repetition(nodes: &[Node], atom: NodeId, min: i32, max: i32, info: &mut TreeInfo) {
    let before = *info;
    *info = TreeInfo::new();
    study_chain(nodes, atom, info);

    let total = info.min_length.wrapping_mul(min).wrapping_add(before.min_length);
    info.min_length = if total < before.min_length { 0xFFF_FFFF } else { total };

    if before.max_valid && info.max_valid {
        let total = info.max_length.wrapping_mul(max).wrapping_add(before.max_length);
        info.max_length = total;
        if total < before.max_length {
            info.max_valid = false;
        }
    } else {
        info.max_valid = false;
    }

    info.deterministic = info.deterministic && min == max && before.deterministic;
}
