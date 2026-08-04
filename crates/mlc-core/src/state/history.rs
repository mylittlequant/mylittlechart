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
use serde::{Serialize, Deserialize};

/// A command plus the moment it was recorded, so commands split across
/// several histories can still be ordered against each other.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stamped {
    seq: u64,
    cmd: Command,
}

/// Manages command history for undo/redo operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandHistory {
    /// Stack of executed commands (for undo)
    undo_stack: Vec<Stamped>,
    /// Stack of undone commands (for redo)
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
