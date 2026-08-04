//! Command History for Undo/Redo
//!
//! Manages the undo/redo stacks with configurable depth.
//!
//! # Why every command carries a sequence number
//!
//! A chart's edits do not all live in one stack. Primitives and indicators
//! belong to the sync GROUP (so undoing a drawing undoes it on every window
//! sharing it), while compare series belong to the WINDOW. Undo therefore has
//! two stacks to choose from, and choosing by stack — "group first, window
//! only if the group is empty" — made undo non-chronological: with any
//! primitive present, a compare edit could never be reached.
//!
//! Each push stamps the command with a process-wide monotonic sequence. Undo
//! takes the stack whose top is the NEWEST (max seq); redo takes the stack
//! whose top is the OLDEST (min seq), which exactly reverses the undo order.

use super::command::Command;
use serde::{Serialize, Deserialize, Deserializer};

/// How many steps of history reach disk, newest first.
pub const PERSIST_MAX_ENTRIES: usize = 200;

/// Byte ceiling for one history on disk.
///
/// The step count alone is not a bound: `DeleteAllPrimitives` carries a full
/// snapshot of every primitive it removed, and `CreatePrimitive` carries the
/// points and data of one. Two hundred of those can run to megabytes inside a
/// preset that is rewritten whole on every autosave.
pub const PERSIST_MAX_BYTES: usize = 1_048_576;

/// A command plus the moment it was recorded, so commands split across
/// several histories can still be ordered against each other.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stamped {
    seq: u64,
    cmd: Command,
}

/// What a stored history entry can turn out to be on the way in.
///
/// Presets outlive the command set. An entry written by an older build may be
/// a command kind that no longer exists (viewport, alerts, symbol changes —
/// all removed 2026-08-04), or a bare command from before entries carried a
/// sequence. Neither may take the whole preset down with it, so unknown
/// entries are dropped and pre-sequence ones load at 0.
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredEntry {
    Stamped(Stamped),
    Legacy(Command),
    Unknown(#[allow(dead_code)] serde_json::Value),
}

fn tolerant_stack<'de, D>(d: D) -> Result<Vec<Stamped>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Vec<StoredEntry> = Vec::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|e| match e {
            StoredEntry::Stamped(s) => Some(s),
            StoredEntry::Legacy(cmd) => Some(Stamped { seq: 0, cmd }),
            StoredEntry::Unknown(_) => None,
        })
        .collect())
}

/// Manages command history for undo/redo operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandHistory {
    /// Stack of executed commands (for undo)
    #[serde(deserialize_with = "tolerant_stack")]
    undo_stack: Vec<Stamped>,
    /// Stack of undone commands (for redo)
    #[serde(deserialize_with = "tolerant_stack")]
    redo_stack: Vec<Stamped>,
    /// Maximum number of commands to keep
    pub max_size: usize,
}

impl CommandHistory {
    /// Create a new history with specified max size
    pub fn new(max_size: usize) -> Self {
        Self {
            undo_stack: Vec::with_capacity(max_size),
            redo_stack: Vec::new(),
            max_size,
        }
    }

    /// Push a new command to the history under sequence number `seq`.
    ///
    /// This clears the redo stack since we've taken a new action.
    pub fn push(&mut self, seq: u64, command: Command) {
        // Clear redo stack - can't redo after new action
        self.redo_stack.clear();

        // Add to undo stack
        self.undo_stack.push(Stamped { seq, cmd: command });

        // Trim if over max size
        while self.undo_stack.len() > self.max_size {
            self.undo_stack.remove(0);
        }
    }

    /// Pop the last command for undo
    ///
    /// Returns the command that should be undone, and moves it to redo stack.
    pub fn undo(&mut self) -> Option<Command> {
        let entry = self.undo_stack.pop()?;
        let cmd = entry.cmd.clone();
        self.redo_stack.push(entry);
        Some(cmd)
    }

