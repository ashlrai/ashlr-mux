//! Port of `Policy/CommandPaletteOverlayPromotionPolicy.swift`.

/// Decides when the palette overlay container should be re-promoted above
/// sibling overlay views: exactly on the hidden-to-visible transition, so an
/// already-visible palette is not reshuffled on every state update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandPaletteOverlayPromotionPolicy {
    /// Whether the overlay was visible before this update.
    pub previously_visible: bool,
    /// Whether the overlay is visible after this update.
    pub is_visible: bool,
}

impl CommandPaletteOverlayPromotionPolicy {
    /// Captures the visibility transition to evaluate.
    pub fn new(previously_visible: bool, is_visible: bool) -> Self {
        Self {
            previously_visible,
            is_visible,
        }
    }

    /// Whether the overlay should be promoted above its siblings.
    pub fn should_promote(self) -> bool {
        self.is_visible && !self.previously_visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Swift: `promotesOnlyOnHiddenToVisibleTransition`.
    #[test]
    fn promotes_only_on_hidden_to_visible_transition() {
        assert!(CommandPaletteOverlayPromotionPolicy::new(false, true).should_promote());
        assert!(!CommandPaletteOverlayPromotionPolicy::new(true, true).should_promote());
        assert!(!CommandPaletteOverlayPromotionPolicy::new(false, false).should_promote());
        assert!(!CommandPaletteOverlayPromotionPolicy::new(true, false).should_promote());
    }
}
