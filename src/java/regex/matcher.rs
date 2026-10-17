//! The backtracking engine behind `Matcher.matches()`.
//!
//! Matching is a depth-first walk of the node graph. Java does it with the call stack; here every
//! pending decision is a [`Frame`] on an explicit stack, so input length cannot exhaust the native
//! stack. A step either enters a node ([`Step::Enter`]) or reports success or failure to the top
//! frame ([`Step::Return`]). Frames see both outcomes, which matters: a sub-match that succeeded
//! leaves its captures behind even when the code after it fails, and Java's quirks in that area
//! are observable through back-references.

use super::charclass::{self, CharClass};
use super::node::{Fold, Kind, Node, NodeId, Program, Quantifier};
use super::{property, unicode, Thrown};
use std::collections::HashSet;

type Stepped = Result<Step, Thrown>;

/// What the engine does next.
#[derive(Clone, Copy, Debug)]
enum Step {
    Enter(NodeId, i32),
    Return(bool),
}

/// The fields of an in-progress greedy repetition.
#[derive(Clone, Copy, Debug)]
struct Greedy {
    node: NodeId,
    i: i32,
    /// Iterations so far.
    count: i32,
    /// Length of the iterations being repeated.
    stride: i32,
    /// Count that may not be backed off past.
    floor: i32,
}

/// The value of a capture group: start and end.
type Span = (i32, i32);

/// A decision or clean-up waiting for the outcome of a sub-walk.
#[derive(Clone, Copy, Debug)]
enum Frame {
    /// Restore a local on any outcome.
    RestoreLocal {
        slot: u32,
        value: i32,
    },
    /// Restore a local on failure.
    RestoreCount {
        slot: u32,
        value: i32,
    },
    /// Restore a group on failure.
    RestoreGroup {
        group: u32,
        span: Span,
    },
    /// On failure, try the next alternative.
    Branch {
        node: NodeId,
        alternative: u32,
        i: i32,
    },
    /// On failure, continue at `next` from `i`.
    OrElse {
        next: NodeId,
        i: i32,
    },
    /// `atom?`: the atom has run, the continuation is next.
    QuesAtom {
        next: NodeId,
        i: i32,
    },
    /// `atom??`: the continuation failed, the atom is next.
    QuesLazy {
        node: NodeId,
        i: i32,
    },
    /// `atom?+`: continue wherever the atom got to.
    QuesPossessive {
        next: NodeId,
        i: i32,
    },
    /// The atom must succeed; continue after it.
    AtomThen {
        next: NodeId,
    },
    Lookahead {
        next: NodeId,
        i: i32,
        negated: bool,
    },
    Lookbehind {
        node: NodeId,
        i: i32,
        j: i32,
        from: i32,
        saved_end: i32,
    },
    /// Mandatory iterations of a [`Kind::Curly`].
    CurlyMin {
        node: NodeId,
        count: i32,
    },
    CurlyFirst(Greedy),
    CurlyStep(Greedy),
    CurlyRecurse(Greedy),
    CurlyBack(Greedy),
    /// The continuation of a lazy repetition failed; one more iteration may help.
    CurlyLazy {
        node: NodeId,
        i: i32,
        count: i32,
    },
    CurlyLazyAtom {
        node: NodeId,
        i: i32,
        count: i32,
    },
    CurlyPossessive {
        node: NodeId,
        i: i32,
        count: i32,
    },
    /// Mandatory iterations of a [`Kind::GroupCurly`].
    GroupMin {
        node: NodeId,
        i: i32,
        count: i32,
    },
    GroupFirst(Greedy, Span),
    GroupStep(Greedy, Span),
    GroupRecurse(Greedy, Span),
    GroupBack(Greedy, Span),
    GroupLazy {
        node: NodeId,
        i: i32,
        count: i32,
    },
    GroupLazyAtom {
        node: NodeId,
        i: i32,
        count: i32,
    },
    /// A character run backing off.
    RunBack {
        node: NodeId,
        i: i32,
        count: i32,
    },
    /// A lazy loop's first continuation failed; try the body.
    LoopEntryLazy {
        node: NodeId,
        i: i32,
    },
    /// An optional iteration of a greedy loop failed.
    LoopBody {
        node: NodeId,
        i: i32,
        count: i32,
    },
    /// The continuation of a lazy loop failed.
    LoopLazy {
        node: NodeId,
        i: i32,
        count: i32,
    },
}

/// Runs `program` against the whole of `text`.
pub(super) fn matches(program: &Program, text: &[u16]) -> Result<bool, Thrown> {
    let to = i32::try_from(text.len())
        .map_err(|_| Thrown::new("java.lang.OutOfMemoryError", "Required length exceeds implementation limit"))?;
    let group_slots = program.group_count.max(10) as usize * 2;
    let mut matcher = Matcher {
        program,
        text,
        to,
        groups: vec![-1; group_slots],
        locals: vec![-1; program.local_count as usize],
        memos: vec![HashSet::new(); program.memo_count as usize],
        last: 0,
        lookbehind_end: 0,
        stack: Vec::new(),
    };
    matcher.run()
}