    /// Pop from redo stack
    ///
    /// Returns the command that should be redone, and moves it back to undo stack.
    pub fn redo(&mut self) -> Option<Command> {
        let entry = self.redo_stack.pop()?;
        let cmd = entry.cmd.clone();
        self.undo_stack.push(entry);
        Some(cmd)
    }

    /// Sequence number of the command undo would take next.
    ///
    /// The caller compares this across histories and undoes the LARGEST.
    pub fn next_undo_seq(&self) -> Option<u64> {
        self.undo_stack.last().map(|e| e.seq)
    }

    /// Sequence number of the command redo would take next.
    ///
    /// The caller compares this across histories and redoes the SMALLEST —
    /// undo walks seq downwards, so redo has to walk it back upwards.
    pub fn next_redo_seq(&self) -> Option<u64> {
        self.redo_stack.last().map(|e| e.seq)
    }

    /// Check if undo is available
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if redo is available
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Get description of the next undo action
    pub fn undo_description(&self) -> Option<&str> {
        self.undo_stack.last().map(|e| e.cmd.description())
    }

    /// Get description of the next redo action
    pub fn redo_description(&self) -> Option<&str> {
        self.redo_stack.last().map(|e| e.cmd.description())
    }

    /// Drop the redo future without touching the undo past.
    ///
    /// A new action invalidates redo GLOBALLY, and a chart's actions are
    /// split across two histories — so recording into one of them has to
    /// clear the other's redo stack too. Without this a stale entry survives
    /// in the stack that was not written to, and the next redo replays it:
    /// undo a drawing, then draw something new, then undo that and press
    /// redo — and the FIRST drawing comes back instead.
    pub fn clear_redo(&mut self) {
        self.redo_stack.clear();
    }

    /// Clear all history
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Get the current size of the undo stack
    pub fn size(&self) -> usize {
        self.undo_stack.len()
    }

    /// Get undo stack length
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Get redo stack length
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Highest sequence number this history holds.
    ///
    /// Read after a cold load so the app's counter resumes ABOVE what came
    /// off disk — a counter that restarted at zero would stamp new commands
    /// below the restored ones and undo would walk them in the wrong order.
    pub fn max_seq(&self) -> u64 {
        // Scan both stacks whole rather than reading the tops: a history
        // loaded from an older preset can carry pre-sequence entries at 0
        // mixed in among stamped ones, so the top is not always the largest.
        self.undo_stack
            .iter()
            .chain(self.redo_stack.iter())
            .map(|e| e.seq)
            .max()
            .unwrap_or(0)
    }

    /// Fold a primitive edit into the one already on top when they are the
    /// same continuous change.
    ///
    /// Returns true when it merged. One colour pick reaches the app through
    /// two handlers (the settings modal's and the colour picker's own), and
    /// repeated picks while the picker is open are one gesture to the user —
    /// each would otherwise be its own Ctrl+Z step.
    pub fn coalesce_primitive_modify(
        &mut self,
        prim_id: u64,
        previous_json: &str,
        new_json: &str,
    ) -> bool {
        let Some(top) = self.undo_stack.last_mut() else { return false };
        let Command::ModifyPrimitiveFull {
            prim_id: top_id,
            previous_json: top_prev,
            new_json: top_new,
            ..
        } = &mut top.cmd else {
            return false;
        };
        if *top_id != prim_id {
            return false;
        }
        if top_prev.as_str() == previous_json && top_new.as_str() == new_json {
            // The SAME edit reported twice — one colour pick reaches both the
            // settings modal's click handler and the colour picker's own, and
            // each snapshots from the same starting state. Keep one.
            return true;
        }
        if top_new.as_str() != previous_json {
            return false;
        }
        // A continuation of the edit already on top: widen it instead of
        // stacking a second step for the same gesture.
        *top_new = new_json.to_string();
        self.redo_stack.clear();
        true
    }

    /// Every (recorded id, type, geometry) this history claims about a
    /// primitive — the evidence a cold load matches against live drawings.
    pub fn primitive_identities(&self) -> Vec<(u64, String, Vec<(i64, f64)>)> {
        self.undo_stack
            .iter()
            .chain(self.redo_stack.iter())
            .filter_map(|e| {
                let id = e.cmd.prim_id()?;
                let (type_id, points) = e.cmd.primitive_identity()?;
                Some((id, type_id.to_string(), points.to_vec()))
            })
            .collect()
    }

