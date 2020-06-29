//! Bounded single-game numeric PDN 3 replay and canonical export.
use crate::board::{Position, Side, Turn, resolve_turn};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fmt, fmt::Write};

pub const MAX_INPUT: usize = 256 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "byte {}: {}", self.offset, self.message)
    }
}

#[derive(Clone)]
pub struct Record {
    pub start: Position,
    pub tags: Vec<(String, String)>,
    pub line: Line,
    pub result: String,
    pub sha256: String,
    pub diagnostic: Option<Diagnostic>,
}

#[derive(Clone, Default)]
pub struct Line {
    pub introduction: Vec<String>,
    pub plies: Vec<Ply>,
}

#[derive(Clone)]
pub struct Ply {
    pub turn: Turn,
    pub before: Position,
    pub after: Position,
    pub annotations: Vec<Annotation>,
}

#[derive(Clone)]
pub enum Annotation {
    Comment(String),
    Glyph(String),
    Variation(Line),
}

#[derive(Debug)]
enum Token<'a> {
    Tag(&'a str, String),
    Comment(&'a str),
    Open,
    Close,
    Glyph(&'a str),
    Number(u32, bool),
    Move(String),
    Result(&'a str),
}

struct Lexer<'a> {
    input: &'a str,
    cursor: usize,
}

const RESULTS: [&str; 8] = ["1/2-1/2", "1-0", "0-1", "0-0", "2-0", "0-2", "1-1", "*"];

impl<'a> Lexer<'a> {
    fn rest(&self) -> &'a str {
        &self.input[self.cursor..]
    }

    fn byte(&self) -> Option<u8> {
        self.rest().as_bytes().first().copied()
    }

    fn error(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            offset: self.cursor,
            message: message.into(),
        }
    }

    fn take_while(&mut self, test: impl Fn(u8) -> bool) -> &'a str {
        let rest = self.rest();
        let size = rest.bytes().take_while(|&byte| test(byte)).count();
        self.cursor += size;
        &rest[..size]
    }

    fn ws(&mut self) {
        self.take_while(|b| b.is_ascii_whitespace());
    }

    fn next(&mut self) -> Result<Option<(usize, Token<'a>)>, Diagnostic> {
        self.ws();
        let start = self.cursor;
        let Some(byte) = self.byte() else {
            return Ok(None);
        };
        let token = match byte {
            b'[' => {
                self.cursor += 1;
                self.ws();
                let key = self.take_while(|b| b.is_ascii_alphanumeric() || b == b'_');
                if key.is_empty() || !key.as_bytes()[0].is_ascii_alphabetic() {
                    return Err(self.error("expected tag name"));
                }
                self.ws();
                if self.byte() != Some(b'"') {
                    return Err(self.error("expected quoted tag value"));
                }
                self.cursor += 1;
                let mut value = String::new();
                loop {
                    let ch = self
                        .rest()
                        .chars()
                        .next()
                        .ok_or_else(|| self.error("unterminated tag string"))?;
                    self.cursor += ch.len_utf8();
                    match ch {
                        '"' => break,
                        '\\' => {
                            let escaped = self
                                .byte()
                                .filter(|b| matches!(b, b'"' | b'\\'))
                                .ok_or_else(|| self.error("invalid tag escape"))?;
                            self.cursor += 1;
                            value.push(escaped as char);
                        }
                        _ if ch.is_control() => return Err(self.error("control character in tag")),
                        _ => value.push(ch),
                    }
                }
                self.ws();
                if self.byte() != Some(b']') {
                    return Err(self.error("expected closing tag bracket"));
                }
                self.cursor += 1;
                Token::Tag(key, value)
            }
            b'{' | b';' => {
                self.cursor += 1;
                let rest = self.rest();
                let size = if byte == b'{' {
                    rest.find('}')
                        .ok_or_else(|| self.error("unterminated brace comment"))?
                } else {
                    rest.find('\n').unwrap_or(rest.len())
                };
                self.cursor += size + usize::from(byte == b'{');
                Token::Comment(&rest[..size])
            }
            b'(' | b')' => {
                self.cursor += 1;
                if byte == b'(' {
                    Token::Open
                } else {
                    Token::Close
                }
            }
            b'!' | b'?' => Token::Glyph(self.take_while(|b| matches!(b, b'!' | b'?'))),
            b'$' => {
                self.cursor += 1;
                if self.take_while(|b| b.is_ascii_digit()).is_empty() {
                    return Err(self.error("NAG requires a numeric identifier"));
                }
                Token::Glyph(&self.input[start..self.cursor])
            }
            _ if RESULTS.iter().any(|r| {
                self.rest()
                    .strip_prefix(r)
                    .is_some_and(|tail| tail.bytes().next().is_none_or(is_separator))
            }) =>
            {
                let result = RESULTS
                    .into_iter()
                    .find(|r| {
                        self.rest()
                            .strip_prefix(r)
                            .is_some_and(|tail| tail.bytes().next().is_none_or(is_separator))
                    })
                    .unwrap();
                self.cursor += result.len();
                Token::Result(result)
            }
            b'0'..=b'9' => {
                let digits = self.take_while(|b| b.is_ascii_digit());
                if self.byte() == Some(b'.') {
                    let dots = self.take_while(|b| b == b'.').len();
                    if !matches!(dots, 1 | 3) {
                        return Err(self.error("move number needs one or three dots"));
                    }
                    let number = digits
                        .parse()
                        .map_err(|_| self.error("move number exceeds limit"))?;
                    Token::Number(number, dots == 3)
                } else {
                    let mut route = digits.to_string();
                    loop {
                        let checkpoint = self.cursor;
                        self.ws();
                        let Some(mark @ (b'-' | b'x' | b'X')) = self.byte() else {
                            self.cursor = checkpoint;
                            break;
                        };
                        self.cursor += 1;
                        self.ws();
                        let landing = self.take_while(|b| b.is_ascii_digit());
                        if landing.is_empty() || route.len() > 48 {
                            return Err(self.error("missing landing or oversized route"));
                        }
                        route.push(mark as char);
                        route.push_str(landing);
                    }
                    Token::Move(route)
                }
            }
            _ => {
                return Err(self.error(
                    "unsupported token. Use numeric GameType 21, x captures, and a header FEN.",
                ));
            }
        };
        Ok(Some((start, token)))
    }
}