struct Matcher<'a> {
    program: &'a Program,
    text: &'a [u16],
    to: i32,
    /// Start and end of each group; `-1` while unset.
    groups: Vec<i32>,
    /// Group starts and loop counts.
    locals: Vec<i32>,
    /// Failed positions of the loops that remember them.
    memos: Vec<HashSet<i32>>,
    /// End of the latest successful sub-match.
    last: i32,
    lookbehind_end: i32,
    stack: Vec<Frame>,
}

fn is_high_surrogate(unit: u16) -> bool {
    (0xD800..=0xDBFF).contains(&unit)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

fn char_count(cp: u32) -> i32 {
    if cp >= 0x10000 {
        2
    } else {
        1
    }
}

fn pair(high: u16, low: u16) -> u32 {
    0x10000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(low) - 0xDC00)
}

/// Grapheme extenders, as far as general categories tell.
fn extends_grapheme(cp: u32) -> bool {
    use super::unicode::category;
    matches!(cp, 0x200C | 0x200D)
        || unicode::has_category(cp, category::NON_SPACING_MARK | category::ENCLOSING_MARK | category::SPACING_MARK)
}

/// A simplified grapheme cluster boundary: no break inside `\r\n` or before extenders.
fn is_grapheme_boundary(before: u32, after: u32) -> bool {
    use super::unicode::category;
    if before == 0x0D && after == 0x0A {
        return false;
    }
    let control = category::CONTROL | category::LINE_SEPARATOR | category::PARAGRAPH_SEPARATOR;
    unicode::has_category(before, control) || unicode::has_category(after, control) || !extends_grapheme(after)
}

