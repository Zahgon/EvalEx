//! Recursive-descent parser turning pattern text into a [`Program`].
//!
//! Error descriptions and indices are part of the contract, and Java reports the position of its
//! cursor at the moment it gives up, so the cursor movements here are deliberate: `peek`, `next`,
//! `read`, `skip` and friends differ in where they leave the cursor and in whether they honour
//! `COMMENTS` mode.

use super::charclass::{self, CharClass};
use super::escape::Escaped;
use super::node::{Fold, Kind, Node, NodeId, Program, Quantifier, ACCEPT, LAST_ACCEPT, MAX_REPS};
use super::study::{study_chain, TreeInfo};
use super::{unicode, JString, PatternSyntaxError};

pub(super) const CASE_INSENSITIVE: u32 = 1 << 1;
pub(super) const UNIX_LINES: u32 = 1 << 0;
pub(super) const COMMENTS: u32 = 1 << 2;
pub(super) const MULTILINE: u32 = 1 << 3;
pub(super) const DOTALL: u32 = 1 << 5;
pub(super) const UNICODE_CASE: u32 = 1 << 6;
pub(super) const CANON_EQ: u32 = 1 << 7;
pub(super) const UNICODE_CHARACTER_CLASS: u32 = 1 << 8;

/// Deepest nesting of groups and bracket classes the recursive parser accepts.
const MAX_NESTING: u32 = 250;

pub(super) type Parsed<T> = Result<T, PatternSyntaxError>;

/// Classifies a code point for `match`: ASCII values stay, everything else becomes `0xFF`, which
/// no arm of the parser tests for.
pub(super) fn ascii(ch: i32) -> u8 {
    u8::try_from(ch).ok().filter(u8::is_ascii).unwrap_or(0xFF)
}

/// `true` for what cannot be compared unit by unit: supplementary code points and surrogates.
fn is_supplementary(ch: i32) -> bool {
    ch >= 0x10000 || (0xD800..=0xDFFF).contains(&ch)
}

/// The pattern text decoded to code points.
fn code_points(source: &[u16]) -> Vec<i32> {
    char::decode_utf16(source.iter().copied())
        .map(|unit| match unit {
            Ok(c) => u32::from(c) as i32,
            Err(lone) => i32::from(lone.unpaired_surrogate()),
        })
        .collect()
}

/// Rewrites `\Q...\E` quotes into plain escapes. Everything in the parser, error indices
/// included, then refers to the rewritten text.
fn remove_quoting(text: Vec<i32>) -> Vec<i32> {
    let at = |index: usize| text.get(index).copied().unwrap_or(0);
    let mut i = 0;
    while i + 1 < text.len() {
        if at(i) != i32::from(b'\\') {
            i += 1;
        } else if at(i + 1) != i32::from(b'Q') {
            i += 2;
        } else {
            break;
        }
    }
    if i + 1 >= text.len() {
        return text;
    }

    let mut rewritten: Vec<i32> = text.get(..i).unwrap_or(&[]).to_vec();
    let backslash = i32::from(b'\\');
    i += 2;
    let mut in_quote = true;
    let mut begin_quote = true;
    while i < text.len() {
        let c = at(i);
        i += 1;
        if ascii(c) == 0xFF || charclass::ascii_alpha(c as u32) {
            rewritten.push(c);
        } else if charclass::ascii_digit(c as u32) {
            if begin_quote {
                // An escape before the quote must not swallow this digit.
                rewritten.extend([backslash, i32::from(b'x'), i32::from(b'3')]);
            }
            rewritten.push(c);
        } else if c != backslash {
            if in_quote {
                rewritten.push(backslash);
            }
            rewritten.push(c);
        } else if in_quote {
            if at(i) == i32::from(b'E') {
                i += 1;
                in_quote = false;
            } else {
                rewritten.extend([backslash, backslash]);
            }
        } else if at(i) == i32::from(b'Q') {
            i += 1;
            in_quote = true;
            begin_quote = true;
            continue;
        } else {
            rewritten.push(c);
            if i != text.len() {
                rewritten.push(at(i));
                i += 1;
            }
        }
        begin_quote = false;
    }
    rewritten
}

