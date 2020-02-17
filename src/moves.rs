use crate::board::{Position, Side, Turn, bit, coordinates, square_at, squares};

#[derive(Clone, Copy)]
struct Ray {
    near: u8,
    far: u8,
}

const fn geometry() -> [[Ray; 4]; 32] {
    let mut table = [[Ray { near: 32, far: 32 }; 4]; 32];
    let mut origin = 0;
    while origin < 32 {
        let (row, col) = coordinates(origin as u8);
        let mut d = 0;
        while d < 4 {
            let dr = if d < 2 { 1 } else { -1 };
            let dc = if d & 1 == 0 { -1 } else { 1 };
            table[origin][d] = Ray {
                near: square_at(row + dr, col + dc),
                far: square_at(row + 2 * dr, col + 2 * dc),
            };
            d += 1;
        }
        origin += 1;
    }
    table
}

const GEOMETRY: [[Ray; 4]; 32] = geometry();

fn rays(square: u8, side: Side, king: bool) -> &'static [Ray] {
    let row = &GEOMETRY[square as usize];
    if king {
        row
    } else if side == Side::Black {
        &row[..2]
    } else {
        &row[2..]
    }
}

struct Capture<'a> {
    blockers: u32,
    enemy: u32,
    side: Side,
    king: bool,
    output: &'a mut Vec<Turn>,
}

impl Capture<'_> {
    fn trace(&mut self, path: Turn) {
        let current = path.to();
        let mut extended = false;
        if path.len == 1 || self.king || !self.side.crown(current) {
            let unused = self.enemy & !path.victims;
            for ray in rays(current, self.side, self.king) {
                if ray.far < 32 && unused & bit(ray.near) != 0 && self.blockers & bit(ray.far) == 0
                {
                    self.trace(path.step(ray.far, bit(ray.near)));
                    extended = true;
                }
            }
        }
        if !extended && path.is_capture() {
            self.output.push(path);
        }
    }
}

pub fn generate(position: Position, output: &mut Vec<Turn>) {
    output.clear();
    let side = position.side();
    let ours = position.mask(side);
    let enemy = position.mask(side.other());
    if enemy == 0 {
        return;
    }
    let occupied = ours | enemy;
    for origin in squares(ours) {
        Capture {
            blockers: occupied ^ bit(origin),
            enemy,
            side,
            king: position.kings() & bit(origin) != 0,
            output,
        }
        .trace(Turn::origin(origin));
    }
    if !output.is_empty() {
        return;
    }
    for origin in squares(ours) {
        let king = position.kings() & bit(origin) != 0;
        for ray in rays(origin, side, king) {
            if ray.near < 32 && occupied & bit(ray.near) == 0 {
                output.push(Turn::origin(origin).step(ray.near, 0));
            }
        }
    }
}

pub fn legal(position: Position) -> Vec<Turn> {
    let mut result = Vec::with_capacity(48);
    generate(position, &mut result);
    result
}

pub fn perft(position: Position, depth: u8) -> u64 {
    fn walk(position: &mut Position, buffers: &mut [Vec<Turn>]) -> u64 {
        let Some((moves, children)) = buffers.split_first_mut() else {
            return 1;
        };
        generate(*position, moves);
        if children.is_empty() {
            return moves.len() as u64;
        }
        let mut leaves = 0;
        for &turn in moves.iter() {
            let undo = position.make(turn);
            leaves += walk(position, children);
            position.unmake(turn, undo);
        }
        leaves
    }
    let mut buffers = (0..depth)
        .map(|_| Vec::with_capacity(48))
        .collect::<Vec<_>>();
    walk(&mut { position }, &mut buffers)
}

#[derive(Default)]
pub(crate) struct Activity {
    pub quiet: i32,
    pub jumps: i32,
    pub blocked: i32,
    pub threatened: u32,
}

pub(crate) fn activity(position: Position, side: Side) -> Activity {
    let mut result = Activity::default();
    let enemy = position.mask(side.other());
    let occupied = position.occupied();
    for source in squares(position.mask(side)) {
        let king = position.kings() & bit(source) != 0;
        let mut open = false;
        for ray in rays(source, side, king) {
            if ray.near < 32 && occupied & bit(ray.near) == 0 {
                result.quiet += 1;
                open = true;
            } else if ray.far < 32 && enemy & bit(ray.near) != 0 && occupied & bit(ray.far) == 0 {
                result.jumps += 1;
                result.threatened |= bit(ray.near);
                open = true;
            }
        }
        if !open {
            result.blocked += 1;
        }
    }
    result
}
