#[derive(Debug, Eq, PartialEq)]
enum PendingWrite {
    Text(String),
    Image(String),
}

#[derive(Debug, Default)]
pub struct SelfWriteSuppression {
    pending: Option<PendingWrite>,
}

impl SelfWriteSuppression {
    pub fn arm_text(&mut self, text: &str) {
        self.pending = Some(PendingWrite::Text(text.to_owned()));
    }

    pub fn arm_image(&mut self, content_hash: &str) {
        self.pending = Some(PendingWrite::Image(content_hash.to_owned()));
    }

    /// Consumes a pending self-write without inspecting any payload, for the
    /// case where the clipboard itself already reports the content as locally
    /// owned.
    ///
    /// LionClip only ever writes to the clipboard from a restore, and every
    /// restore arms a pending write immediately before writing. So "this
    /// process owns the content" plus "a write is pending" identifies our own
    /// restore exactly, and the payload comparison the other checks perform is
    /// what it would otherwise cost a full selection read to obtain.
    pub fn take_self_write(&mut self) -> bool {
        self.pending.take().is_some()
    }

    pub fn should_suppress_text(&mut self, observed_text: &str) -> bool {
        matches!(
            self.pending.take(),
            Some(PendingWrite::Text(expected)) if expected == observed_text
        )
    }

    pub fn should_suppress_image(&mut self, observed_hash: &str) -> bool {
        matches!(
            self.pending.take(),
            Some(PendingWrite::Image(expected)) if expected == observed_hash
        )
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_text_self_write_is_suppressed_once() {
        let mut suppression = SelfWriteSuppression::default();
        suppression.arm_text("restored text");
        assert!(suppression.should_suppress_text("restored text"));
        assert!(!suppression.should_suppress_text("restored text"));
    }

    #[test]
    fn image_suppression_is_typed_and_one_shot() {
        let mut suppression = SelfWriteSuppression::default();
        suppression.arm_image("abc123");
        assert!(!suppression.should_suppress_text("abc123"));
        suppression.arm_image("abc123");
        assert!(suppression.should_suppress_image("abc123"));
        assert!(!suppression.should_suppress_image("abc123"));
    }

    #[test]
    fn different_external_write_clears_pending_suppression() {
        let mut suppression = SelfWriteSuppression::default();
        suppression.arm_text("restored text");
        assert!(!suppression.should_suppress_text("new external text"));
        assert!(!suppression.should_suppress_text("restored text"));
    }

    #[test]
    fn a_locally_owned_change_is_suppressed_once_without_a_payload() {
        let mut suppression = SelfWriteSuppression::default();
        suppression.arm_text("restored text");
        assert!(suppression.take_self_write());
        // One-shot, exactly like the payload-comparing checks: a second
        // change is somebody else's even if it is still locally owned.
        assert!(!suppression.take_self_write());
    }

    #[test]
    fn a_locally_owned_change_with_nothing_armed_is_not_suppressed() {
        let mut suppression = SelfWriteSuppression::default();
        assert!(!suppression.take_self_write());
    }

    #[test]
    fn taking_a_self_write_leaves_nothing_for_the_payload_checks() {
        let mut suppression = SelfWriteSuppression::default();
        suppression.arm_image("abc123");
        assert!(suppression.take_self_write());
        assert!(!suppression.should_suppress_image("abc123"));
    }

    #[test]
    fn unsupported_change_cancels_pending_suppression() {
        let mut suppression = SelfWriteSuppression::default();
        suppression.arm_image("abc");
        suppression.cancel();
        assert!(!suppression.should_suppress_image("abc"));
    }
}