/// One parsed quantifier.
struct Repeat {
    min: i32,
    max: i32,
    quantifier: Quantifier,
    shape: Shape,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// `?` and `{0,1}`
    Optional,
    /// `*`, `+` and `{n,}`
    Open,
    /// `{n}` and `{n,m}`
    Bounded,
}

/// A chain of linked nodes.
type Chain = (NodeId, NodeId);

/// Parser state; `text` ends in two zeros so that lookahead past the end reads as NUL.
pub(super) struct Parser<'a> {
    source: &'a [u16],
    pub(super) text: Vec<i32>,
    pub(super) length: usize,
    pub(super) cursor: usize,
    pub(super) flags: u32,
    nodes: Vec<Node>,
    pub(super) group_count: u32,
    local_count: u32,
    named_groups: Vec<(String, u32)>,
    pub(super) has_group_ref: bool,
    /// Greedy unbounded loops not nested in another quantifier or look-behind.
    top_loops: Vec<NodeId>,
    pub(super) nesting: u32,
}

impl<'a> Parser<'a> {
    fn new(source: &'a [u16]) -> Parser<'a> {
        let mut text = remove_quoting(code_points(source));
        let length = text.len();
        text.extend([0, 0]);
        let sentinel = |kind| Node { kind, next: ACCEPT };
        Parser {
            source,
            text,
            length,
            cursor: 0,
            flags: 0,
            nodes: vec![sentinel(Kind::Accept), sentinel(Kind::LastAccept)],
            group_count: 1,
            local_count: 0,
            named_groups: Vec::new(),
            has_group_ref: false,
            top_loops: Vec::new(),
            nesting: 0,
        }
    }

    /// Compiles `source`.
    pub(super) fn compile(source: &'a [u16]) -> Parsed<Program> {
        let mut parser = Parser::new(source);
        let root = parser.expr(LAST_ACCEPT)?;
        if parser.length != parser.cursor {
            // Only a backslash at the very end carries the cursor past the pattern.
            let message =
                if parser.peek() == i32::from(b')') { "Unmatched closing ')'" } else { "Unescaped trailing backslash" };
            return Err(parser.error(message));
        }
        let memo_count = parser.assign_memos();
        Ok(Program {
            nodes: parser.nodes,
            root,
            group_count: parser.group_count,
            local_count: parser.local_count,
            memo_count,
        })
    }

    /// Without back-references, a loop may remember the positions where its body failed.
    fn assign_memos(&mut self) -> u32 {
        if self.has_group_ref {
            return 0;
        }
        let mut memo_count = 0;
        for id in &self.top_loops {
            if let Some(Node { kind: Kind::Loop { memo, .. }, .. }) = self.nodes.get_mut(*id as usize) {
                *memo = Some(memo_count);
                memo_count += 1;
            }
        }
        memo_count
    }

    /// A syntax error at the character before the cursor.
    pub(super) fn error(&self, description: &str) -> PatternSyntaxError {
        self.error_text(super::js(description))
    }

    /// Like [`Parser::error`] for a description that is already UTF-16.
    pub(super) fn error_text(&self, description: JString) -> PatternSyntaxError {
        PatternSyntaxError { description, index: self.cursor as i32 - 1, pattern: self.source.to_vec() }
    }

    pub(super) fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    pub(super) fn at(&self, index: usize) -> i32 {
        self.text.get(index).copied().unwrap_or(0)
    }

    fn is_line_separator(&self, ch: i32) -> bool {
        if self.has(UNIX_LINES) {
            ch == 0x0A
        } else {
            charclass::line_terminator(ch as u32)
        }
    }

    /// Moves to the end of a comment, leaving the cursor on the character returned.
    fn peek_past_line(&mut self) -> i32 {
        self.cursor += 1;
        let mut ch = self.at(self.cursor);
        while ch != 0 && !self.is_line_separator(ch) {
            self.cursor += 1;
            ch = self.at(self.cursor);
        }
        ch
    }

    fn peek_past_whitespace(&mut self, mut ch: i32) -> i32 {
        while charclass::ascii_space(ch as u32) || ch == i32::from(b'#') {
            while charclass::ascii_space(ch as u32) {
                self.cursor += 1;
                ch = self.at(self.cursor);
            }
            if ch == i32::from(b'#') {
                ch = self.peek_past_line();
            }
        }
        ch
    }

    /// Consumes the rest of a comment, leaving the cursor after the character returned.
    fn parse_past_line(&mut self) -> i32 {
        let mut ch = self.read_escaped();
        while ch != 0 && !self.is_line_separator(ch) && self.cursor <= self.length {
            ch = self.read_escaped();
        }
        ch
    }

    fn parse_past_whitespace(&mut self, mut ch: i32) -> i32 {
        while charclass::ascii_space(ch as u32) || ch == i32::from(b'#') {
            while charclass::ascii_space(ch as u32) {
                ch = self.read_escaped();
            }
            if ch == i32::from(b'#') {
                ch = self.parse_past_line();
            }
        }
        ch
    }

    /// The current character; comments are skipped but it is not consumed.
    pub(super) fn peek(&mut self) -> i32 {
        let ch = self.at(self.cursor);
        if self.has(COMMENTS) {
            self.peek_past_whitespace(ch)
        } else {
            ch
        }
    }

    /// Consumes and returns the current character.
    pub(super) fn read(&mut self) -> i32 {
        let ch = self.read_escaped();
        if self.has(COMMENTS) {
            self.parse_past_whitespace(ch)
        } else {
            ch
        }
    }

    fn read_escaped(&mut self) -> i32 {
        let ch = self.at(self.cursor);
        self.cursor += 1;
        ch
    }

    /// Advances, then returns the new current character.
    pub(super) fn next(&mut self) -> i32 {
        self.cursor += 1;
        self.peek()
    }

    pub(super) fn next_escaped(&mut self) -> i32 {
        self.cursor += 1;
        self.at(self.cursor)
    }

    /// Returns the following character and moves past it, whatever the flags say.
    pub(super) fn skip(&mut self) -> i32 {
        let ch = self.at(self.cursor + 1);
        self.cursor += 2;
        ch
    }

    pub(super) fn unread(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn accept(&mut self, expected: u8, message: &str) -> Parsed<()> {
        if self.read() == i32::from(expected) {
            Ok(())
        } else {
            Err(self.error(message))
        }
    }

    pub(super) fn enter_nested(&mut self) -> Parsed<()> {
        self.nesting += 1;
        if self.nesting > MAX_NESTING {
            return Err(self.error("Stack overflow during pattern compilation"));
        }
        Ok(())
    }

    fn push(&mut self, kind: Kind) -> NodeId {
        let id = self.nodes.len() as NodeId;
        self.nodes.push(Node { kind, next: ACCEPT });
        id
    }

    fn set_next(&mut self, id: NodeId, next: NodeId) {
        if let Some(node) = self.nodes.get_mut(id as usize) {
            node.next = next;
        }
    }

    fn next_of(&self, id: NodeId) -> NodeId {
        self.nodes.get(id as usize).map_or(ACCEPT, |node| node.next)
    }

    fn study(&self, head: NodeId) -> TreeInfo {
        let mut info = TreeInfo::new();
        study_chain(&self.nodes, head, &mut info);
        info
    }

    /// Alternation: returns the head of the expression, which continues at `end`.
    fn expr(&mut self, end: NodeId) -> Parsed<NodeId> {
        let mut alternatives = Vec::new();
        loop {
            alternatives.push(self.sequence(end)?);
            if self.peek() != i32::from(b'|') {
                break;
            }
            self.next();
        }
        if let [(head, _)] = alternatives.as_slice() {
            return Ok(*head);
        }
        let conn = self.push(Kind::BranchConn);
        self.set_next(conn, end);
        let mut atoms = Vec::with_capacity(alternatives.len());
        for (head, tail) in alternatives {
            if head == end {
                atoms.push(None);
            } else {
                self.set_next(tail, conn);
                atoms.push(Some(head));
            }
        }
        Ok(self.push(Kind::Branch { atoms, conn }))
    }

    /// One alternative; the chain is `(end, end)` when it is empty.
    fn sequence(&mut self, end: NodeId) -> Parsed<Chain> {
        let mut chain: Option<Chain> = None;
        loop {
            let ch = self.peek();
            let kind = match ascii(ch) {
                b'(' => {
                    if let Some((head, tail)) = self.group()? {
                        chain = Some(self.link(chain, head, tail));
                    }
                    continue;
                }
                b'[' => Kind::Char(self.bracket_class(true)?),
                b'\\' => {
                    let escaped = self.next_escaped();
                    if ascii(escaped) == b'p' || ascii(escaped) == b'P' {
                        Kind::Char(self.property(ascii(escaped) == b'P')?)
                    } else {
                        self.unread();
                        self.atom()?
                    }
                }
                b'^' => {
                    self.next();
                    if !self.has(MULTILINE) {
                        Kind::Begin
                    } else if self.has(UNIX_LINES) {
                        Kind::UnixCaret
                    } else {
                        Kind::Caret
                    }
                }
                b'$' => {
                    self.next();
                    self.dollar(self.has(MULTILINE))
                }
                b'.' => {
                    self.next();
                    Kind::Char(if self.has(DOTALL) {
                        CharClass::all()
                    } else if self.has(UNIX_LINES) {
                        CharClass::test(|cp| cp != 0x0A)
                    } else {
                        CharClass::test(|cp| !charclass::line_terminator(cp))
                    })
                }
                b'|' | b')' => break,
                b'?' | b'*' | b'+' => {
                    self.next();
                    let symbol = char::from(ascii(ch));
                    return Err(self.error(&format!("Dangling meta character '{symbol}'")));
                }
                0 if self.cursor >= self.length => break,
                _ => self.atom()?,
            };
            let atom = self.push(kind);
            let node = match self.closure()? {
                Some(repeat) => self.repeat_atom(atom, &repeat),
                None => atom,
            };
            chain = Some(self.link(chain, node, node));
        }
        Ok(match chain {
            Some((head, tail)) => {
                self.set_next(tail, end);
                (head, tail)
            }
            None => (end, end),
        })
    }

    fn link(&mut self, chain: Option<Chain>, head: NodeId, tail: NodeId) -> Chain {
        match chain {
            Some((first, last)) => {
                self.set_next(last, head);
                (first, tail)
            }
            None => (head, tail),
        }
    }

    pub(super) fn dollar(&self, multiline: bool) -> Kind {
        if self.has(UNIX_LINES) {
            Kind::UnixDollar { multiline }
        } else {
            Kind::Dollar { multiline }
        }
    }

    /// A run of literal characters or one escape; a quantifier binds to the last character only.
    fn atom(&mut self) -> Parsed<Kind> {
        let mut literal: Vec<i32> = Vec::new();
        let mut previous = self.cursor;
        let mut wide = false;
        let mut ch = self.peek();
        loop {
            match ascii(ch) {
                b'*' | b'+' | b'?' | b'{' => {
                    if literal.len() > 1 {
                        self.cursor = previous;
                        literal.pop();
                    }
                    break;
                }
                b'$' | b'.' | b'^' | b'(' | b'[' | b'|' | b')' => break,
                b'\\' => {
                    let escaped = self.next_escaped();
                    if ascii(escaped) == b'p' || ascii(escaped) == b'P' {
                        if literal.is_empty() {
                            let class = self.property(ascii(escaped) == b'P')?;
                            return Ok(Kind::Char(class));
                        }
                        self.unread();
                        break;
                    }
                    self.unread();
                    previous = self.cursor;
                    match self.escape(false, false)? {
                        Escaped::Char(value) => {
                            literal.push(value);
                            wide |= is_supplementary(value);
                            ch = self.peek();
                            continue;
                        }
                        Escaped::Class(class) if literal.is_empty() => {
                            return Ok(Kind::Char(class));
                        }
                        Escaped::Node(kind) if literal.is_empty() => return Ok(kind),
                        Escaped::Class(_) | Escaped::Node(_) => {
                            self.cursor = previous;
                            break;
                        }
                    }
                }
                0 if self.cursor >= self.length => break,
                _ => {
                    previous = self.cursor;
                    literal.push(ch);
                    wide |= is_supplementary(ch);
                    ch = self.next();
                }
            }
        }
        Ok(match literal.as_slice() {
            [single] => Kind::Char(self.single(*single as u32)),
            _ => self.slice(&literal, wide),
        })
    }

    fn slice(&self, literal: &[i32], wide: bool) -> Kind {
        let chars = literal.iter().map(|&ch| ch as u32);
        let (text, fold) = if !self.has(CASE_INSENSITIVE) {
            (chars.collect(), Fold::None)
        } else if self.has(UNICODE_CASE) {
            (chars.map(charclass::unicode_fold).collect(), Fold::Unicode)
        } else {
            (chars.map(charclass::ascii_to_lower).collect(), Fold::Ascii)
        };
        Kind::Slice { text, wide, fold }
    }

    /// The class matching the single literal `cp` under the current flags.
    pub(super) fn single(&self, cp: u32) -> CharClass {
        if self.has(CASE_INSENSITIVE) {
            if self.has(UNICODE_CASE) {
                let upper = unicode::to_upper(cp);
                let lower = unicode::to_lower(upper);
                if upper != lower {
                    return CharClass::folded(lower);
                }
            } else if charclass::ascii_alpha(cp) {
                return CharClass::either(charclass::ascii_to_lower(cp), charclass::ascii_to_upper(cp));
            }
        }
        CharClass::single(cp)
    }

    /// A parenthesised construct and its quantifier, `None` for a bare `(?flags)`.
    fn group(&mut self) -> Parsed<Option<Chain>> {
        self.enter_nested()?;
        let chain = self.group_body()?;
        self.nesting -= 1;
        Ok(chain)
    }

    fn group_body(&mut self) -> Parsed<Option<Chain>> {
        let saved_flags = self.flags;
        let saved_loops = self.top_loops.len();
        let (head, tail) = if self.next() == i32::from(b'?') {
            let ch = self.skip();
            match ascii(ch) {
                b':' => self.plain_group(None)?,
                b'=' | b'!' => {
                    let (cond, _) = self.plain_group(None)?;
                    let negated = ascii(ch) == b'!';
                    let node = self.push(Kind::Lookahead { cond, negated });
                    (node, node)
                }
                b'>' => {
                    let (atom, _) = self.plain_group(None)?;
                    let quantifier = Quantifier::Atomic;
                    let node = self.push(Kind::Ques { atom, quantifier });
                    (node, node)
                }
                b'<' => {
                    let ch = self.read();
                    if ch == i32::from(b'=') || ch == i32::from(b'!') {
                        let node = self.lookbehind(ch == i32::from(b'!'), saved_loops)?;
                        (node, node)
                    } else {
                        let name = self.group_name(ch)?;
                        if self.named_group(&name).is_some() {
                            let message = format!("Named capturing group <{name}> is already defined");
                            return Err(self.error(&message));
                        }
                        let group = self.group_count;
                        self.named_groups.push((name, group));
                        self.plain_group(Some(group))?
                    }
                }
                b'$' | b'@' => return Err(self.error("Unknown group type")),
                _ => {
                    self.unread();
                    self.add_flags();
                    let ch = self.read();
                    if ch == i32::from(b')') {
                        return Ok(None);
                    }
                    if ch != i32::from(b':') {
                        return Err(self.error("Unknown inline modifier"));
                    }
                    self.plain_group(None)?
                }
            }
        } else {
            self.plain_group(Some(self.group_count))?
        };

        self.accept(b')', "Unclosed group")?;
        self.flags = saved_flags;

        let Some(repeat) = self.closure()? else {
            return Ok(Some((head, tail)));
        };
        self.top_loops.truncate(saved_loops);
        if head == tail {
            let node = self.repeat_atom(head, &repeat);
            return Ok(Some((node, node)));
        }
        Ok(Some(self.repeat_group(head, tail, &repeat)))
    }

    /// Parses up to the closing parenthesis of a group; `group` makes it capturing.
    fn plain_group(&mut self, group: Option<u32>) -> Parsed<Chain> {
        let slot = self.local_count;
        self.local_count += 1;
        if group.is_some() {
            self.group_count += 1;
        }
        let tail = self.push(Kind::GroupTail { slot, group });
        let head = self.push(Kind::GroupHead { slot });
        let body = self.expr(tail)?;
        self.set_next(head, body);
        Ok((head, tail))
    }

    fn lookbehind(&mut self, negated: bool, saved_loops: usize) -> Parsed<NodeId> {
        let start = self.cursor;
        let (cond, tail) = self.plain_group(None)?;
        let end = self.push(Kind::LookBehindEnd);
        self.set_next(tail, end);
        let info = self.study(cond);
        if !info.max_valid {
            return Err(self.error("Look-behind group does not have an obvious maximum length"));
        }
        let wide = self.text.get(start..self.length).is_some_and(|rest| rest.iter().copied().any(is_supplementary));
        self.top_loops.truncate(saved_loops);
        Ok(self.push(Kind::Lookbehind { cond, min: info.min_length, max: info.max_length, negated, wide }))
    }

    /// The name of a named group, read up to and including `>`; `first` is already consumed.
    pub(super) fn group_name(&mut self, first: i32) -> Parsed<String> {
        if !charclass::ascii_alpha(first as u32) {
            return Err(self.error("capturing group name does not start with a Latin letter"));
        }
        let mut name = String::new();
        let mut ch = first;
        while charclass::ascii_alnum(ch as u32) {
            name.push(char::from(ascii(ch)));
            ch = self.read();
        }
        if ch != i32::from(b'>') {
            return Err(self.error("named capturing group is missing trailing '>'"));
        }
        Ok(name)
    }

    /// The number of the named group `name`.
    pub(super) fn named_group(&self, name: &str) -> Option<u32> {
        self.named_groups.iter().find(|(known, _)| known == name).map(|(_, group)| *group)
    }

    fn flag_of(ch: i32) -> Option<u32> {
        Some(match ascii(ch) {
            b'i' => CASE_INSENSITIVE,
            b'm' => MULTILINE,
            b's' => DOTALL,
            b'd' => UNIX_LINES,
            b'u' => UNICODE_CASE,
            b'c' => CANON_EQ,
            b'x' => COMMENTS,
            b'U' => UNICODE_CHARACTER_CLASS | UNICODE_CASE,
            _ => return None,
        })
    }

    /// Applies `idmsuxU-idmsuxU`, stopping at the first other character.
    fn add_flags(&mut self) {
        let mut ch = self.peek();
        while let Some(flag) = Parser::flag_of(ch) {
            self.flags |= flag;
            ch = self.next();
        }
        if ch == i32::from(b'-') {
            ch = self.next();
            while let Some(flag) = Parser::flag_of(ch) {
                self.flags &= !flag;
                ch = self.next();
            }
        }
    }

    /// Parses an optional quantifier.
    fn closure(&mut self) -> Parsed<Option<Repeat>> {
        let repeat = match ascii(self.peek()) {
            b'?' => {
                let ch = self.next();
                Repeat { min: 0, max: 1, quantifier: self.quantifier_suffix(ch), shape: Shape::Optional }
            }
            b'*' => self.open_repeat(0),
            b'+' => self.open_repeat(1),
            b'{' => self.counted_repeat()?,
            _ => return Ok(None),
        };
        Ok(Some(repeat))
    }

    /// Consumes `?` or `+` when `ch` is one, otherwise the quantifier stays greedy.
    fn quantifier_suffix(&mut self, ch: i32) -> Quantifier {
        match ascii(ch) {
            b'?' => {
                self.next();
                Quantifier::Lazy
            }
            b'+' => {
                self.next();
                Quantifier::Possessive
            }
            _ => Quantifier::Greedy,
        }
    }

    /// The cursor is on the last character of `*`, `+` or `{n,}`.
    fn open_repeat(&mut self, min: i32) -> Repeat {
        let ch = self.next();
        Repeat { min, max: MAX_REPS, quantifier: self.quantifier_suffix(ch), shape: Shape::Open }
    }

    fn repeat_digit(&self, value: i32, ch: i32) -> Parsed<i32> {
        value
            .checked_mul(10)
            .and_then(|value| value.checked_add(ch - i32::from(b'0')))
            .ok_or_else(|| self.error("Illegal repetition range"))
    }

    fn counted_repeat(&mut self) -> Parsed<Repeat> {
        let mut ch = self.skip();
        if !charclass::ascii_digit(ch as u32) {
            return Err(self.error("Illegal repetition"));
        }
        let mut min = 0;
        while charclass::ascii_digit(ch as u32) {
            min = self.repeat_digit(min, ch)?;
            ch = self.read();
        }
        let mut max = min;
        if ch == i32::from(b',') {
            ch = self.read();
            if ch == i32::from(b'}') {
                self.unread();
                return Ok(self.open_repeat(min));
            }
            max = 0;
            while charclass::ascii_digit(ch as u32) {
                max = self.repeat_digit(max, ch)?;
                ch = self.read();
            }
        }
        if ch != i32::from(b'}') {
            return Err(self.error("Unclosed counted closure"));
        }
        if max < min {
            return Err(self.error("Illegal repetition range"));
        }
        let ch = self.peek();
        Ok(Repeat {
            min,
            max,
            quantifier: self.quantifier_suffix(ch),
            shape: if (min, max) == (0, 1) { Shape::Optional } else { Shape::Bounded },
        })
    }

    /// Applies `repeat` to a single-node atom.
    fn repeat_atom(&mut self, atom: NodeId, repeat: &Repeat) -> NodeId {
        let Repeat { min, max, quantifier, shape } = *repeat;
        if shape == Shape::Optional {
            return self.push(Kind::Ques { atom, quantifier });
        }
        if shape == Shape::Open && quantifier == Quantifier::Greedy {
            if let Some(Node { kind: kind @ Kind::Char(_), .. }) = self.nodes.get_mut(atom as usize) {
                if let Kind::Char(class) = std::mem::replace(kind, Kind::Accept) {
                    *kind = Kind::CharRun { class, min };
                }
                return atom;
            }
        }
        self.push(Kind::Curly { atom, min, max, quantifier })
    }

    /// Applies `repeat` to the group `head..=tail`.
    fn repeat_group(&mut self, head: NodeId, tail: NodeId, repeat: &Repeat) -> Chain {
        let Repeat { min, max, quantifier, shape } = *repeat;
        if quantifier == Quantifier::Possessive {
            let node = self.repeat_atom(head, repeat);
            return (node, node);
        }
        let lazy = quantifier == Quantifier::Lazy;
        if shape == Shape::Optional {
            let conn = self.push(Kind::BranchConn);
            self.set_next(tail, conn);
            let atoms = if lazy { vec![None, Some(head)] } else { vec![Some(head), None] };
            return (self.push(Kind::Branch { atoms, conn }), conn);
        }

        let (slot, group) = match self.nodes.get(tail as usize) {
            Some(Node { kind: Kind::GroupTail { slot, group }, .. }) => (*slot, *group),
            _ => (0, None),
        };
        if self.study(head).deterministic {
            // An iteration ends at the tail, where the repetition takes over.
            if let Some(node) = self.nodes.get_mut(tail as usize) {
                node.kind = Kind::Accept;
            }
            let node = self.push(Kind::GroupCurly { atom: self.next_of(head), min, max, lazy, group });
            return (node, node);
        }

        let count = self.local_count;
        self.local_count += 1;
        let target = self.push(Kind::Loop { body: head, begin: slot, count, min, max, lazy, memo: None });
        self.set_next(tail, target);
        if !lazy && max == MAX_REPS {
            self.top_loops.push(target);
        }
        (self.push(Kind::LoopEntry { target }), target)
    }
}
