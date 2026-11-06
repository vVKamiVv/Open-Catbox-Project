//! What the original's ADV context keeps for saving: the save-point counter, the route stream, the pending SSETs
//! and the slot info of the last save point (`shin::ADV::Context` `+0x2530..+0x2dd8`, `+0x90e8`, `+0x9158`).
//!
//! A save does not hold the scene. It holds how to get there: the scenario, the VM's first random seed, the
//! counter value at the save point and the route (every SGET, SELECT and QUIZ result). Loading replays the script
//! to that counter value (`Adv::new`). See `research/docs/engine-internals.md`, "Saving and loading".

use chrono::Local;
use shin_core::format::save::{
    GameData, GameDataEntry, PersistData, RouteData, SaveData, route::RouteCursor,
};

pub struct SaveState {
    /// The slot info as of the last save point; its route is the live one
    pub entry: GameDataEntry,
    /// The counter value a replay runs to (`+0x2548`); 0 for a new game
    pub target: u32,
    /// SSETs before this counter value already happened when the save was made, so replay skips them (`+0x254c`)
    pub sset_threshold: u32,
    pub cursor: RouteCursor,
    /// Counts save points as their commands start (`+0x2db0`)
    pub counter: u32,
    /// The counter at the last MSGSET, SELECT, QUIZ or AUTOSAVE save point (`+0x2dc8`)
    pub last_save_point: u32,
    /// The EVBEGIN event being played, if any (`+0x2ddc`, -1 for none): its lines are not save points
    pub event: Option<i32>,
    /// An `EXIT 0` was reached (`+0x2dd8`)
    pub exit_seen: bool,
    /// A line has been shown since the last MSGCLOSE that closed the box (`+0x90f0`): a choice asked under it is not
    /// a save point
    pub message_current: bool,
    /// Whether the next MSGCLOSE really clears the line (`+0x90e8`)
    pub message_pending: bool,
    /// The current line has been read before (`+0x2dd9`): skipping may pass it when only read lines are skipped
    pub line_read: bool,
    /// SYSCALL 0 clears it for the end of a piece (`+0x913c`): no skip, auto or menus then
    pub system_enabled: bool,
    /// SSETs not written to the persistent data yet, applied at the next save point (`+0x9158`)
    pending_sset: Vec<(i32, i32)>,
    /// The save points of the line before the current one and of the current one (`+0x2db4`, `+0x2db8`): rollback
    /// goes back to the first
    pub line_points: (u32, u32),
    /// Lines played since the start; rollback needs two
    pub lines: u32,
    /// The save points of the last two AUTOSAVEs, the chapter starts (`+0x2dbc`, `+0x2dc0`), and the lines played
    /// since each (`+0x2dcc`, `+0x2dd0`): "Return to the last chapter" picks one
    pub chapter_points: [u32; 2],
    pub chapter_lines: [u32; 2],
}

impl SaveState {
    /// The state of a fresh context for `entry`: a new game when its save position is 0, else a replay to it
    pub fn new(entry: GameDataEntry) -> Self {
        Self {
            target: entry.save_position,
            sset_threshold: entry.save_position,
            entry,
            cursor: RouteCursor::default(),
            counter: 0,
            last_save_point: 0,
            event: None,
            exit_seen: false,
            message_current: false,
            message_pending: false,
            line_read: false,
            // shortcut: the Context's initial value was not decoded; the game plays as with it set
            system_enabled: true,
            pending_sset: Vec::new(),
            line_points: (0, 0),
            lines: 0,
            chapter_points: [0; 2],
            chapter_lines: [0; 2],
        }
    }

    /// A line outside an event became a save point
    pub fn line(&mut self) {
        self.line_points = (self.line_points.1, self.counter);
        self.lines += 1;
        for lines in &mut self.chapter_lines {
            *lines += 1;
        }
    }

    /// An AUTOSAVE (a chapter start) became a save point
    pub fn chapter(&mut self) {
        self.chapter_points = [self.chapter_points[1], self.counter];
        self.chapter_lines = [self.chapter_lines[1], 0];
    }

    /// Where "Return to the last chapter?" goes (`0x71000182c0`): the current chapter's start once more than one of
    /// its lines has played, else the one before
    pub fn previous_chapter(&self) -> u32 {
        self.chapter_points[(self.chapter_lines[1] > 1) as usize]
    }

