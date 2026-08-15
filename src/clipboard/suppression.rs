#[derive(Debug, Default)]
pub struct SelfWriteSuppression {
    pending_text: Option<String>,
}

impl SelfWriteSuppression {
    pub fn arm(&mut self, text: &str) {
        self.pending_text = Some(text.to_owned());
    }

    pub fn should_suppress(&mut self, observed_text: &str) -> bool {
        self.pending_text
            .take()
            .is_some_and(|pending| pending == observed_text)
    }

    pub fn cancel(&mut self) {
        self.pending_text = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_self_write_is_suppressed_once() {
        let mut suppression = SelfWriteSuppression::default();
        suppression.arm("restored text");

        assert!(suppression.should_suppress("restored text"));
        assert!(!suppression.should_suppress("restored text"));
    }

    #[test]
    fn different_external_write_is_not_suppressed() {
        let mut suppression = SelfWriteSuppression::default();
        suppression.arm("restored text");

        assert!(!suppression.should_suppress("new external text"));
        assert!(!suppression.should_suppress("restored text"));
    }

    #[test]
    fn unsupported_change_cancels_pending_suppression() {
        let mut suppression = SelfWriteSuppression::default();
        suppression.arm("restored text");
        suppression.cancel();

        assert!(!suppression.should_suppress("restored text"));
    }
}