    /// Rewrite primitive references through `remap`, then drop what still
    /// points at a primitive that is not there.
    ///
    /// A command left pointing at a dead id is worse than no command: undo
    /// matches nothing (looks like undo is broken) and redo inserts a second
    /// copy of a drawing that already exists.
    pub fn remap_and_prune_primitives(
        &mut self,
        remap: &std::collections::HashMap<u64, u64>,
        live: &std::collections::HashSet<u64>,
    ) -> usize {
        let mut dropped = 0usize;
        for stack in [&mut self.undo_stack, &mut self.redo_stack] {
            stack.retain_mut(|e| {
                let Some(old) = e.cmd.prim_id() else {
                    return true; // not a primitive command — untouched
                };
                if let Some(&new) = remap.get(&old) {
                    e.cmd.set_prim_id(new);
                    return true;
                }
                if live.contains(&old) {
                    return true;
                }
                dropped += 1;
                false
            });
        }
        dropped
    }

    /// A copy trimmed to what may reach disk: the newest entries, under both
    /// a step count and a byte budget.
    ///
    /// The ring itself stays in RAM — the file is rewritten whole on every
    /// autosave, so persistence needs no wraparound, just a slice of the
    /// newest steps. Undo is filled first; redo gets whatever budget is left,
    /// because losing the ability to re-do beats losing the ability to undo.
    pub fn bounded_for_persistence(&self) -> Self {
        let mut budget = PERSIST_MAX_BYTES;
        let undo_stack = take_newest_within(&self.undo_stack, &mut budget);
        let redo_stack = take_newest_within(&self.redo_stack, &mut budget);
        Self { undo_stack, redo_stack, max_size: self.max_size }
    }
}

