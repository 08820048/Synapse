#[derive(Debug)]
pub struct CursorBlinkState {
    visible: bool,
    generation: u64,
}

impl Default for CursorBlinkState {
    fn default() -> Self {
        Self {
            visible: true,
            generation: 0,
        }
    }
}

impl CursorBlinkState {
    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn restart(&mut self) -> u64 {
        self.visible = true;
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }

    pub fn toggle(&mut self, generation: u64) -> bool {
        if self.generation != generation {
            return false;
        }
        self.visible = !self.visible;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::CursorBlinkState;

    #[test]
    fn p2_cursor_blink_toggles_for_the_active_generation() {
        let mut blink = CursorBlinkState::default();
        let generation = blink.restart();

        assert!(blink.visible());
        assert!(blink.toggle(generation));
        assert!(!blink.visible());
        assert!(blink.toggle(generation));
        assert!(blink.visible());
    }

    #[test]
    fn p2_cursor_blink_restart_is_visible_and_invalidates_old_timer() {
        let mut blink = CursorBlinkState::default();
        let old_generation = blink.restart();
        assert!(blink.toggle(old_generation));
        assert!(!blink.visible());

        let current_generation = blink.restart();

        assert!(blink.visible());
        assert!(!blink.toggle(old_generation));
        assert!(blink.toggle(current_generation));
        assert!(!blink.visible());
    }
}
