//! Command History for Undo/Redo
//!
//! Manages the undo/redo stacks with configurable depth.

use super::command::Command;
use serde::{Serialize, Deserialize};

/// Manages command history for undo/redo operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandHistory {
    /// Stack of executed commands (for undo)
    undo_stack: Vec<Command>,
    /// Stack of undone commands (for redo)
    redo_stack: Vec<Command>,
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

    /// Push a new command to the history
    ///
    /// This clears the redo stack since we've taken a new action.
    pub fn push(&mut self, command: Command) {
        // Clear redo stack - can't redo after new action
        self.redo_stack.clear();

        // Add to undo stack
        self.undo_stack.push(command);

        // Trim if over max size
        while self.undo_stack.len() > self.max_size {
            self.undo_stack.remove(0);
        }
    }

    /// Pop the last command for undo
    ///
    /// Returns the command that should be undone, and moves it to redo stack.
    pub fn undo(&mut self) -> Option<Command> {
        if let Some(command) = self.undo_stack.pop() {
            self.redo_stack.push(command.clone());
            Some(command)
        } else {
            None
        }
    }

    /// Pop from redo stack
    ///
    /// Returns the command that should be redone, and moves it back to undo stack.
    pub fn redo(&mut self) -> Option<Command> {
        if let Some(command) = self.redo_stack.pop() {
            self.undo_stack.push(command.clone());
            Some(command)
        } else {
            None
        }
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
        self.undo_stack.last().map(|cmd| cmd.description())
    }

    /// Get description of the next redo action
    pub fn redo_description(&self) -> Option<&str> {
        self.redo_stack.last().map(|cmd| cmd.description())
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

    #[test]
    fn test_push_and_undo() {
        let mut history = CommandHistory::new(10);

        history.push(Command::SetVisibility {
            object_id: 1,
            visible: false,
            previous: true,
        });

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

        history.push(Command::SetVisibility {
            object_id: 1,
            visible: false,
            previous: true,
        });

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

        history.push(Command::SetVisibility {
            object_id: 1,
            visible: false,
            previous: true,
        });

        history.undo();
        assert!(history.can_redo());

        // New action should clear redo
        history.push(Command::SetVisibility {
            object_id: 2,
            visible: false,
            previous: true,
        });

        assert!(!history.can_redo());
    }

    #[test]
    fn test_max_size() {
        let mut history = CommandHistory::new(3);

        for i in 0..5 {
            history.push(Command::SetVisibility {
                object_id: i,
                visible: false,
                previous: true,
            });
        }

        assert_eq!(history.size(), 3);
    }
}