/// Newest-first walk that keeps entries while they fit, returned back in
/// oldest-first stack order.
fn take_newest_within(stack: &[Stamped], budget: &mut usize) -> Vec<Stamped> {
    let mut kept: Vec<Stamped> = Vec::new();
    for entry in stack.iter().rev().take(PERSIST_MAX_ENTRIES) {
        let size = serde_json::to_vec(entry).map(|v| v.len()).unwrap_or(usize::MAX);
        if size > *budget {
            break;
        }
        *budget -= size;
        kept.push(entry.clone());
    }
    kept.reverse();
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vis(prim_id: u64) -> Command {
        Command::SetPrimitiveVisibility { prim_id, visible: false, previous: true }
    }

    #[test]
    fn test_push_and_undo() {
        let mut history = CommandHistory::new(10);

        history.push(1, vis(1));

        assert!(history.can_undo());
        assert!(!history.can_redo());

        let cmd = history.undo();
        assert!(cmd.is_some());
        assert!(!history.can_undo());
        assert!(history.can_redo());
    }

    #[test]
    fn test_redo() {
        let mut history = CommandHistory::new(10);

        history.push(1, vis(1));

        history.undo();
        assert!(history.can_redo());

        let cmd = history.redo();
        assert!(cmd.is_some());
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn test_new_action_clears_redo() {
        let mut history = CommandHistory::new(10);

        history.push(1, vis(1));

        history.undo();
        assert!(history.can_redo());

        // New action should clear redo
        history.push(2, vis(2));

        assert!(!history.can_redo());
    }

    #[test]
    fn test_max_size() {
        let mut history = CommandHistory::new(3);

        for i in 0..5 {
            history.push(i, vis(i));
        }

        assert_eq!(history.size(), 3);
    }

    #[test]
    fn persistence_keeps_the_newest_steps_and_survives_a_round_trip() {
        let mut h = CommandHistory::new(500);
        for i in 0..(PERSIST_MAX_ENTRIES as u64 + 50) {
            h.push(i + 1, vis(i));
        }
        let bounded = h.bounded_for_persistence();
        assert_eq!(bounded.undo_count(), PERSIST_MAX_ENTRIES);
        // Newest kept, oldest dropped.
        assert_eq!(bounded.max_seq(), PERSIST_MAX_ENTRIES as u64 + 50);

        let json = serde_json::to_string(&bounded).unwrap();
        let back: CommandHistory = serde_json::from_str(&json).unwrap();
        assert_eq!(back.undo_count(), PERSIST_MAX_ENTRIES);
        assert_eq!(back.max_seq(), bounded.max_seq());
    }

    /// A preset written by an older build carries command kinds that no
    /// longer exist. Those entries must be dropped, not take the preset with
    /// them, and a pre-sequence entry must still load.
    #[test]
    fn unknown_and_pre_sequence_entries_load_instead_of_failing() {
        let json = r#"{
            "undo_stack": [
                {"ViewportChange":{"previous":{"view_start":1.0},"new":{"view_start":2.0}}},
                {"seq":7,"cmd":{"SetPrimitiveVisibility":{"prim_id":3,"visible":false,"previous":true}}},
                {"SetPrimitiveVisibility":{"prim_id":9,"visible":true,"previous":false}}
            ],
            "redo_stack": [],
            "max_size": 250
        }"#;
        let h: CommandHistory = serde_json::from_str(json).expect("must not fail on removed kinds");
        assert_eq!(h.undo_count(), 2, "the viewport entry is dropped, the other two survive");
        assert_eq!(h.max_seq(), 7);
    }

    /// The exact defect the owner hit: a stale redo entry left in the stack
    /// that the new action did NOT get pushed onto, replayed ahead of the
    /// command that was actually undone last.
    #[test]
    fn a_new_action_invalidates_the_other_history_s_redo_too() {
        let mut group = CommandHistory::new(10);
        let mut window = CommandHistory::new(10);

        group.push(1, vis(1));      // draw
        group.undo();               // undo the drawing — group redo holds it
        assert!(group.can_redo());

        // A new action anywhere must kill the whole redo future, not just the
        // redo stack it happens to land on.
        group.clear_redo();
        window.clear_redo();
        window.push(2, vis(2));     // add a compare series

        assert!(!group.can_redo(), "the drawing must no longer be redoable");

        window.undo();              // undo the compare
        // Only one candidate left, and it is the compare.
        assert_eq!(window.next_redo_seq(), Some(2));
        assert_eq!(group.next_redo_seq(), None);
    }

    /// Two histories interleaved: undo must walk the sequence downwards
    /// across BOTH, and redo must walk it back upwards — this is the whole
    /// point of stamping, and the property the old stack-priority routing
    /// violated.
    #[test]
    fn two_histories_undo_newest_first_and_redo_reverses_it() {
        let mut group = CommandHistory::new(10);
        let mut window = CommandHistory::new(10);

        group.push(1, vis(10));   // oldest
        window.push(2, vis(20));
        group.push(3, vis(30));   // newest

        // Undo picks the larger of the two tops, every time.
        let mut undone = Vec::new();
        for _ in 0..3 {
            let take_group = match (group.next_undo_seq(), window.next_undo_seq()) {
                (Some(g), Some(w)) => g > w,
                (Some(_), None) => true,
                _ => false,
            };
            let seq = if take_group { group.next_undo_seq() } else { window.next_undo_seq() };
            if take_group { group.undo(); } else { window.undo(); }
            undone.push(seq.unwrap());
        }
        assert_eq!(undone, vec![3, 2, 1]);

        // Redo picks the smaller of the two tops, replaying in reverse.
        let mut redone = Vec::new();
        for _ in 0..3 {
            let take_group = match (group.next_redo_seq(), window.next_redo_seq()) {
                (Some(g), Some(w)) => g < w,
                (Some(_), None) => true,
                _ => false,
            };
            let seq = if take_group { group.next_redo_seq() } else { window.next_redo_seq() };
            if take_group { group.redo(); } else { window.redo(); }
            redone.push(seq.unwrap());
        }
        assert_eq!(redone, vec![1, 2, 3]);
    }
}
