use crate::{
    board::{Position, Side, Turn, resolve_turn},
    moves,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    Tournament,
    House,
    Study,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Elimination,
    Immobility,
    Repetition,
    NoProgress,
    Agreement,
    Resignation,
    Flag,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Outcome {
    pub winner: Option<Side>,
    pub reason: Reason,
}

impl Outcome {
    pub fn win(side: Side, reason: Reason) -> Self {
        Self {
            winner: Some(side),
            reason,
        }
    }

    pub fn draw(reason: Reason) -> Self {
        Self {
            winner: None,
            reason,
        }
    }
}

pub fn board_outcome(position: Position, legal: &[Turn]) -> Option<Outcome> {
    if let Some(winner) = position.elimination() {
        return Some(Outcome::win(winner, Reason::Elimination));
    }
    if legal.is_empty() {
        return Some(Outcome::win(position.side().other(), Reason::Immobility));
    }
    None
}

#[derive(Clone)]
pub struct History {
    positions: Vec<Position>,
    starts: Vec<usize>,
}

impl History {
    pub fn new(position: Position) -> Self {
        Self {
            positions: vec![position],
            starts: vec![0],
        }
    }

    pub fn current(&self) -> Position {
        *self.positions.last().unwrap()
    }

    pub(crate) fn push_generated(&mut self, turn: Turn, next: Position) {
        let start = if self.current().irreversible(turn) {
            self.positions.len()
        } else {
            *self.starts.last().unwrap()
        };
        self.positions.push(next);
        self.starts.push(start);
    }

    pub fn play(&mut self, turn: Turn) -> Result<(), String> {
        let next = self.current().play(turn)?;
        self.push_generated(turn, next);
        Ok(())
    }

    pub(crate) fn pop(&mut self) {
        assert!(self.positions.len() > 1);
        self.positions.pop();
        self.starts.pop();
    }

    pub fn reversible(&self) -> &[Position] {
        &self.positions[*self.starts.last().unwrap()..]
    }

    pub fn quiet_plies(&self) -> usize {
        self.reversible().len() - 1
    }

    pub fn prospective_repetition(&self, turn: Turn) -> Result<bool, String> {
        let next = self.current().play(turn)?;
        Ok(self.repeated_after(turn, next))
    }

    pub(crate) fn repeated_after(&self, turn: Turn, next: Position) -> bool {
        !self.current().irreversible(turn)
            && self
                .reversible()
                .iter()
                .filter(|&&position| position == next)
                .take(2)
                .count()
                == 2
    }

    pub fn house_draw(&self) -> Option<Outcome> {
        let current = self.current();
        if self
            .reversible()
            .iter()
            .filter(|&&p| p == current)
            .take(3)
            .count()
            == 3
        {
            Some(Outcome::draw(Reason::Repetition))
        } else if self.quiet_plies() >= 80 {
            Some(Outcome::draw(Reason::NoProgress))
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClockSpec {
    pub initial_ms: u64,
    pub increment_ms: u64,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ClockView {
    pub black_ms: u64,
    pub white_ms: u64,
    pub paused: bool,
}

#[derive(Clone, Copy)]
struct Clock {
    balance: [u64; 2],
    spec: ClockSpec,
    anchor: u64,
    paused: bool,
}

impl Clock {
    fn time(self, side: Side, now: u64) -> u64 {
        let elapsed = if self.paused {
            0
        } else {
            now.saturating_sub(self.anchor)
        };
        self.balance[side as usize].saturating_sub(elapsed)
    }

    fn view(self, side: Side, now: u64) -> ClockView {
        let mut balance = self.balance;
        balance[side as usize] = self.time(side, now);
        ClockView {
            black_ms: balance[0],
            white_ms: balance[1],
            paused: self.paused,
        }
    }

    fn commit(&mut self, side: Side, now: u64) {
        self.balance[side as usize] = self.time(side, now).saturating_add(self.spec.increment_ms);
        self.anchor = now;
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Move { turn: String },
    ClaimRepetition { turn: String },
    ClaimNoProgress,
    OfferDraw,
    AcceptDraw,
    Resign,
    Pause,
    Resume,
    Takeback,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    Move {
        turn: String,
    },
    DrawOffered,
    Paused,
    Resumed,
    Takeback {
        turn: String,
    },
    Finished {
        outcome: Outcome,
        intended_turn: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub at_unix_ms: u64,
    pub actor: Side,
    pub change: EventKind,
}

struct Played {
    turn: Turn,
    clock: Option<Clock>,
}

pub struct Game {
    history: History,
    played: Vec<Played>,
    events: Vec<Event>,
    offer: Option<Side>,
    outcome: Option<Outcome>,
    clock: Option<Clock>,
    now: u64,
    policy: Policy,
    local: bool,
}

impl Game {
    pub fn new(
        position: Position,
        policy: Policy,
        spec: Option<ClockSpec>,
        local: bool,
        now: u64,
    ) -> Result<Self, String> {
        if let Some(spec) = spec {
            if policy == Policy::Study
                || !(1..=86_400_000).contains(&spec.initial_ms)
                || spec.increment_ms > 600_000
            {
                return Err("invalid clock for this session".into());
            }
        }
        Ok(Self {
            history: History::new(position),
            played: Vec::new(),
            events: Vec::new(),
            offer: None,
            outcome: board_outcome(position, &moves::legal(position)),
            clock: spec.map(|spec| Clock {
                balance: [spec.initial_ms; 2],
                spec,
                anchor: now,
                paused: false,
            }),
            now,
            policy,
            local,
        })
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn position(&self) -> Position {
        self.history.current()
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn turns(&self) -> impl Iterator<Item = Turn> + '_ {
        self.played.iter().map(|played| played.turn)
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    pub fn offer(&self) -> Option<Side> {
        self.offer
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    pub fn clock(&self) -> Option<ClockView> {
        self.clock
            .map(|clock| clock.view(self.position().side(), self.now))
    }

    pub fn legal(&self) -> Vec<Turn> {
        if self.outcome.is_some() || self.clock.is_some_and(|clock| clock.paused) {
            Vec::new()
        } else {
            moves::legal(self.position())
        }
    }

    fn record(&mut self, actor: Side, at_unix_ms: u64, change: EventKind) {
        self.events.push(Event {
            actor,
            at_unix_ms,
            change,
        });
    }

    fn finish(&mut self, outcome: Outcome, actor: Side, at: u64, intended_turn: Option<String>) {
        self.outcome = Some(outcome);
        self.offer = None;
        let side = self.position().side();
        if let Some(clock) = &mut self.clock {
            clock.balance[side as usize] = clock.time(side, self.now);
            clock.paused = true;
        }
        self.record(
            actor,
            at,
            EventKind::Finished {
                outcome,
                intended_turn,
            },
        );
    }

    pub fn tick(&mut self, now: u64, at: u64) -> Result<(), String> {
        if now < self.now {
            return Err("monotonic time went backwards".into());
        }
        self.now = now;
        let side = self.position().side();
        if self.outcome.is_none() && self.clock.is_some_and(|clock| clock.time(side, now) == 0) {
            self.finish(Outcome::win(side.other(), Reason::Flag), side, at, None);
        }
        Ok(())
    }

    pub fn apply(
        &mut self,
        actor: Side,
        command: Command,
        now: u64,
        at: u64,
    ) -> Result<(), String> {
        self.tick(now, at)?;
        if self.events.len() >= 20_000 {
            return Err("game event limit reached".into());
        }
        if matches!(command, Command::Takeback) && self.local && self.policy == Policy::Study {
            let played = self.played.pop().ok_or("no turn to take back")?;
            self.history.pop();
            self.clock = played.clock;
            self.outcome = None;
            self.offer = None;
            self.record(
                actor,
                at,
                EventKind::Takeback {
                    turn: played.turn.to_string(),
                },
            );
            return Ok(());
        }
        if self.outcome.is_some() {
            return Err("game has finished".into());
        }
        let position = self.position();
        let side = position.side();
        match command {
            Command::Move { turn } => {
                if side != actor || self.clock.is_some_and(|clock| clock.paused) {
                    return Err("no active turn for this player".into());
                }
                let turn = resolve_turn(position, &turn)?;
                let mut next = position;
                next.make(turn);
                self.history.push_generated(turn, next);
                self.played.push(Played {
                    turn,
                    clock: self.clock,
                });
                if self.offer != Some(actor) {
                    self.offer = None;
                }
                if let Some(clock) = &mut self.clock {
                    clock.commit(side, now);
                }
                self.record(
                    actor,
                    at,
                    EventKind::Move {
                        turn: turn.to_string(),
                    },
                );
                let result = board_outcome(next, &moves::legal(next)).or_else(|| {
                    if self.policy == Policy::House {
                        self.history.house_draw()
                    } else {
                        None
                    }
                });
                if let Some(outcome) = result {
                    self.finish(outcome, actor, at, None);
                }
            }
            Command::ClaimRepetition { turn } => {
                if self.policy != Policy::Tournament
                    || actor != side
                    || self.clock.is_some_and(|clock| clock.paused)
                {
                    return Err("repetition claim unavailable".into());
                }
                let turn = resolve_turn(position, &turn)?;
                if !self.history.prospective_repetition(turn)? {
                    return Err("the intended turn would not create a third occurrence".into());
                }
                self.finish(
                    Outcome::draw(Reason::Repetition),
                    actor,
                    at,
                    Some(turn.to_string()),
                );
            }
            Command::ClaimNoProgress => {
                if self.policy != Policy::Tournament
                    || self.history.quiet_plies() < 80
                    || self.clock.is_some_and(|clock| clock.paused)
                {
                    return Err("no-progress claim unavailable".into());
                }
                self.finish(Outcome::draw(Reason::NoProgress), actor, at, None);
            }
            Command::OfferDraw if self.offer.is_none() => {
                self.offer = Some(actor);
                self.record(actor, at, EventKind::DrawOffered);
            }
            Command::AcceptDraw if self.offer == Some(actor.other()) => {
                self.finish(Outcome::draw(Reason::Agreement), actor, at, None);
            }
            Command::Resign => self.finish(
                Outcome::win(actor.other(), Reason::Resignation),
                actor,
                at,
                None,
            ),
            Command::Pause if self.local => {
                let clock = self
                    .clock
                    .as_mut()
                    .filter(|clock| !clock.paused)
                    .ok_or("no running clock")?;
                clock.balance[side as usize] = clock.time(side, now);
                clock.paused = true;
                self.record(actor, at, EventKind::Paused);
            }
            Command::Resume if self.local => {
                let clock = self
                    .clock
                    .as_mut()
                    .filter(|clock| clock.paused)
                    .ok_or("no paused clock")?;
                clock.anchor = now;
                clock.paused = false;
                self.record(actor, at, EventKind::Resumed);
            }
            _ => return Err("command unavailable in this session".into()),
        }
        Ok(())
    }
}