fn is_separator(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'{' | b';' | b')' | b'[')
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    pending: Option<(usize, Token<'a>)>,
    turns: usize,
    legacy_dash: bool,
}

impl<'a> Parser<'a> {
    fn next(&mut self) -> Result<Option<(usize, Token<'a>)>, Diagnostic> {
        if self.pending.is_some() {
            return Ok(self.pending.take());
        }
        self.lexer.next()
    }

    fn line(
        &mut self,
        line: &mut Line,
        mut position: Position,
        mut number: u32,
        depth: u8,
    ) -> Result<Option<&'a str>, Diagnostic> {
        let mut numbered = false;
        while let Some((offset, token)) = self.next()? {
            let error = |message: &str| Diagnostic {
                offset,
                message: message.into(),
            };
            match token {
                Token::Number(n, second) => {
                    if numbered || n != number || second != (position.side() == Side::White) {
                        return Err(error("move number conflicts with the replay position"));
                    }
                    numbered = true;
                }
                Token::Move(text) => {
                    self.turns += 1;
                    if self.turns > 6000 {
                        return Err(error("game and variations exceed 6000 turns"));
                    }
                    let text = if self.legacy_dash
                        && !text.contains(['x', 'X'])
                        && crate::moves::legal(position)
                            .first()
                            .is_some_and(|turn| turn.is_capture())
                    {
                        text.replace('-', "x")
                    } else {
                        text
                    };
                    let turn = resolve_turn(position, &text)
                        .map_err(|message| Diagnostic { offset, message })?;
                    let mut after = position;
                    after.make(turn);
                    line.plies.push(Ply {
                        turn,
                        before: position,
                        after,
                        annotations: Vec::new(),
                    });
                    if position.side() == Side::White {
                        number += 1;
                    }
                    position = after;
                    numbered = false;
                }
                Token::Comment(text) => {
                    if let Some(last) = line.plies.last_mut() {
                        last.annotations.push(Annotation::Comment(text.into()));
                    } else {
                        line.introduction.push(text.into());
                    }
                }
                Token::Glyph(glyph) => {
                    let last = line
                        .plies
                        .last_mut()
                        .filter(|_| !numbered)
                        .ok_or_else(|| error("glyph without preceding turn"))?;
                    last.annotations.push(Annotation::Glyph(glyph.into()));
                }
                Token::Open => {
                    if depth >= 24 || numbered {
                        return Err(error("invalid variation start or nesting exceeds 24"));
                    }
                    let last = line
                        .plies
                        .last_mut()
                        .ok_or_else(|| error("variation requires a preceding turn to replace"))?;
                    let branch_number = number - u32::from(last.before.side() == Side::White);
                    let mut variation = Line::default();
                    self.line(&mut variation, last.before, branch_number, depth + 1)?;
                    if variation.plies.is_empty() {
                        return Err(error("empty variation"));
                    }
                    last.annotations.push(Annotation::Variation(variation));
                }
                Token::Close if depth > 0 && !numbered => return Ok(None),
                Token::Result(result) if depth == 0 && !numbered => return Ok(Some(result)),
                _ => {
                    return Err(error(
                        "unexpected token, game boundary, or result inside variation",
                    ));
                }
            }
        }
        Err(self.lexer.error(if depth == 0 {
            "truncated game. Missing result or *"
        } else {
            "unclosed variation"
        }))
    }
}

