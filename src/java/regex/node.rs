//! The compiled form of a pattern: a graph of nodes held in one arena.

use super::charclass::CharClass;

/// Index of a node in [`Program::nodes`].
pub(super) type NodeId = u32;

/// Ends a sub-match (quantified atom, look-around, atomic group) successfully.
pub(super) const ACCEPT: NodeId = 0;
/// Ends the whole match: it succeeds only at the end of the input.
pub(super) const LAST_ACCEPT: NodeId = 1;

/// The largest repetition count, standing for "unbounded".
pub(super) const MAX_REPS: i32 = i32::MAX;

/// How literal text is compared with the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fold {
    None,
    Ascii,
    Unicode,
}

/// Backtracking discipline of a quantifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Quantifier {
    Greedy,
    Lazy,
    Possessive,
    /// Exactly once, never revisited: the body of `(?>...)`.
    Atomic,
}

/// What a node tests or does before continuing with its `next`.
#[derive(Clone, Debug)]
pub(super) enum Kind {
    Accept,
    LastAccept,
    LookBehindEnd,
    /// `\A`, and `^` without `MULTILINE`.
    Begin,
    /// `\z`.
    End,
    /// `^` under `MULTILINE`.
    Caret,
    /// `^` under `MULTILINE | UNIX_LINES`.
    UnixCaret,
    /// `$` and `\Z`.
    Dollar {
        multiline: bool,
    },
    /// `$` and `\Z` under `UNIX_LINES`.
    UnixDollar {
        multiline: bool,
    },
    /// `\G`.
    LastMatch,
    /// `\b` and, when `negated`, `\B`.
    WordBoundary {
        negated: bool,
        unicode: bool,
    },
    /// `\b{g}`.
    GraphemeBoundary,
    /// `\R`.
    LineEnding,
    /// `\X`.
    Grapheme,
    /// One character.
    Char(CharClass),
    /// `class{min,}` with the greedy discipline; its atom never needs a sub-match.
    CharRun {
        class: CharClass,
        min: i32,
    },
    /// Literal text; `wide` compares code points, otherwise UTF-16 units.
    Slice {
        text: Vec<u32>,
        wide: bool,
        fold: Fold,
    },
    BackRef {
        group: u32,
        fold: Fold,
    },
    /// Records where a group begins. `slot` indexes the matcher's locals.
    GroupHead {
        slot: u32,
    },
    /// Closes a group, recording it when `group` is set.
    GroupTail {
        slot: u32,
        group: Option<u32>,
    },
    Branch {
        /// Alternatives in order; `None` is an empty alternative.
        atoms: Vec<Option<NodeId>>,
        conn: NodeId,
    },
    /// Where the alternatives of a [`Kind::Branch`] join.
    BranchConn,
    /// `atom?`, and the atomic group.
    Ques {
        atom: NodeId,
        quantifier: Quantifier,
    },
    /// Repetition of an atom whose iterations are matched independently.
    Curly {
        atom: NodeId,
        min: i32,
        max: i32,
        quantifier: Quantifier,
    },
    /// Repetition of a group the compiler proved deterministic.
    GroupCurly {
        atom: NodeId,
        min: i32,
        max: i32,
        lazy: bool,
        group: Option<u32>,
    },
    /// First entry into a [`Kind::Loop`].
    LoopEntry {
        target: NodeId,
    },
    /// Repetition of an arbitrary group; reached from the end of the body.
    Loop {
        body: NodeId,
        /// Local holding the start of the current iteration.
        begin: u32,
        /// Local holding the iteration count.
        count: u32,
        min: i32,
        max: i32,
        lazy: bool,
        /// Index of the set of already failed positions, if the loop keeps one.
        memo: Option<u32>,
    },
    Lookahead {
        cond: NodeId,
        negated: bool,
    },
    Lookbehind {
        cond: NodeId,
        min: i32,
        max: i32,
        negated: bool,
        /// Distances are counted in code points rather than UTF-16 units.
        wide: bool,
    },
}

/// A node with its successor.
#[derive(Clone, Debug)]
pub(super) struct Node {
    pub(super) kind: Kind,
    pub(super) next: NodeId,
}

/// A compiled pattern.
#[derive(Clone, Debug)]
pub(super) struct Program {
    pub(super) nodes: Vec<Node>,
    pub(super) root: NodeId,
    /// Number of capturing groups including group 0.
    pub(super) group_count: u32,
    pub(super) local_count: u32,
    pub(super) memo_count: u32,
}
