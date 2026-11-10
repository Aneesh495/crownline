use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Side {
    Black,
    White,
}

impl Side {
    pub const ALL: [Self; 2] = [Self::Black, Self::White];

    pub const fn other(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }

    pub const fn crown(self, square: u8) -> bool {
        match self {
            Self::Black => square >= 28,
            Self::White => square < 4,
        }
    }

    pub const fn letter(self) -> char {
        match self {
            Self::Black => 'B',
            Self::White => 'W',
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Position {
    pub(crate) pieces: [u32; 2],
    pub(crate) kings: u32,
    pub(crate) side: Side,
    key: u64,
}

pub(crate) const fn bit(square: u8) -> u32 {
    1 << square
}

pub(crate) fn squares(mut mask: u32) -> impl Iterator<Item = u8> {
    std::iter::from_fn(move || {
        if mask == 0 {
            return None;
        }
        let square = mask.trailing_zeros() as u8;
        mask &= mask - 1;
        Some(square)
    })
}

pub const fn coordinates(square: u8) -> (i8, i8) {
    let row = (square / 4) as i8;
    (row, ((square % 4) * 2) as i8 + 1 - (row & 1))
}

pub(crate) const fn square_at(row: i8, col: i8) -> u8 {
    if row < 0 || row > 7 || col < 0 || col > 7 || (row + col) & 1 == 0 {
        return 32;
    }
    (row * 4 + col / 2) as u8
}

const fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

const TURN_KEY: u64 = mix(129);

const fn piece_key(side: Side, king: bool, square: u8) -> u64 {
    mix(1 + square as u64 + 32 * (side as u64 + 2 * king as u64))
}

impl Position {
    pub fn new(black: u32, white: u32, kings: u32, side: Side) -> Result<Self, String> {
        if black & white != 0 || kings & !(black | white) != 0 {
            return Err("overlapping or unoccupied piece masks".into());
        }
        if black | white == 0 || black.count_ones() > 12 || white.count_ones() > 12 {
            return Err("position must contain pieces and at most 12 of each side".into());
        }
        if black & !kings & 0xf0000000 != 0 || white & !kings & 0xf != 0 {
            return Err("uncrowned man on its promotion row".into());
        }
        Ok(Self::from_valid_masks([black, white], kings, side))
    }

    pub(crate) fn from_valid_masks(pieces: [u32; 2], kings: u32, side: Side) -> Self {
        let mut position = Self {
            pieces,
            kings,
            side,
            key: 0,
        };
        position.key = position.recompute_key();
        position
    }

    pub fn start() -> Self {
        Self::from_valid_masks([0xfff, 0xfff00000], 0, Side::Black)
    }

    pub fn side(self) -> Side {
        self.side
    }

    pub fn mask(self, side: Side) -> u32 {
        self.pieces[side as usize]
    }

    pub fn kings(self) -> u32 {
        self.kings
    }

    pub fn occupied(self) -> u32 {
        self.pieces[0] | self.pieces[1]
    }

    pub fn key(self) -> u64 {
        self.key
    }

    pub fn recompute_key(self) -> u64 {
        let mut key = if self.side == Side::White {
            TURN_KEY
        } else {
            0
        };
        for side in Side::ALL {
            for square in squares(self.mask(side)) {
                key ^= piece_key(side, self.kings & bit(square) != 0, square);
            }
        }
        key
    }

    pub fn flipped(self) -> Self {
        Self::from_valid_masks(
            [self.pieces[1].reverse_bits(), self.pieces[0].reverse_bits()],
            self.kings.reverse_bits(),
            self.side.other(),
        )
    }

    pub fn elimination(self) -> Option<Side> {
        Side::ALL
            .into_iter()
            .find(|&side| self.mask(side.other()) == 0)
    }

    pub fn irreversible(self, turn: Turn) -> bool {
        turn.is_capture() || self.kings & bit(turn.from()) == 0
    }

    pub(crate) fn make(&mut self, turn: Turn) -> Undo {
        let from = turn.from();
        let to = turn.to();
        let ours = self.side;
        let theirs = ours.other();
        let was_king = self.kings & bit(from) != 0;
        let is_king = was_king || ours.crown(to);
        let victims = turn.victims;
        let captured_kings = victims & self.kings;
        self.key ^= TURN_KEY ^ piece_key(ours, was_king, from) ^ piece_key(ours, is_king, to);
        for square in squares(victims) {
            self.key ^= piece_key(theirs, captured_kings & bit(square) != 0, square);
        }
        self.pieces[ours as usize] ^= bit(from) ^ bit(to);
        self.pieces[theirs as usize] ^= victims;
        self.kings &= !(bit(from) | victims);
        if is_king {
            self.kings |= bit(to);
        }
        self.side = theirs;
        Undo {
            was_king,
            captured_kings,
        }
    }

    pub(crate) fn unmake(&mut self, turn: Turn, undo: Undo) {
        let ours = self.side.other();
        let from = turn.from();
        let to = turn.to();
        let is_king = self.kings & bit(to) != 0;
        self.key ^= TURN_KEY ^ piece_key(ours, undo.was_king, from) ^ piece_key(ours, is_king, to);
        for square in squares(turn.victims) {
            self.key ^= piece_key(self.side, undo.captured_kings & bit(square) != 0, square);
        }
        self.pieces[ours as usize] ^= bit(from) ^ bit(to);
        self.pieces[self.side as usize] |= turn.victims;
        self.kings = (self.kings & !bit(to)) | undo.captured_kings;
        if undo.was_king {
            self.kings |= bit(from);
        }
        self.side = ours;
    }

    pub fn play(mut self, turn: Turn) -> Result<Self, String> {
        if !crate::moves::legal(self).contains(&turn) {
            return Err(format!("illegal turn {turn}"));
        }
        self.make(turn);
        Ok(self)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Undo {
    was_king: bool,
    captured_kings: u32,
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.side.letter())?;
        for side in Side::ALL {
            write!(f, ":{}", side.letter())?;
            for (index, square) in squares(self.mask(side)).enumerate() {
                let prefix = if self.kings & bit(square) == 0 {
                    ""
                } else {
                    "K"
                };
                if index != 0 {
                    write!(f, ",")?;
                }
                write!(f, "{prefix}{}", square + 1)?;
            }
        }
        Ok(())
    }
}

fn parse_side(side: &str) -> Result<Side, String> {
    match side {
        "B" => Ok(Side::Black),
        "W" => Ok(Side::White),
        _ => Err("side must be B or W".into()),
    }
}

pub(crate) fn parse_square(token: &str) -> Result<u8, String> {
    if token.is_empty() || !token.bytes().all(|c| c.is_ascii_digit()) {
        return Err(format!("invalid numeric square {token:?}"));
    }
    match token.parse::<u8>() {
        Ok(value @ 1..=32) => Ok(value - 1),
        _ => Err(format!("square {token:?} outside 1 through 32")),
    }
}

impl FromStr for Position {
    type Err = String;

    fn from_str(fen: &str) -> Result<Self, Self::Err> {
        if !fen.is_ascii() || fen.len() > 256 {
            return Err("position must be at most 256 ASCII bytes".into());
        }
        let mut fields = fen.split(':');
        let side = parse_side(fields.next().unwrap_or(""))?;
        let mut pieces = [None; 2];
        let mut kings = 0;
        for field in fields {
            let (name, list) = field.split_at_checked(1).ok_or("empty side field")?;
            let slot = &mut pieces[parse_side(name)? as usize];
            if slot.is_some() {
                return Err(format!("repeated {name} side field"));
            }
            let mut mask = 0;
            if !list.is_empty() {
                for token in list.split(',') {
                    let (king, range) = token
                        .strip_prefix('K')
                        .map_or((false, token), |s| (true, s));
                    let (start, end) = match range.split_once('-') {
                        Some((a, b)) => (parse_square(a)?, parse_square(b)?),
                        None => {
                            let sq = parse_square(range)?;
                            (sq, sq)
                        }
                    };
                    if start > end {
                        return Err("descending square range".into());
                    }
                    for sq in start..=end {
                        if mask & bit(sq) != 0 {
                            return Err(format!("duplicate square {}", sq + 1));
                        }
                        mask |= bit(sq);
                        if king {
                            kings |= bit(sq);
                        }
                    }
                }
            }
            *slot = Some(mask);
        }
        Self::new(
            pieces[0].ok_or("missing black side list")?,
            pieces[1].ok_or("missing white side list")?,
            kings,
            side,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Turn {
    pub(crate) path: [u8; 13],
    pub(crate) len: u8,
    pub(crate) victims: u32,
}

impl Turn {
    pub(crate) fn origin(square: u8) -> Self {
        let mut path = [0; 13];
        path[0] = square;
        Self {
            path,
            len: 1,
            victims: 0,
        }
    }

    pub(crate) fn step(mut self, landing: u8, victim: u32) -> Self {
        self.path[self.len as usize] = landing;
        self.len += 1;
        self.victims |= victim;
        self
    }

    pub fn path(&self) -> &[u8] {
        &self.path[..self.len as usize]
    }

    pub fn from(self) -> u8 {
        self.path[0]
    }

    pub fn to(self) -> u8 {
        self.path[self.len as usize - 1]
    }

    pub fn is_capture(self) -> bool {
        self.victims != 0
    }

    pub fn captured(self) -> u32 {
        self.victims
    }

    pub fn flipped(self) -> Self {
        let mut result = Self::origin(31 - self.from());
        for &square in &self.path()[1..] {
            result = result.step(31 - square, 0);
        }
        result.victims = self.victims.reverse_bits();
        result
    }
}

impl fmt::Display for Turn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let delimiter = if self.is_capture() { 'x' } else { '-' };
        write!(f, "{}", self.from() + 1)?;
        for &square in &self.path()[1..] {
            write!(f, "{delimiter}{}", square + 1)?;
        }
        Ok(())
    }
}

pub fn resolve_turn(position: Position, text: &str) -> Result<Turn, String> {
    let capture = text.contains(['x', 'X']);
    let path = text
        .split(if capture { &['x', 'X'][..] } else { &['-'][..] })
        .map(parse_square)
        .collect::<Result<Vec<_>, _>>()?;
    if !(2..=13).contains(&path.len()) || (!capture && path.len() != 2) {
        return Err(format!("invalid turn notation {text:?}"));
    }
    let moves = crate::moves::legal(position);
    let mut matches = moves.iter().copied().filter(|turn| {
        turn.is_capture() == capture
            && (turn.path() == path
                || (capture && path.len() == 2 && turn.from() == path[0] && turn.to() == path[1]))
    });
    let first = matches
        .next()
        .ok_or_else(|| format!("illegal turn {text} from {position}"))?;
    if matches.next().is_some() {
        return Err(format!(
            "ambiguous capture {text}. Give all landing squares."
        ));
    }
    Ok(first)
}
