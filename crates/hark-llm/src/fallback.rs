//! A cloud model with the local one behind it: when the network call fails before any
//! text arrives (offline, bad key, outage), the same request runs on the local model.

use crate::backend::{ChatMessage, GenOptions, LlmBackend, LlmError};
use std::sync::{Arc, OnceLock};

type Loader = Box<dyn Fn() -> Option<Arc<dyn LlmBackend>> + Send + Sync>;

pub struct WithFallback {
    primary: Arc<dyn LlmBackend>,
    load_local: Loader,
    local: OnceLock<Option<Arc<dyn LlmBackend>>>,
}

impl WithFallback {
    /// `load_local` runs at most once, the first time the primary fails.
    pub fn new(primary: Arc<dyn LlmBackend>, load_local: Loader) -> Self {
        WithFallback { primary, load_local, local: OnceLock::new() }
    }
}

impl LlmBackend for WithFallback {
    fn chat(&self, msgs: &[ChatMessage], opts: &GenOptions, on_token: &mut dyn FnMut(&str) -> bool) -> Result<String, LlmError> {
        let mut streamed = false;
        let first = self.primary.chat(msgs, opts, &mut |t| {
            streamed = true;
            on_token(t)
        });
        match first {
            Err(LlmError::Http(e)) if !streamed => {
                let Some(local) = self.local.get_or_init(|| (self.load_local)()).clone() else {
                    return Err(LlmError::Http(e));
                };
                tracing::warn!("cloud model failed ({e}); answering with {}", local.name());
                local.chat(msgs, opts, on_token)
            }
            other => other,
        }
    }

    fn name(&self) -> String {
        self.primary.name()
    }

    fn context_tokens(&self) -> usize {
        self.primary.context_tokens()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Result<&'static str, &'static str>, usize);
    impl LlmBackend for Fixed {
        fn chat(&self, _: &[ChatMessage], _: &GenOptions, on_token: &mut dyn FnMut(&str) -> bool) -> Result<String, LlmError> {
            match self.0 {
                Ok(s) => {
                    on_token(s);
                    Ok(s.into())
                }
                Err(e) => Err(LlmError::Http(e.into())),
            }
        }
        fn name(&self) -> String {
            "fixed".into()
        }
        fn context_tokens(&self) -> usize {
            self.1
        }
    }

    #[test]
    fn falls_back_only_on_network_errors() {
        let ok = WithFallback::new(Arc::new(Fixed(Ok("cloud"), 1_000_000)), Box::new(|| Some(Arc::new(Fixed(Ok("local"), 8192)) as _)));
        assert_eq!(crate::backend::complete(&ok, &[], &GenOptions::default()).unwrap(), "cloud");
        assert_eq!(ok.context_tokens(), 1_000_000);
        let down = WithFallback::new(Arc::new(Fixed(Err("offline"), 1_000_000)), Box::new(|| Some(Arc::new(Fixed(Ok("local"), 8192)) as _)));
        assert_eq!(crate::backend::complete(&down, &[], &GenOptions::default()).unwrap(), "local");
        let none = WithFallback::new(Arc::new(Fixed(Err("offline"), 1_000_000)), Box::new(|| None));
        assert!(matches!(crate::backend::complete(&none, &[], &GenOptions::default()), Err(LlmError::Http(_))));
    }
}
