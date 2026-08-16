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

    pub fn should_suppress_text(&mut self, observed_text: &str) -> bool {
        self.pending
            .take()
            .is_some_and(|pending| pending == PendingWrite::Text(observed_text.to_owned()))
    }

    pub fn should_suppress_image(&mut self, observed_hash: &str) -> bool {
        self.pending
            .take()
            .is_some_and(|pending| pending == PendingWrite::Image(observed_hash.to_owned()))
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
    fn unsupported_change_cancels_pending_suppression() {
        let mut suppression = SelfWriteSuppression::default();
        suppression.arm_image("abc");
        suppression.cancel();
        assert!(!suppression.should_suppress_image("abc"));
    }
}
