//! Coordinate-array implementation used for differential checks.
use crate::board::{Position, Side, Turn};

#[derive(Clone, Copy)]
struct Board {
    cells: [i8; 64],
    side: i8,
}

fn cell(square: u8) -> usize {
    let row = square as usize / 4;
    8 * row + 2 * (square as usize % 4) + (row + 1) % 2
}

fn number(cell: usize) -> u8 {
    (cell / 2) as u8
}

impl From<Position> for Board {
    fn from(position: Position) -> Self {
        let mut cells = [0; 64];
        for square in 0..32 {
            let flag = 1 << square;
            let sign = if position.mask(Side::Black) & flag != 0 {
                1
            } else if position.mask(Side::White) & flag != 0 {
                -1
            } else {
                0
            };
            cells[cell(square)] = sign * (1 + (position.kings() & flag != 0) as i8);
        }
        Self {
            cells,
            side: if position.side() == Side::Black {
                1
            } else {
                -1
            },
        }
    }
}

fn offset(source: usize, row_delta: i8, col_delta: i8) -> Option<usize> {
    let row = (source / 8) as i8 + row_delta;
    let col = (source % 8) as i8 + col_delta;
    if (0..8).contains(&row) && (0..8).contains(&col) {
        Some((row * 8 + col) as usize)
    } else {
        None
    }
}

fn crown(cell: usize, sign: i8) -> bool {
    cell / 8 == if sign > 0 { 7 } else { 0 }
}

fn captures(board: &mut Board, current: usize, path: Turn, output: &mut Vec<Turn>) {
    let piece = board.cells[current];
    let stop = path.is_capture() && piece.abs() == 1 && crown(current, piece);
    let mut extended = false;
    if !stop {
        for dr in [-1, 1] {
            if piece.abs() == 1 && dr != piece {
                continue;
            }
            for dc in [-1, 1] {
                let Some(landing) = offset(current, dr * 2, dc * 2) else {
                    continue;
                };
                let jumped = offset(current, dr, dc).unwrap();
                let target = board.cells[jumped];
                if board.cells[landing] != 0
                    || !matches!(target, -2..=-1 | 1..=2)
                    || target.signum() == piece.signum()
                {
                    continue;
                }
                board.cells[current] = 0;
                board.cells[landing] = piece;
                board.cells[jumped] = 3;
                captures(
                    board,
                    landing,
                    path.step(number(landing), 1 << number(jumped)),
                    output,
                );
                board.cells[jumped] = target;
                board.cells[landing] = 0;
                board.cells[current] = piece;
                extended = true;
            }
        }
    }
    if !extended && path.is_capture() {
        output.push(path);
    }
}

pub fn generate(position: Position, output: &mut Vec<Turn>) {
    output.clear();
    let mut board = Board::from(position);
    if !board
        .cells
        .iter()
        .any(|piece| piece.signum() == -board.side)
    {
        return;
    }
    for source in 0..64 {
        if board.cells[source].signum() == board.side {
            captures(&mut board, source, Turn::origin(number(source)), output);
        }
    }
    if !output.is_empty() {
        return;
    }
    for (source, &piece) in board.cells.iter().enumerate() {
        if piece.signum() != board.side {
            continue;
        }
        for dr in [-1, 1] {
            if piece.abs() == 1 && dr != piece {
                continue;
            }
            for dc in [-1, 1] {
                if let Some(landing) = offset(source, dr, dc).filter(|&at| board.cells[at] == 0) {
                    output.push(Turn::origin(number(source)).step(number(landing), 0));
                }
            }
        }
    }
}

pub fn advance(position: Position, turn: Turn) -> Position {
    let mut board = Board::from(position);
    let mut current = cell(turn.from());
    let piece = board.cells[current];
    board.cells[current] = 0;
    for &square in &turn.path()[1..] {
        let landing = cell(square);
        if turn.is_capture() {
            let row = (current / 8 + landing / 8) / 2;
            let col = (current % 8 + landing % 8) / 2;
            board.cells[row * 8 + col] = 0;
        }
        current = landing;
    }
    board.cells[current] = if crown(current, piece.signum()) {
        2 * piece.signum()
    } else {
        piece
    };
    let mut pieces = [0; 2];
    let mut kings = 0;
    for square in 0..32 {
        let piece = board.cells[cell(square)];
        if piece != 0 {
            pieces[usize::from(piece < 0)] |= 1 << square;
            if piece.abs() == 2 {
                kings |= 1 << square;
            }
        }
    }
    Position::from_valid_masks(pieces, kings, position.side().other())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{board::resolve_turn, moves};

    fn route_names(turns: &[Turn]) -> Vec<String> {
        let mut names = turns.iter().map(ToString::to_string).collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    #[test]
    fn published_initial_counts() {
        for (depth, expected) in [1, 7, 49, 302, 1469, 7361, 36768, 179740, 845931]
            .into_iter()
            .enumerate()
        {
            assert_eq!(moves::perft(Position::start(), depth as u8), expected);
        }
    }

    #[test]
    fn directed_capture_cases() {
        for (fen, expected) in [
            (
                "B:BK14:W10,11,18,19",
                &["14x23x16x7x14", "14x7x16x23x14"][..],
            ),
            ("B:B22:W26,27", &["22x31"][..]),
            ("B:B10:W6", &["10-14", "10-15"][..]),
            ("B:B28:WK32,K31", &[][..]),
        ] {
            let position = fen.parse().unwrap();
            assert_eq!(route_names(&moves::legal(position)), expected);
        }
        let loop_capture = "B:BK14:W10,11,18,19".parse().unwrap();
        assert!(
            resolve_turn(loop_capture, "14x14")
                .unwrap_err()
                .contains("ambiguous")
        );
    }

    #[test]
    fn deterministic_transition_campaign() {
        let mut rng = 0x2e23_4319_9326_8134u64;
        let mut position = Position::start();
        let mut fast = Vec::new();
        let mut slow = Vec::new();
        let mut reflected = Vec::new();
        let mut count = 0;
        while count < 100_000 {
            moves::generate(position, &mut fast);
            generate(position, &mut slow);
            fast.sort_unstable_by(|a, b| a.path().cmp(b.path()));
            slow.sort_unstable_by(|a, b| a.path().cmp(b.path()));
            assert_eq!(fast, slow, "{position}");
            if fast.is_empty() || (count % 191 == 0 && count > 0) {
                position = Position::start();
                if fast.is_empty() {
                    continue;
                }
                moves::generate(position, &mut fast);
            }
            moves::generate(position.flipped(), &mut reflected);
            assert_eq!(fast.len(), reflected.len());
            assert_eq!(position.flipped().flipped(), position);
            for &turn in &fast {
                assert!(reflected.contains(&turn.flipped()));
                let mut next = position;
                let undo = next.make(turn);
                assert_eq!(next, advance(position, turn));
                assert_eq!(next.key(), next.recompute_key());
                assert_eq!(
                    next.flipped(),
                    position.flipped().play(turn.flipped()).unwrap()
                );
                next.unmake(turn, undo);
                assert_eq!(next, position);
            }
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            position.make(fast[rng as usize % fast.len()]);
            count += 1;
        }
        eprintln!("{count} replayed transitions with independent move and state comparisons");
    }
}
