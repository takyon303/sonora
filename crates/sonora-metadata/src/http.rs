//! The single place that touches the network.
//!
//! Providers take `&dyn Http`, so tests substitute canned responses and the
//! real client stays a thin wrapper around `ureq`.

use std::time::Duration;

pub trait Http: Send + Sync {
    /// Fetch a URL and return the response body.
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError>;

    /// Submit a form-encoded write request and return the response body.
    ///
    /// Read-only transports can leave this alone; the default reports that the
    /// request could not be sent rather than pretending it worked.
    fn post(&self, _url: &str, _form: &str) -> Result<Vec<u8>, HttpError> {
        Err(HttpError::Transport(
            "this transport cannot send write requests".to_string(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The service answered, but not with success (404 is common and expected).
    Status(u16),
    Transport(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Status(code) => write!(formatter, "service answered HTTP {code}"),
            Self::Transport(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for HttpError {}

/// Blocking client with a bounded timeout.
///
/// Providers are called from worker threads, never from the UI thread, so a
/// slow service delays one lookup instead of freezing the window.
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new(user_agent: &str) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(15))
            .timeout_connect(Duration::from_secs(8))
            .user_agent(user_agent)
            .build();
        Self { agent }
    }
}

impl Http for UreqHttp {
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        read(self.agent.get(url).call())
    }

    fn post(&self, url: &str, form: &str) -> Result<Vec<u8>, HttpError> {
        read(
            self.agent
                .post(url)
                .set("Content-Type", "application/x-www-form-urlencoded")
                .send_string(form),
        )
    }
}

fn read(response: Result<ureq::Response, ureq::Error>) -> Result<Vec<u8>, HttpError> {
    match response {
        Ok(response) => {
            let mut body = Vec::new();
            std::io::Read::read_to_end(&mut response.into_reader(), &mut body)
                .map_err(|err| HttpError::Transport(err.to_string()))?;
            Ok(body)
        }
        Err(ureq::Error::Status(code, _)) => Err(HttpError::Status(code)),
        Err(ureq::Error::Transport(err)) => Err(HttpError::Transport(err.to_string())),
    }
}