impl Matcher<'_> {
    fn run(&mut self) -> Result<bool, Thrown> {
        let mut step = Step::Enter(self.program.root, 0);
        loop {
            step = match step {
                Step::Enter(id, i) => self.enter(id, i)?,
                Step::Return(outcome) => match self.stack.pop() {
                    Some(frame) => self.resume(frame, outcome)?,
                    None => return Ok(outcome),
                },
            };
        }
    }

    fn node(&self, id: NodeId) -> &Node {
        self.program.nodes.get(id as usize).unwrap_or(&ACCEPT_NODE)
    }

    fn next_of(&self, id: NodeId) -> NodeId {
        self.node(id).next
    }

    /// `seq.charAt(index)`.
    fn unit(&self, index: i32) -> Result<u16, Thrown> {
        usize::try_from(index).ok().and_then(|position| self.text.get(position).copied()).ok_or_else(|| {
            let length = self.text.len();
            let message = format!("Index {index} out of bounds for length {length}");
            Thrown::new("java.lang.StringIndexOutOfBoundsException", &message)
        })
    }

    /// `Character.codePointAt(seq, index)`.
    fn code_point_at(&self, index: i32) -> Result<u32, Thrown> {
        let unit = self.unit(index)?;
        if is_high_surrogate(unit) && index + 1 < self.to {
            let low = self.unit(index + 1)?;
            if is_low_surrogate(low) {
                return Ok(pair(unit, low));
            }
        }
        Ok(u32::from(unit))
    }

    /// `Character.codePointBefore(seq, index)`.
    fn code_point_before(&self, index: i32) -> Result<u32, Thrown> {
        let unit = self.unit(index - 1)?;
        if is_low_surrogate(unit) && index >= 2 {
            let high = self.unit(index - 2)?;
            if is_high_surrogate(high) {
                return Ok(pair(high, unit));
            }
        }
        Ok(u32::from(unit))
    }

    fn local(&self, slot: u32) -> i32 {
        self.locals.get(slot as usize).copied().unwrap_or(-1)
    }

    fn set_local(&mut self, slot: u32, value: i32) {
        if let Some(local) = self.locals.get_mut(slot as usize) {
            *local = value;
        }
    }

    fn group(&self, group: u32) -> Span {
        let index = group as usize * 2;
        (self.groups.get(index).copied().unwrap_or(-1), self.groups.get(index + 1).copied().unwrap_or(-1))
    }

    fn set_group(&mut self, group: u32, span: Span) {
        if let Some(slots) = self.groups.get_mut(group as usize * 2..group as usize * 2 + 2) {
            slots.copy_from_slice(&[span.0, span.1]);
        }
    }

    fn set_optional_group(&mut self, group: Option<u32>, span: Span) {
        if let Some(group) = group {
            self.set_group(group, span);
        }
    }

    fn enter(&mut self, id: NodeId, i: i32) -> Stepped {
        let program = self.program;
        let node = match program.nodes.get(id as usize) {
            Some(node) => node,
            None => return Ok(Step::Return(false)),
        };
        let next = node.next;
        let passed = |condition: bool| {
            if condition {
                Step::Enter(next, i)
            } else {
                Step::Return(false)
            }
        };
        Ok(match &node.kind {
            Kind::Accept => {
                self.last = i;
                Step::Return(true)
            }
            Kind::LastAccept => {
                if i == self.to {
                    self.last = i;
                }
                Step::Return(i == self.to)
            }
            Kind::LookBehindEnd => Step::Return(i == self.lookbehind_end),
            Kind::Begin | Kind::LastMatch => passed(i == 0),
            Kind::End => passed(i == self.to),
            Kind::Caret => passed(self.at_line_start(i)?),
            Kind::UnixCaret => passed(i != self.to && (i == 0 || self.unit(i - 1)? == 0x0A)),
            Kind::Dollar { multiline } => passed(self.at_dollar(i, *multiline)?),
            Kind::UnixDollar { multiline } => passed(self.at_unix_dollar(i, *multiline)?),
            Kind::WordBoundary { negated, unicode } => passed(self.at_word_boundary(i, *unicode)? != *negated),
            Kind::GraphemeBoundary => passed(self.at_grapheme_boundary(i)?),
            Kind::LineEnding => self.line_ending(next, i)?,
            Kind::Grapheme => {
                if i < self.to {
                    Step::Enter(next, self.grapheme_end(i)?)
                } else {
                    Step::Return(false)
                }
            }
            Kind::Char(class) => match self.char_end(class, i)? {
                Some(end) => Step::Enter(next, end),
                None => Step::Return(false),
            },
            Kind::CharRun { class, min } => self.run_forward(id, class, *min, i)?,
            Kind::Slice { text, wide, fold } => match self.slice_end(text, *wide, *fold, i)? {
                Some(end) => Step::Enter(next, end),
                None => Step::Return(false),
            },
            Kind::BackRef { group, fold } => match self.back_reference_end(*group, *fold, i)? {
                Some(end) => Step::Enter(next, end),
                None => Step::Return(false),
            },
            Kind::GroupHead { slot } => {
                let value = self.local(*slot);
                self.stack.push(Frame::RestoreLocal { slot: *slot, value });
                self.set_local(*slot, i);
                Step::Enter(next, i)
            }
            Kind::GroupTail { slot, group } => {
                if let Some(group) = *group {
                    let span = self.group(group);
                    self.stack.push(Frame::RestoreGroup { group, span });
                    self.set_group(group, (self.local(*slot), i));
                }
                Step::Enter(next, i)
            }
            Kind::Branch { atoms, conn } => self.try_alternative(id, atoms, *conn, 0, i),
            Kind::BranchConn => Step::Enter(next, i),
            Kind::Ques { atom, quantifier } => {
                let frame = match quantifier {
                    Quantifier::Greedy => Frame::QuesAtom { next, i },
                    Quantifier::Possessive => Frame::QuesPossessive { next, i },
                    Quantifier::Atomic => Frame::AtomThen { next },
                    Quantifier::Lazy => {
                        self.stack.push(Frame::QuesLazy { node: id, i });
                        return Ok(Step::Enter(next, i));
                    }
                };
                self.stack.push(frame);
                Step::Enter(*atom, i)
            }
            Kind::Curly { atom, min, .. } => {
                if *min > 0 {
                    self.stack.push(Frame::CurlyMin { node: id, count: 0 });
                    Step::Enter(*atom, i)
                } else {
                    self.curly_optional(id, i, 0)
                }
            }
            Kind::GroupCurly { group, .. } => {
                if let Some(group) = *group {
                    let span = self.group(group);
                    self.stack.push(Frame::RestoreGroup { group, span });
                }
                self.group_mandatory(id, i, 0)
            }
            Kind::LoopEntry { target } => self.enter_loop(*target, i),
            Kind::Loop { .. } => self.continue_loop(id, i),
            Kind::Lookahead { cond, negated } => {
                self.stack.push(Frame::Lookahead { next, i, negated: *negated });
                Step::Enter(*cond, i)
            }
            Kind::Lookbehind { .. } => self.enter_lookbehind(id, i)?,
        })
    }

    fn resume(&mut self, frame: Frame, outcome: bool) -> Stepped {
        let done = Step::Return(outcome);
        Ok(match frame {
            Frame::RestoreLocal { slot, value } => {
                self.set_local(slot, value);
                done
            }
            Frame::RestoreCount { slot, value } => {
                if !outcome {
                    self.set_local(slot, value);
                }
                done
            }
            Frame::RestoreGroup { group, span } => {
                if !outcome {
                    self.set_group(group, span);
                }
                done
            }
            Frame::Branch { node, alternative, i } => {
                if outcome {
                    return Ok(done);
                }
                let program = self.program;
                match program.nodes.get(node as usize).map(|node| &node.kind) {
                    Some(Kind::Branch { atoms, conn }) => {
                        self.try_alternative(node, atoms, *conn, alternative as usize, i)
                    }
                    _ => done,
                }
            }
            Frame::OrElse { next, i } => {
                if outcome {
                    done
                } else {
                    Step::Enter(next, i)
                }
            }
            Frame::QuesAtom { next, i } => {
                if outcome {
                    self.stack.push(Frame::OrElse { next, i });
                    Step::Enter(next, self.last)
                } else {
                    Step::Enter(next, i)
                }
            }
            Frame::QuesLazy { node, i } => {
                if outcome {
                    return Ok(done);
                }
                let next = self.next_of(node);
                self.stack.push(Frame::AtomThen { next });
                match self.node(node).kind {
                    Kind::Ques { atom, .. } => Step::Enter(atom, i),
                    _ => Step::Return(false),
                }
            }
            Frame::QuesPossessive { next, i } => Step::Enter(next, if outcome { self.last } else { i }),
            Frame::AtomThen { next } => {
                if outcome {
                    Step::Enter(next, self.last)
                } else {
                    done
                }
            }
            Frame::Lookahead { next, i, negated } => {
                if outcome != negated {
                    Step::Enter(next, i)
                } else {
                    Step::Return(false)
                }
            }
            Frame::Lookbehind { node, i, j, from, saved_end } => {
                self.resume_lookbehind(node, i, j, from, saved_end, outcome)?
            }
            Frame::CurlyMin { node, count } => {
                if !outcome {
                    return Ok(done);
                }
                self.curly_mandatory(node, self.last, count + 1)
            }
            Frame::CurlyFirst(state) => self.curly_first(state, outcome),
            Frame::CurlyStep(state) => self.curly_step(state, outcome),
            Frame::CurlyRecurse(state) => {
                if outcome {
                    done
                } else {
                    self.curly_back_off(state)
                }
            }
            Frame::CurlyBack(mut state) => {
                if outcome {
                    return Ok(done);
                }
                state.i -= state.stride;
                state.count -= 1;
                self.curly_back_off(state)
            }
            Frame::CurlyLazy { node, i, count } => {
                if outcome {
                    return Ok(done);
                }
                let (atom, _, max, _) = self.curly_fields(node);
                if count >= max {
                    return Ok(Step::Return(false));
                }
                self.stack.push(Frame::CurlyLazyAtom { node, i, count });
                Step::Enter(atom, i)
            }
            Frame::CurlyLazyAtom { node, i, count } => {
                if !outcome || i == self.last {
                    return Ok(Step::Return(false));
                }
                self.curly_lazy(node, self.last, count + 1)
            }
            Frame::CurlyPossessive { node, i, count } => {
                if !outcome || i == self.last {
                    return Ok(Step::Enter(self.next_of(node), i));
                }
                self.curly_possessive(node, self.last, count + 1)
            }
            Frame::GroupMin { node, i, count } => {
                if !outcome {
                    return Ok(done);
                }
                let last = self.last;
                self.set_optional_group(self.group_curly_fields(node).3, (i, last));
                self.group_mandatory(node, last, count + 1)
            }
            Frame::GroupFirst(state, saved) => self.group_first(state, saved, outcome),
            Frame::GroupStep(state, saved) => self.group_step(state, saved, outcome),
            Frame::GroupRecurse(state, saved) => {
                if outcome {
                    done
                } else {
                    self.group_back_off(state, saved)
                }
            }
            Frame::GroupBack(mut state, saved) => {
                let group = self.group_curly_fields(state.node).3;
                if outcome {
                    self.set_optional_group(group, (state.i - state.stride, state.i));
                    return Ok(done);
                }
                state.i -= state.stride;
                self.set_optional_group(group, (state.i - state.stride, state.i));
                state.count -= 1;
                self.group_back_off(state, saved)
            }
            Frame::GroupLazy { node, i, count } => {
                if outcome {
                    return Ok(done);
                }
                let (atom, _, max, _) = self.group_curly_fields(node);
                if count >= max {
                    return Ok(Step::Return(false));
                }
                self.stack.push(Frame::GroupLazyAtom { node, i, count });
                Step::Enter(atom, i)
            }
            Frame::GroupLazyAtom { node, i, count } => {
                if !outcome || i == self.last {
                    return Ok(Step::Return(false));
                }
                let last = self.last;
                self.set_optional_group(self.group_curly_fields(node).3, (i, last));
                self.group_lazy(node, last, count + 1)
            }
            Frame::RunBack { node, i, count } => {
                if outcome {
                    return Ok(done);
                }
                self.run_back_off(node, i, count)?
            }
            Frame::LoopEntryLazy { node, i } => {
                if outcome {
                    return Ok(done);
                }
                match self.node(node).kind {
                    Kind::Loop { body, count, max, .. } if max > 0 => {
                        self.set_local(count, 1);
                        Step::Enter(body, i)
                    }
                    _ => Step::Return(false),
                }
            }
            Frame::LoopBody { node, i, count } => {
                if outcome {
                    return Ok(done);
                }
                if let Kind::Loop { count: slot, memo, .. } = self.node(node).kind {
                    self.set_local(slot, count);
                    if let Some(set) = memo.and_then(|memo| self.memos.get_mut(memo as usize)) {
                        set.insert(i);
                    }
                }
                Step::Enter(self.next_of(node), i)
            }
            Frame::LoopLazy { node, i, count } => {
                if outcome {
                    return Ok(done);
                }
                match self.node(node).kind {
                    Kind::Loop { body, count: slot, max, .. } if count < max => {
                        self.set_local(slot, count + 1);
                        self.stack.push(Frame::RestoreCount { slot, value: count });
                        Step::Enter(body, i)
                    }
                    _ => Step::Return(false),
                }
            }
        })
    }

    fn at_line_start(&self, i: i32) -> Result<bool, Thrown> {
        if i == self.to {
            return Ok(false);
        }
        if i > 0 {
            let before = u32::from(self.unit(i - 1)?);
            if !charclass::line_terminator(before) {
                return Ok(false);
            }
            if before == 0x0D && self.unit(i)? == 0x0A {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn at_dollar(&self, i: i32, multiline: bool) -> Result<bool, Thrown> {
        let to = self.to;
        if !multiline {
            if i < to - 2 {
                return Ok(false);
            }
            if i == to - 2 && (self.unit(i)? != 0x0D || self.unit(i + 1)? != 0x0A) {
                return Ok(false);
            }
        }
        if i < to {
            let unit = self.unit(i)?;
            if unit == 0x0A {
                if i > 0 && self.unit(i - 1)? == 0x0D {
                    return Ok(false);
                }
            } else if !charclass::line_terminator(u32::from(unit)) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn at_unix_dollar(&self, i: i32, multiline: bool) -> Result<bool, Thrown> {
        if i < self.to {
            if self.unit(i)? != 0x0A {
                return Ok(false);
            }
            if !multiline && i != self.to - 1 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn at_word_boundary(&self, i: i32, unicode_classes: bool) -> Result<bool, Thrown> {
        let mark = unicode::category::NON_SPACING_MARK;
        let mut left = false;
        if i > 0 {
            let cp = self.code_point_before(i)?;
            left = property::is_word(cp, unicode_classes)
                || (unicode::has_category(cp, mark) && self.has_base_character(i - 1)?);
        }
        let mut right = false;
        if i < self.to {
            let cp = self.code_point_at(i)?;
            right = property::is_word(cp, unicode_classes)
                || (unicode::has_category(cp, mark) && self.has_base_character(i)?);
        }
        Ok(left != right)
    }

    /// `true` when the marks ending at `i` follow a letter or digit.
    fn has_base_character(&self, i: i32) -> Result<bool, Thrown> {
        let mut x = i;
        while x >= 0 {
            let cp = self.code_point_at(x)?;
            if unicode::is_letter_or_digit(cp) {
                return Ok(true);
            }
            if !unicode::has_category(cp, unicode::category::NON_SPACING_MARK) {
                return Ok(false);
            }
            x -= 1;
        }
        Ok(false)
    }

    /// Java measures the cluster from the end of the latest sub-match, not from `i`, so this
    /// depends on `last` and throws when `last` is at the end of the input.
    fn at_grapheme_boundary(&self, i: i32) -> Result<bool, Thrown> {
        if i == 0 || i >= self.to {
            return Ok(true);
        }
        let high = self.unit(i - 1)?;
        if is_high_surrogate(high) && is_low_surrogate(self.unit(i)?) {
            return Ok(false);
        }
        Ok(self.grapheme_end(self.last)? <= i)
    }

    fn grapheme_end(&self, i: i32) -> Result<i32, Thrown> {
        let mut before = self.code_point_at(i)?;
        let mut j = i + char_count(before);
        while j < self.to {
            let after = self.code_point_at(j)?;
            if is_grapheme_boundary(before, after) {
                break;
            }
            before = after;
            j += char_count(after);
        }
        Ok(j.min(self.to))
    }

    fn line_ending(&mut self, next: NodeId, i: i32) -> Stepped {
        if i >= self.to {
            return Ok(Step::Return(false));
        }
        let unit = u32::from(self.unit(i)?);
        if unit == 0x0D {
            let after = i + 1;
            if after < self.to && self.unit(after)? == 0x0A {
                self.stack.push(Frame::OrElse { next, i: after });
                return Ok(Step::Enter(next, after + 1));
            }
            return Ok(Step::Enter(next, after));
        }
        Ok(if charclass::vertical_space(unit) { Step::Enter(next, i + 1) } else { Step::Return(false) })
    }

    /// The position after the character at `i`, if `class` accepts it.
    fn char_end(&self, class: &CharClass, i: i32) -> Result<Option<i32>, Thrown> {
        if i >= self.to {
            return Ok(None);
        }
        let cp = self.code_point_at(i)?;
        let end = i + char_count(cp);
        Ok((end <= self.to && class.contains(cp)).then_some(end))
    }

    fn run_forward(&mut self, node: NodeId, class: &CharClass, min: i32, start: i32) -> Stepped {
        let mut i = start;
        let mut count = 0;
        while let Some(end) = self.char_end(class, i)? {
            i = end;
            count += 1;
        }
        self.run_attempt(node, i, count, min)
    }

    fn run_attempt(&mut self, node: NodeId, i: i32, count: i32, min: i32) -> Stepped {
        if count < min {
            return Ok(Step::Return(false));
        }
        self.stack.push(Frame::RunBack { node, i, count });
        Ok(Step::Enter(self.next_of(node), i))
    }

    fn run_back_off(&mut self, node: NodeId, i: i32, count: i32) -> Stepped {
        let min = match &self.node(node).kind {
            Kind::CharRun { min, .. } => *min,
            _ => 0,
        };
        if count == min {
            return Ok(Step::Return(false));
        }
        let width = char_count(self.code_point_before(i)?);
        self.run_attempt(node, (i - width).max(0), count - 1, min)
    }

    fn slice_end(&self, text: &[u32], wide: bool, fold: Fold, i: i32) -> Result<Option<i32>, Thrown> {
        let matches_folded = |expected: u32, actual: u32| {
            expected == actual
                || match fold {
                    Fold::None => false,
                    Fold::Ascii => expected == charclass::ascii_to_lower(actual),
                    Fold::Unicode => expected == charclass::unicode_fold(actual),
                }
        };
        let mut x = i;
        for &expected in text {
            if x >= self.to {
                return Ok(None);
            }
            let actual = if wide { self.code_point_at(x)? } else { u32::from(self.unit(x)?) };
            if !matches_folded(expected, actual) {
                return Ok(None);
            }
            x += if wide { char_count(actual) } else { 1 };
            if x > self.to {
                return Ok(None);
            }
        }
        Ok(Some(x))
    }

    fn back_reference_end(&self, group: u32, fold: Fold, i: i32) -> Result<Option<i32>, Thrown> {
        let (start, end) = self.group(group);
        if start < 0 {
            return Ok(None);
        }
        let size = end - start;
        if i + size > self.to {
            return Ok(None);
        }
        if fold == Fold::None {
            for offset in 0..size {
                if self.unit(i + offset)? != self.unit(start + offset)? {
                    return Ok(None);
                }
            }
            return Ok(Some(i + size));
        }
        let mut x = i;
        let mut j = start;
        while x < i + size {
            let first = self.code_point_at(x)?;
            let second = self.code_point_at(j)?;
            if first != second {
                let same = if fold == Fold::Unicode {
                    let (upper1, upper2) = (unicode::to_upper(first), unicode::to_upper(second));
                    upper1 == upper2 || unicode::to_lower(upper1) == unicode::to_lower(upper2)
                } else {
                    charclass::ascii_to_lower(first) == charclass::ascii_to_lower(second)
                };
                if !same {
                    return Ok(None);
                }
            }
            x += char_count(first);
            j += char_count(second);
        }
        Ok(Some(i + size))
    }

    fn try_alternative(
        &mut self,
        node: NodeId,
        atoms: &[Option<NodeId>],
        conn: NodeId,
        alternative: usize,
        i: i32,
    ) -> Step {
        let Some(atom) = atoms.get(alternative) else {
            return Step::Return(false);
        };
        if alternative + 1 < atoms.len() {
            self.stack.push(Frame::Branch { node, alternative: alternative as u32 + 1, i });
        }
        match atom {
            Some(atom) => Step::Enter(*atom, i),
            None => Step::Enter(self.next_of(conn), i),
        }
    }

    fn enter_lookbehind(&mut self, node: NodeId, i: i32) -> Stepped {
        let Kind::Lookbehind { min, max, wide, .. } = self.node(node).kind else {
            return Ok(Step::Return(false));
        };
        let (farthest, nearest) = if wide {
            (self.count_chars(i, max.wrapping_neg())?, self.count_chars(i, min.wrapping_neg())?)
        } else {
            (max, min)
        };
        let from = i.wrapping_sub(farthest).max(0);
        let saved_end = self.lookbehind_end;
        self.lookbehind_end = i;
        self.try_lookbehind(node, i, i.wrapping_sub(nearest), from, saved_end, false)
    }

    fn try_lookbehind(&mut self, node: NodeId, i: i32, j: i32, from: i32, saved_end: i32, matched: bool) -> Stepped {
        let Kind::Lookbehind { cond, negated, .. } = self.node(node).kind else {
            return Ok(Step::Return(false));
        };
        if !matched && j >= from {
            self.stack.push(Frame::Lookbehind { node, i, j, from, saved_end });
            return Ok(Step::Enter(cond, j));
        }
        self.lookbehind_end = saved_end;
        Ok(if matched != negated { Step::Enter(self.next_of(node), i) } else { Step::Return(false) })
    }

    fn resume_lookbehind(&mut self, node: NodeId, i: i32, j: i32, from: i32, saved_end: i32, matched: bool) -> Stepped {
        let wide = matches!(self.node(node).kind, Kind::Lookbehind { wide: true, .. });
        let step = if wide && j > from { self.count_chars(j, -1)? } else { 1 };
        self.try_lookbehind(node, i, j - step, from, saved_end, matched)
    }

    /// Number of UTF-16 units spanned by `length` code points from `index`, backwards if negative.
    fn count_chars(&self, index: i32, length: i32) -> Result<i32, Thrown> {
        let mut x = index;
        if length >= 0 {
            let mut done = 0;
            while x < self.to && done < length {
                let unit = self.unit(x)?;
                x += 1;
                if is_high_surrogate(unit) && x < self.to && is_low_surrogate(self.unit(x)?) {
                    x += 1;
                }
                done += 1;
            }
            return Ok(x - index);
        }
        if index == 0 {
            return Ok(0);
        }
        let wanted = length.wrapping_neg();
        let mut done = 0;
        while x > 0 && (done < wanted || wanted < 0) {
            x -= 1;
            if is_low_surrogate(self.unit(x)?) && x > 0 && is_high_surrogate(self.unit(x - 1)?) {
                x -= 1;
            }
            done += 1;
        }
        Ok(index - x)
    }

    /// Atom, min, max and quantifier of a [`Kind::Curly`].
    fn curly_fields(&self, node: NodeId) -> (NodeId, i32, i32, Quantifier) {
        match self.node(node).kind {
            Kind::Curly { atom, min, max, quantifier } => (atom, min, max, quantifier),
            _ => (0, 0, 0, Quantifier::Greedy),
        }
    }

    fn curly_mandatory(&mut self, node: NodeId, i: i32, count: i32) -> Step {
        let (atom, min, _, _) = self.curly_fields(node);
        if count < min {
            self.stack.push(Frame::CurlyMin { node, count });
            Step::Enter(atom, i)
        } else {
            self.curly_optional(node, i, count)
        }
    }

    fn curly_optional(&mut self, node: NodeId, i: i32, count: i32) -> Step {
        match self.curly_fields(node).3 {
            Quantifier::Lazy => self.curly_lazy(node, i, count),
            Quantifier::Possessive | Quantifier::Atomic => self.curly_possessive(node, i, count),
            Quantifier::Greedy => self.curly_greedy(node, i, count),
        }
    }

    fn curly_lazy(&mut self, node: NodeId, i: i32, count: i32) -> Step {
        self.stack.push(Frame::CurlyLazy { node, i, count });
        Step::Enter(self.next_of(node), i)
    }

    fn curly_possessive(&mut self, node: NodeId, i: i32, count: i32) -> Step {
        let (atom, _, max, _) = self.curly_fields(node);
        if count < max {
            self.stack.push(Frame::CurlyPossessive { node, i, count });
            Step::Enter(atom, i)
        } else {
            Step::Enter(self.next_of(node), i)
        }
    }

    fn curly_greedy(&mut self, node: NodeId, i: i32, count: i32) -> Step {
        let (atom, _, max, _) = self.curly_fields(node);
        if count >= max {
            return Step::Enter(self.next_of(node), i);
        }
        self.stack.push(Frame::CurlyFirst(Greedy { node, i, count, stride: 0, floor: count }));
        Step::Enter(atom, i)
    }

    fn curly_first(&mut self, mut state: Greedy, matched: bool) -> Step {
        if matched {
            state.stride = self.last - state.i;
            if state.stride != 0 {
                state.i = self.last;
                state.count += 1;
                return self.curly_iterate(state);
            }
        }
        Step::Enter(self.next_of(state.node), state.i)
    }

    fn curly_iterate(&mut self, state: Greedy) -> Step {
        let (atom, _, max, _) = self.curly_fields(state.node);
        if state.count < max {
            self.stack.push(Frame::CurlyStep(state));
            Step::Enter(atom, state.i)
        } else {
            self.curly_back_off(state)
        }
    }

    fn curly_step(&mut self, mut state: Greedy, matched: bool) -> Step {
        if !matched {
            return self.curly_back_off(state);
        }
        if state.i + state.stride != self.last {
            self.stack.push(Frame::CurlyRecurse(state));
            return self.curly_greedy(state.node, self.last, state.count + 1);
        }
        state.i += state.stride;
        state.count += 1;
        self.curly_iterate(state)
    }

    fn curly_back_off(&mut self, state: Greedy) -> Step {
        if state.count < state.floor {
            return Step::Return(false);
        }
        self.stack.push(Frame::CurlyBack(state));
        Step::Enter(self.next_of(state.node), state.i)
    }

    /// Atom, min, max and group of a [`Kind::GroupCurly`].
    fn group_curly_fields(&self, node: NodeId) -> (NodeId, i32, i32, Option<u32>) {
        match self.node(node).kind {
            Kind::GroupCurly { atom, min, max, group, .. } => (atom, min, max, group),
            _ => (0, 0, 0, None),
        }
    }

    fn group_mandatory(&mut self, node: NodeId, i: i32, count: i32) -> Step {
        let (atom, min, _, _) = self.group_curly_fields(node);
        if count < min {
            self.stack.push(Frame::GroupMin { node, i, count });
            return Step::Enter(atom, i);
        }
        if matches!(self.node(node).kind, Kind::GroupCurly { lazy: true, .. }) {
            self.group_lazy(node, i, count)
        } else {
            self.group_greedy(node, i, count)
        }
    }

    fn group_lazy(&mut self, node: NodeId, i: i32, count: i32) -> Step {
        self.stack.push(Frame::GroupLazy { node, i, count });
        Step::Enter(self.next_of(node), i)
    }

    fn saved_group(&self, group: Option<u32>) -> Span {
        group.map_or((0, 0), |group| self.group(group))
    }

    fn group_greedy(&mut self, node: NodeId, i: i32, count: i32) -> Step {
        let (atom, _, max, group) = self.group_curly_fields(node);
        let saved = self.saved_group(group);
        let state = Greedy { node, i, count, stride: 0, floor: count };
        if count >= max {
            return self.group_finish(state, saved);
        }
        self.stack.push(Frame::GroupFirst(state, saved));
        Step::Enter(atom, i)
    }

    fn group_first(&mut self, mut state: Greedy, saved: Span, matched: bool) -> Step {
        if !matched {
            return self.group_finish(state, saved);
        }
        state.stride = self.last - state.i;
        if state.stride <= 0 {
            state.i += state.stride;
            return self.group_finish(state, saved);
        }
        self.group_record(state, saved)
    }

    /// Records one iteration of length `stride`, then tries another.
    fn group_record(&mut self, mut state: Greedy, saved: Span) -> Step {
        let (atom, _, max, group) = self.group_curly_fields(state.node);
        self.set_optional_group(group, (state.i, state.i + state.stride));
        state.i += state.stride;
        state.count += 1;
        if state.count >= max {
            return self.group_back_off(state, saved);
        }
        self.stack.push(Frame::GroupStep(state, saved));
        Step::Enter(atom, state.i)
    }

    fn group_step(&mut self, state: Greedy, saved: Span, matched: bool) -> Step {
        if !matched {
            return self.group_back_off(state, saved);
        }
        if state.i + state.stride != self.last {
            self.stack.push(Frame::GroupRecurse(state, saved));
            return self.group_greedy(state.node, state.i, state.count);
        }
        self.group_record(state, saved)
    }

    fn group_back_off(&mut self, state: Greedy, saved: Span) -> Step {
        if state.count > state.floor {
            self.stack.push(Frame::GroupBack(state, saved));
            return Step::Enter(self.next_of(state.node), state.i);
        }
        self.group_finish(state, saved)
    }

    fn group_finish(&mut self, state: Greedy, saved: Span) -> Step {
        self.set_optional_group(self.group_curly_fields(state.node).3, saved);
        Step::Enter(self.next_of(state.node), state.i)
    }

    fn enter_loop(&mut self, target: NodeId, i: i32) -> Step {
        let Kind::Loop { body, count, min, max, lazy, .. } = self.node(target).kind else {
            return Step::Return(false);
        };
        let value = self.local(count);
        self.stack.push(Frame::RestoreLocal { slot: count, value });
        if min > 0 {
            self.set_local(count, 1);
            return Step::Enter(body, i);
        }
        let next = self.next_of(target);
        if lazy {
            self.stack.push(Frame::LoopEntryLazy { node: target, i });
            return Step::Enter(next, i);
        }
        if max > 0 {
            self.set_local(count, 1);
            self.stack.push(Frame::OrElse { next, i });
            return Step::Enter(body, i);
        }
        Step::Enter(next, i)
    }

    fn continue_loop(&mut self, node: NodeId, i: i32) -> Step {
        let next = self.next_of(node);
        let Kind::Loop { body, begin, count: slot, min, max, lazy, memo } = self.node(node).kind else {
            return Step::Return(false);
        };
        if i <= self.local(begin) {
            return Step::Enter(next, i);
        }
        let count = self.local(slot);
        if count < min {
            self.set_local(slot, count + 1);
            self.stack.push(Frame::RestoreCount { slot, value: count });
            return Step::Enter(body, i);
        }
        if lazy {
            self.stack.push(Frame::LoopLazy { node, i, count });
            return Step::Enter(next, i);
        }
        if count < max {
            let failed_before = memo.and_then(|memo| self.memos.get(memo as usize)).is_some_and(|set| set.contains(&i));
            if !failed_before {
                self.set_local(slot, count + 1);
                self.stack.push(Frame::LoopBody { node, i, count });
                return Step::Enter(body, i);
            }
        }
        Step::Enter(next, i)
    }
}

/// Stand-in for a node that cannot be missing from a well-formed program.
static ACCEPT_NODE: Node = Node { kind: Kind::Accept, next: 0 };
