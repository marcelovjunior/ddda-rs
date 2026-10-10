//! The ordinary pawn order mailbox and its native actor-frame clocks.
//!
//! This is the scalar part of `cAICheckSituationCmc` at +0x52C..+0x548.
//! Selection of recipients and input bindings belongs to the host.

/// Values accepted by `uPlayerBase::vf98` (0x00B79FC0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    None = 8,
    Go = 9,
    Come = 10,
    Help = 11,
}

impl Command {
    /// Decode rising player-pad bits as the native sender does. Go and
    /// Come cannot be combined with another command. Both Help directions
    /// together are accepted. Unrelated buttons do not affect selection.
    // Translated from 0x00B768E0 (DDDA.exe 2017-12-13 Steam).
    pub fn from_pressed_bits(bits: u32) -> Option<Self> {
        match bits & 0x0078_0000 {
            0x0008_0000 => Some(Self::Go),
            0x0010_0000 => Some(Self::Come),
            0x0020_0000 | 0x0040_0000 | 0x0060_0000 => Some(Self::Help),
            _ => None,
        }
    }

    pub fn from_message(message: u32) -> Option<Self> {
        match message {
            8 => Some(Self::None),
            9 => Some(Self::Go),
            10 => Some(Self::Come),
            11 => Some(Self::Help),
            _ => None,
        }
    }

    /// cPlActCMCOrder's parameter +4, passed by 0x00B768E0.
    pub fn shout_index(self) -> Option<u8> {
        match self {
            Self::None => None,
            Self::Go => Some(0),
            Self::Come => Some(1),
            Self::Help => Some(2),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Clock {
    pub active: Command,
    pub pending: Command,
    pub mailbox: Command,
    pub duration: f32,
    pub delay: f32,
    pub active_age: f32,
    pub pending_age: f32,
    pub feedback_remaining: f32,
    pub feedback_active: bool,
    pending_ready: bool,
}

impl Default for Clock {
    // Translated from 0x00581C20 (DDDA.exe 2017-12-13 Steam).
    fn default() -> Self {
        Self {
            active: Command::None,
            pending: Command::None,
            mailbox: Command::None,
            duration: 600.0,
            delay: 0.0,
            active_age: 0.0,
            pending_age: 0.0,
            feedback_remaining: 0.0,
            feedback_active: false,
            pending_ready: false,
        }
    }
}

impl Clock {
    /// vf98 overwrites the mailbox: the last delivered message wins.
    // Translated from 0x00B79FC0 (DDDA.exe 2017-12-13 Steam).
    pub fn deliver(&mut self, command: Command) {
        if command != Command::None {
            self.mailbox = command;
        }
    }

    /// Return whether the priority list must be rebuilt. An ineligible
    /// update leaves the mailbox and all clocks untouched, as the native
    /// owner/AI/resource guards do. `actor_frames` is uCmc +0x14.
    // Translated from 0x005927F0 (DDDA.exe 2017-12-13 Steam).
    pub fn step(&mut self, actor_frames: f32, eligible: bool) -> bool {
        if !eligible {
            return false;
        }
        let message = self.mailbox;
        self.mailbox = Command::None;
        if message != Command::None && message != self.pending {
            self.pending = message;
            self.pending_ready = true;
            self.pending_age = 0.0;
        }
        let mut rebuild = false;
        if self.pending_ready {
            self.pending_age += actor_frames;
            if self.pending_age < self.delay {
                // The original returns here, before expiry and feedback.
                return false;
            }
            rebuild = self.pending != self.active;
            self.feedback_active = true;
            self.feedback_remaining = 6.0;
            self.active = self.pending;
            self.active_age = 0.0;
            self.pending_age = 0.0;
            self.pending_ready = false;
            self.pending = Command::None;
        }
        if self.active != Command::None {
            self.active_age += actor_frames;
            if self.duration <= self.active_age {
                self.active = Command::None;
                self.active_age = 0.0;
                rebuild = true;
            }
        }
        if self.feedback_remaining > 0.0 {
            self.feedback_remaining -= actor_frames;
            if self.feedback_remaining < 0.0 {
                self.feedback_active = false;
                self.feedback_remaining = 0.0;
            }
        }
        rebuild
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_four_directions_select_only_three_native_messages() {
        for mask in 0..16 {
            let expected = match mask {
                1 => Some(Command::Go),
                2 => Some(Command::Come),
                4 | 8 | 12 => Some(Command::Help),
                _ => None,
            };
            assert_eq!(Command::from_pressed_bits((mask << 19) | 1), expected);
        }
        assert_eq!(Command::Go.shout_index(), Some(0));
        assert_eq!(Command::Come.shout_index(), Some(1));
        assert_eq!(Command::Help.shout_index(), Some(2));
    }

    #[test]
    fn actor_clock_defaults_apply_and_expire_at_exact_duration() {
        let mut clock = Clock::default();
        clock.deliver(Command::Go);
        assert!(!clock.step(100.0, false));
        assert_eq!(clock.mailbox, Command::Go);
        assert!(clock.step(0.5, true));
        assert_eq!(clock.active_age, 0.5);
        assert!(!clock.step(599.0, true));
        assert!(clock.step(0.5, true));
        assert_eq!(clock.active, Command::None);
    }

    #[test]
    fn pending_delay_pauses_expiry_and_duplicate_does_not_restart_delay() {
        let mut clock = Clock { delay: 3.0, duration: 4.0, ..Clock::default() };
        clock.deliver(Command::Come);
        assert!(!clock.step(2.0, true));
        clock.deliver(Command::Come);
        assert!(clock.step(1.0, true));
        assert_eq!(clock.active_age, 1.0);
        clock.deliver(Command::Help);
        assert!(!clock.step(2.0, true));
        assert_eq!(clock.active_age, 1.0);
        assert!(clock.step(1.0, true));
        assert_eq!(clock.active, Command::Help);
        assert_eq!(clock.active_age, 1.0);
    }

    #[test]
    fn refreshing_active_command_resets_age_without_rebuild() {
        let mut clock = Clock::default();
        clock.deliver(Command::Go);
        assert!(clock.step(1.0, true));
        assert!(!clock.step(300.0, true));
        clock.deliver(Command::Go);
        assert!(!clock.step(1.0, true));
        assert_eq!(clock.active_age, 1.0);
        // vf98 overwrites the mailbox, including two messages in one tick.
        clock.deliver(Command::Come);
        clock.deliver(Command::Help);
        assert!(clock.step(1.0, true));
        assert_eq!(clock.active, Command::Help);
    }

    #[test]
    fn feedback_zero_keeps_latch_and_positive_overshoot_clears_it() {
        let mut clock = Clock::default();
        clock.deliver(Command::Come);
        assert!(clock.step(6.0, true));
        assert_eq!(clock.feedback_remaining, 0.0);
        assert!(clock.feedback_active);
        assert!(!clock.step(1.0, true));
        assert!(clock.feedback_active);
        clock.deliver(Command::Come);
        assert!(!clock.step(6.5, true));
        assert!(!clock.feedback_active);
        assert_eq!(Command::from_message(12), None);
    }
}