pub fn import(input: &str, partial: bool) -> Result<Record, Diagnostic> {
    if input.len() > MAX_INPUT || input.contains('\0') {
        return Err(Diagnostic {
            offset: 0,
            message: "PDN exceeds 256 KiB or contains NUL".into(),
        });
    }
    let mut parser = Parser {
        lexer: Lexer { input, cursor: 0 },
        pending: None,
        turns: 0,
        legacy_dash: false,
    };
    let mut record = Record {
        start: Position::start(),
        tags: Vec::new(),
        line: Line::default(),
        result: "*".into(),
        sha256: format!("{:x}", Sha256::digest(input)),
        diagnostic: None,
    };
    let parsed = (|| {
        let mut seen = HashSet::new();
        let mut header_result = None;
        loop {
            match parser.next()? {
                Some((offset, Token::Tag(key, value))) => {
                    let error = |message: String| Diagnostic { offset, message };
                    if !seen.insert(key.to_ascii_lowercase()) {
                        return Err(error(format!("repeated tag {key}")));
                    }
                    match key {
                        "GameType" if !matches!(value.as_str(), "21" | "21,B,8,8,N1,0") => {
                            return Err(error(format!("unsupported GameType {value}")));
                        }
                        "FEN" => record.start = value.parse().map_err(error)?,
                        "CaptureNotation" => {
                            parser.legacy_dash = match value.as_str() {
                                "Standard" => false,
                                "LegacyDash" => true,
                                _ => return Err(error("unsupported CaptureNotation tag".into())),
                            };
                        }
                        "Result" if !RESULTS.contains(&value.as_str()) => {
                            return Err(error("unsupported Result tag".into()));
                        }
                        "Result" => header_result = Some(value.clone()),
                        "ResultOrder" if !matches!(value.as_str(), "BlackFirst" | "WhiteFirst") => {
                            return Err(error("unsupported ResultOrder tag".into()));
                        }
                        _ => {}
                    }
                    record.tags.push((key.into(), value));
                }
                token => {
                    parser.pending = token;
                    break;
                }
            }
        }
        let result = parser.line(&mut record.line, record.start, 1, 0)?.unwrap();
        if header_result.as_ref().is_some_and(|r| r != result) {
            return Err(parser.lexer.error("Result tag conflicts with movetext"));
        }
        while let Some((offset, token)) = parser.next()? {
            if let Token::Comment(text) = token {
                if let Some(last) = record.line.plies.last_mut() {
                    last.annotations.push(Annotation::Comment(text.into()));
                } else {
                    record.line.introduction.push(text.into());
                }
            } else {
                return Err(Diagnostic {
                    offset,
                    message: "additional data after single game result".into(),
                });
            }
        }
        let final_position = record
            .line
            .plies
            .last()
            .map_or(record.start, |ply| ply.after);
        if let Some(outcome) =
            crate::game::board_outcome(final_position, &crate::moves::legal(final_position))
        {
            let black_first = record
                .tags
                .iter()
                .any(|(key, value)| key == "ResultOrder" && value == "BlackFirst");
            if outcome.winner.is_some_and(|winner| {
                let first = if black_first {
                    Side::Black
                } else {
                    Side::White
                };
                match result {
                    "1-0" | "2-0" => winner != first,
                    "0-1" | "0-2" => winner == first,
                    "1/2-1/2" | "1-1" => true,
                    _ => false,
                }
            }) {
                return Err(parser.lexer.error("result conflicts with terminal board. Check ResultOrder for older English PDN."));
            }
        }
        record.result = result.into();
        Ok(())
    })();
    if let Err(error) = parsed {
        if !partial {
            return Err(error);
        }
        record.diagnostic = Some(error);
    }
    Ok(record)
}