    /// The slot info of a new game of `scenario_id`
    pub fn new_game(scenario_id: i32, random_seed: u32) -> GameDataEntry {
        GameDataEntry {
            scenario_id,
            random_seed,
            save_position: 0,
            route_data: RouteData::default(),
        }
    }

    /// Whether loading still replays: the counter has not reached the save point
    pub fn is_replaying(&self) -> bool {
        self.counter < self.target
    }

    /// `0x710000f800`, run by every save-point command as it starts: the pending SSETs reach the persistent data
    pub fn checkpoint(&mut self, persist: &mut PersistData) {
        for (slot, value) in self.pending_sset.drain(..) {
            persist.set(slot as usize, value);
        }
    }

    /// A save-point command starts: checkpoint, then count it
    pub fn count(&mut self, persist: &mut PersistData) -> u32 {
        self.checkpoint(persist);
        self.counter += 1;
        self.counter
    }

    /// A MSGSET, SELECT, QUIZ or AUTOSAVE save point starts
    pub fn save_point(&mut self, persist: &mut PersistData) {
        self.last_save_point = self.count(persist);
    }

    /// `0x710000f720`: the current counter becomes the save point, and the auto slot holds it
    pub fn snapshot(&mut self, global: &mut SaveData) {
        self.entry.save_position = self.counter;
        self.sset_threshold = self.counter;
        global.auto_save_slot = Some(GameData {
            date_time: Local::now().naive_local(),
            entry: self.entry.clone(),
        });
    }

    /// SGET played live (`0x7100004d40`): a pending SSET's value, else the persistent one; recorded in the route
    pub fn sget(&mut self, persist: &PersistData, slot: i32) -> i32 {
        let value = self
            .pending_sset
            .iter()
            .find(|(pending, _)| *pending == slot)
            .map_or_else(|| persist.get(slot as usize), |&(_, value)| value);
        self.entry.route_data.write_sget(&mut self.cursor, value);
        value
    }

    /// SGET replayed (`0x7100004ce0`)
    pub fn replay_sget(&mut self) -> i32 {
        self.entry.route_data.read_sget(&mut self.cursor)
    }

    /// SSET (`0x7100004e60`); replayed SSETs from before the save point are skipped (`0x7100004e10`)
    pub fn sset(&mut self, slot: i32, value: i32) {
        if self.counter < self.sset_threshold {
            return;
        }
        match self
            .pending_sset
            .iter_mut()
            .find(|(pending, _)| *pending == slot)
        {
            Some(pending) => pending.1 = value,
            None => self.pending_sset.push((slot, value)),
        }
    }

    /// Records a SELECT pick or QUIZ answer of `bits` bits, which also counts as a save point
    pub fn record_answer(&mut self, bits: u32, value: i32) {
        self.entry
            .route_data
            .write_bits(&mut self.cursor, bits, value as u32);
        self.counter += 1;
    }

    /// A recorded SELECT pick or QUIZ answer, while replaying
    pub fn replay_answer(&mut self, bits: u32) -> i32 {
        let value = self.entry.route_data.read_bits(&mut self.cursor, bits) as i32;
        self.counter += 1;
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_reads_back_what_play_recorded() {
        let mut persist = PersistData::new();
        persist.set(27, 1);
        let mut live = SaveState::new(SaveState::new_game(26, 7));
        assert_eq!(live.sget(&persist, 27), 1);
        live.sset(27, 2);
        // a pending SSET is what SGET sees until the next save point writes it
        assert_eq!(live.sget(&persist, 27), 2);
        assert_eq!(persist.get(27), 1);
        live.save_point(&mut persist);
        assert_eq!(persist.get(27), 2);
        live.record_answer(RouteData::SELECT_BITS, 3);
        live.save_point(&mut persist);
        let mut global = SaveData::new();
        live.snapshot(&mut global);
        let entry = global.auto_save_slot.unwrap().entry;
        assert_eq!(entry.save_position, 3);

        let mut replay = SaveState::new(entry);
        assert!(replay.is_replaying());
        assert_eq!(replay.replay_sget(), 1);
        // already applied before the save
        replay.sset(27, 2);
        assert!(replay.pending_sset.is_empty());
        assert_eq!(replay.replay_sget(), 2);
        replay.save_point(&mut persist);
        assert_eq!(replay.replay_answer(RouteData::SELECT_BITS), 3);
        assert!(replay.is_replaying());
        replay.save_point(&mut persist);
        assert!(!replay.is_replaying());
    }
}