pub fn export(record: &Record) -> String {
    fn comment(output: &mut String, value: &str) {
        if value.contains('}') {
            for line in value.split('\n') {
                let _ = writeln!(output, ";{line}");
            }
        } else {
            let _ = write!(output, "{{{value}}} ");
        }
    }

    fn turns(output: &mut String, line: &Line, mut number: u32) {
        for value in &line.introduction {
            comment(output, value);
        }
        for ply in &line.plies {
            let second = ply.before.side() == Side::White;
            let dots = if second { "..." } else { "." };
            let _ = write!(output, "{number}{dots} {} ", ply.turn);
            for annotation in &ply.annotations {
                match annotation {
                    Annotation::Comment(value) => comment(output, value),
                    Annotation::Glyph(value) => {
                        let _ = write!(output, "{value} ");
                    }
                    Annotation::Variation(branch) => {
                        output.push('(');
                        turns(output, branch, number);
                        output.push_str(") ");
                    }
                }
            }
            number += u32::from(second);
        }
    }

    let mut output = String::from("[GameType \"21\"]\n");
    if record.start != Position::start() {
        let _ = writeln!(output, "[FEN \"{}\"]", record.start);
    }
    for (key, value) in &record.tags {
        if matches!(key.as_str(), "GameType" | "Result" | "FEN") {
            continue;
        }
        let value = value.replace('\\', "\\\\").replace('"', "\\\"");
        let _ = writeln!(output, "[{key} \"{value}\"]");
    }
    let _ = writeln!(output, "[Result \"{}\"]\n", record.result);
    turns(&mut output, &record.line, 1);
    let _ = writeln!(output, "{}", record.result);
    output
}

pub fn completed(
    start: Position,
    turns: impl IntoIterator<Item = Turn>,
    result: &str,
) -> Result<Record, String> {
    if !RESULTS.contains(&result) {
        return Err("unsupported result".into());
    }
    let mut line = Line::default();
    let mut before = start;
    for turn in turns {
        let after = before.play(turn)?;
        line.plies.push(Ply {
            turn,
            before,
            after,
            annotations: Vec::new(),
        });
        before = after;
    }
    Ok(Record {
        start,
        tags: Vec::new(),
        line,
        result: result.into(),
        sha256: String::new(),
        diagnostic: None,
    })
}

/// Byte ranges of complete records in a bounded archive, including trailing comments.
pub fn archive_ranges(input: &str) -> Result<Vec<std::ops::Range<usize>>, Diagnostic> {
    if input.len() > 8 * 1024 * 1024 || input.contains('\0') {
        return Err(Diagnostic {
            offset: 0,
            message: "PDN archive exceeds 8 MiB or contains NUL".into(),
        });
    }
    let mut lexer = Lexer { input, cursor: 0 };
    let mut spans = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut completed = false;
    let mut has_tokens = false;
    while let Some((offset, token)) = lexer.next()? {
        if completed && !matches!(token, Token::Comment(_)) {
            if spans.len() >= 4095 {
                return Err(Diagnostic {
                    offset,
                    message: "PDN archive exceeds 4096 records".into(),
                });
            }
            spans.push(start..offset);
            start = offset;
            completed = false;
        }
        has_tokens = true;
        match token {
            Token::Open => {
                depth += 1;
                if depth > 24 {
                    return Err(Diagnostic {
                        offset,
                        message: "variation nesting exceeds 24".into(),
                    });
                }
            }
            Token::Close => {
                depth = depth.checked_sub(1).ok_or_else(|| Diagnostic {
                    offset,
                    message: "unmatched variation boundary".into(),
                })?;
            }
            Token::Result(_) if depth == 0 => completed = true,
            _ => {}
        }
    }
    if has_tokens {
        spans.push(start..input.len());
    }
    if spans.is_empty() {
        return Err(Diagnostic {
            offset: 0,
            message: "empty PDN archive".into(),
        });
    }
    Ok(spans)
}
