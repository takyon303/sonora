//! Last.fm tags — the only provider that needs a key.
//!
//! The key belongs to the listener, so the client is inert without one and the
//! rest of the metadata pipeline keeps working.

use serde_json::Value;

use crate::http::Http;
use crate::lyrics::TrackQuery;
use crate::{encode, MetadataError, Result};

/// Every Last.fm call — read or write — goes through this one endpoint.
const API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";

#[derive(Clone)]
pub struct LastFm {
    api_key: String,
    shared_secret: String,
    session_key: Option<String>,
}

impl LastFm {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            shared_secret: String::new(),
            session_key: None,
        }
    }

    /// The credentials Last.fm writes need: the key pair from the developer
    /// account. Tags only need the key and keep working without these.
    pub fn with_credentials(api_key: impl Into<String>, shared_secret: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            shared_secret: shared_secret.into(),
            session_key: None,
        }
    }

    /// Attach the session key that proves the listener approved this app.
    pub fn with_session(mut self, session_key: impl Into<String>) -> Self {
        let session_key = session_key.into();
        let session_key = session_key.trim();
        self.session_key = (!session_key.is_empty()).then(|| session_key.to_string());
        self
    }

    pub fn has_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    pub fn has_session(&self) -> bool {
        self.session_key.is_some()
    }

    /// The first half of Last.fm's desktop sign-in: ask for a token, send the
    /// listener to [`auth_url`](Self::auth_url) to approve it, then trade the
    /// approved token for a session key with
    /// [`auth_session`](Self::auth_session).
    pub fn auth_token(&self, http: &dyn Http) -> Result<String> {
        let value = self.write(http, "auth.gettoken", Vec::new())?;
        value
            .get("token")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| MetadataError::Parse("Last.fm did not return a token".to_string()))
    }

    /// Where the listener approves the token in a browser.
    pub fn auth_url(&self, token: &str) -> String {
        format!(
            "https://www.last.fm/api/auth/?api_key={}&token={}",
            encode(self.api_key.trim()),
            encode(token)
        )
    }

    /// Trade an approved token for the session key that scrobbles are sent with.
    pub fn auth_session(&self, http: &dyn Http, token: &str) -> Result<String> {
        let value = self.write(http, "auth.getsession", vec![("token", token.to_string())])?;
        value
            .get("session")
            .and_then(|session| session.get("key"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| MetadataError::Parse("Last.fm did not return a session key".to_string()))
    }

    /// Announce what is playing now; nothing is remembered by Last.fm.
    pub fn now_playing(&self, http: &dyn Http, track: &TrackQuery) -> Result<()> {
        self.write(http, "track.updatenowplaying", track_fields(track))?;
        Ok(())
    }

    /// Report a play that was listened to. `played_at` is when it began.
    pub fn scrobble(&self, http: &dyn Http, track: &TrackQuery, played_at: i64) -> Result<()> {
        let mut fields = track_fields(track);
        fields.push(("timestamp", played_at.to_string()));
        self.write(http, "track.scrobble", fields)?;
        Ok(())
    }

    /// Sign and send one write call, returning its JSON body.
    fn write(
        &self,
        http: &dyn Http,
        method: &str,
        mut fields: Vec<(&str, String)>,
    ) -> Result<Value> {
        if !self.has_key() {
            return Err(MetadataError::MissingApiKey);
        }
        let mut params: Vec<(&str, String)> = vec![
            ("method", method.to_string()),
            ("api_key", self.api_key.trim().to_string()),
        ];
        if let Some(session) = &self.session_key {
            params.push(("sk", session.clone()));
        }
        params.append(&mut fields);

        // Last.fm's signature covers every parameter except `format` and the
        // signature itself, sorted by name and hashed with the shared secret.
        let signature = self.sign(&params)?;
        params.push(("api_sig", signature));
        params.push(("format", "json".to_string()));
        let form = params
            .iter()
            .map(|(key, value)| format!("{key}={}", encode(value)))
            .collect::<Vec<_>>()
            .join("&");

        let body = http.post(API_ROOT, &form)?;
        if body.is_empty() {
            return Ok(Value::Null);
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|err| MetadataError::Parse(format!("Last.fm response: {err}")))?;
        // Last.fm answers a refused write with HTTP 200 and an `error` member.
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            return Err(MetadataError::Parse(format!("Last.fm: {message}")));
        }
        Ok(value)
    }

    fn sign(&self, params: &[(&str, String)]) -> Result<String> {
        let secret = self.shared_secret.trim();
        if secret.is_empty() {
            return Err(MetadataError::MissingSecret);
        }
        let mut params: Vec<(&str, &str)> = params
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        params.sort_by(|a, b| a.0.cmp(b.0));
        let mut joined = String::new();
        for (key, value) in params {
            joined.push_str(key);
            joined.push_str(value);
        }
        joined.push_str(secret);
        Ok(format!("{:x}", md5::compute(joined.as_bytes())))
    }

    pub fn top_tags_url(&self, artist: &str, track: &str) -> String {
        format!(
            "https://ws.audioscrobbler.com/2.0/?method=track.gettoptags&artist={}&track={}&api_key={}&format=json",
            encode(artist),
            encode(track),
            encode(self.api_key.trim())
        )
    }

    /// Listener-assigned tags for a track, strongest first.
    pub fn track_tags(&self, http: &dyn Http, artist: &str, track: &str) -> Result<Vec<String>> {
        if !self.has_key() {
            return Err(MetadataError::MissingApiKey);
        }
        let body = http.get(&self.top_tags_url(artist, track))?;
        parse_tags(&body)
    }
}

/// The fields every scrobble call carries, from the track as the library knows
/// it. Last.fm takes the duration in whole seconds and ignores a blank album.
fn track_fields(track: &TrackQuery) -> Vec<(&'static str, String)> {
    let mut fields = vec![
        ("artist", track.artist.clone()),
        ("track", track.title.clone()),
    ];
    if let Some(album) = track
        .album
        .as_ref()
        .filter(|album| !album.trim().is_empty())
    {
        fields.push(("album", album.clone()));
    }
    if let Some(duration) = track.duration_ms.filter(|ms| *ms > 0) {
        fields.push(("duration", (duration / 1_000).to_string()));
    }
    fields
}

fn parse_tags(body: &[u8]) -> Result<Vec<String>> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| MetadataError::Parse(format!("Last.fm response: {err}")))?;

    // Last.fm returns `{"toptags":{"tag":[]}}` for a track nobody tagged, and
    // sometimes an object instead of an array for a single tag.
    let Some(tags) = value
        .get("toptags")
        .and_then(|top| top.get("tag"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };

    Ok(tags
        .iter()
        .filter_map(|tag| tag.get("name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex as StdMutex;

    struct Stub(HashMap<String, Vec<u8>>);

    impl Http for Stub {
        fn get(&self, url: &str) -> std::result::Result<Vec<u8>, crate::HttpError> {
            self.0
                .get(url)
                .cloned()
                .ok_or(crate::HttpError::Status(404))
        }
    }

    #[test]
    fn builds_a_keyed_request() {
        let client = LastFm::new("abc123");
        let url = client.top_tags_url("Aphex Twin", "Xtal");
        assert!(url.contains("method=track.gettoptags"));
        assert!(url.contains("artist=Aphex%20Twin"));
        assert!(url.contains("api_key=abc123"));
        assert!(url.contains("format=json"));
        assert!(client.has_key());
        assert!(!LastFm::new("  ").has_key());
    }

    #[test]
    fn reads_tags_in_order() {
        let body = r#"{"toptags":{"tag":[{"name":"electronic","count":100},{"name":"ambient","count":70}]}}"#;
        assert_eq!(
            parse_tags(body.as_bytes()).unwrap(),
            vec!["electronic".to_string(), "ambient".to_string()]
        );
    }

    #[test]
    fn an_untagged_track_yields_no_tags() {
        let body = r#"{"toptags":{"tag":[]}}"#;
        assert!(parse_tags(body.as_bytes()).unwrap().is_empty());
        assert!(parse_tags(b"{}").unwrap().is_empty());
    }

    #[test]
    fn without_a_key_the_client_refuses_instead_of_calling_out() {
        let client = LastFm::new("");
        let error = client
            .track_tags(&Stub(HashMap::new()), "Aphex Twin", "Xtal")
            .unwrap_err();
        assert!(matches!(error, MetadataError::MissingApiKey));
    }

    #[test]
    fn a_keyed_lookup_returns_the_parsed_tags() {
        let client = LastFm::new("abc123");
        let url = client.top_tags_url("Aphex Twin", "Xtal");
        let stub = Stub(HashMap::from([(
            url,
            br#"{"toptags":{"tag":[{"name":"idm"}]}}"#.to_vec(),
        )]));
        assert_eq!(
            client.track_tags(&stub, "Aphex Twin", "Xtal").unwrap(),
            vec!["idm".to_string()]
        );
    }
    /// Records write requests and answers them from a queue of canned bodies.
    struct PostStub {
        sent: StdMutex<Vec<(String, String)>>,
        replies: StdMutex<VecDeque<Vec<u8>>>,
    }

    impl PostStub {
        fn replying(replies: &[&str]) -> Self {
            Self {
                sent: StdMutex::new(Vec::new()),
                replies: StdMutex::new(
                    replies
                        .iter()
                        .map(|body| body.as_bytes().to_vec())
                        .collect(),
                ),
            }
        }

        fn last_form(&self) -> String {
            self.sent.lock().unwrap().last().unwrap().1.clone()
        }
    }

    impl Http for PostStub {
        fn get(&self, _url: &str) -> std::result::Result<Vec<u8>, crate::HttpError> {
            Err(crate::HttpError::Status(404))
        }

        fn post(&self, url: &str, form: &str) -> std::result::Result<Vec<u8>, crate::HttpError> {
            self.sent
                .lock()
                .unwrap()
                .push((url.to_string(), form.to_string()));
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(crate::HttpError::Status(500))
        }
    }

    fn credentials() -> LastFm {
        LastFm::with_credentials("key123", "secret456").with_session("sess789")
    }

    fn play() -> TrackQuery {
        TrackQuery {
            artist: "Aphex Twin".to_string(),
            title: "Xtal".to_string(),
            album: Some("SAW 85-92".to_string()),
            duration_ms: Some(293_000),
        }
    }

    #[test]
    fn a_scrobble_is_signed_over_its_sorted_parameters() {
        let stub = PostStub::replying(&[r#"{"scrobbles":{"scrobble":{}}}"#]);
        credentials()
            .scrobble(&stub, &play(), 1_700_000_000)
            .unwrap();

        let (url, form) = stub.sent.lock().unwrap().last().unwrap().clone();
        assert_eq!(url, "https://ws.audioscrobbler.com/2.0/");
        assert_eq!(
            form,
            concat!(
                "method=track.scrobble&api_key=key123&sk=sess789",
                "&artist=Aphex%20Twin&track=Xtal&album=SAW%2085-92&duration=293",
                "&timestamp=1700000000",
                "&api_sig=ae033a66d0b94f9254f3f760d30d18c6&format=json"
            )
        );
    }

    #[test]
    fn now_playing_is_its_own_call() {
        let stub = PostStub::replying(&["{}"]);
        credentials().now_playing(&stub, &play()).unwrap();
        let form = stub.last_form();
        assert!(form.contains("method=track.updatenowplaying"), "{form}");
        assert!(!form.contains("timestamp"), "{form}");
        assert!(
            form.ends_with("&api_sig=4c9aa5080956fc4f158c1324c47928f9&format=json"),
            "{form}"
        );
    }

    #[test]
    fn writes_need_the_shared_secret() {
        let stub = PostStub::replying(&["{}"]);
        let error = LastFm::new("key123")
            .scrobble(&stub, &play(), 1_700_000_000)
            .unwrap_err();
        assert!(matches!(error, MetadataError::MissingSecret), "{error}");
        assert!(stub.sent.lock().unwrap().is_empty(), "nothing was sent");
    }

    #[test]
    fn a_session_key_is_only_sent_when_there_is_one() {
        let stub = PostStub::replying(&["{}"]);
        LastFm::with_credentials("key123", "secret456")
            .scrobble(&stub, &play(), 1_700_000_000)
            .unwrap();
        assert!(!stub.last_form().contains("sk="), "{}", stub.last_form());
    }

    #[test]
    fn a_refused_write_reports_what_lastfm_said() {
        let stub = PostStub::replying(&[r#"{"error":9,"message":"Invalid session key"}"#]);
        let error = credentials()
            .scrobble(&stub, &play(), 1_700_000_000)
            .unwrap_err();
        assert!(error.to_string().contains("Invalid session key"), "{error}");
    }

    #[test]
    fn sign_in_trades_an_approved_token_for_a_session_key() {
        let stub = PostStub::replying(&[
            r#"{"token":"tok-1"}"#,
            r#"{"session":{"name":"listener","key":"sess-1"}}"#,
        ]);
        let client = LastFm::with_credentials("key123", "secret456");

        let token = client.auth_token(&stub).unwrap();
        assert_eq!(token, "tok-1");
        assert_eq!(
            client.auth_url(&token),
            "https://www.last.fm/api/auth/?api_key=key123&token=tok-1"
        );
        assert_eq!(client.auth_session(&stub, &token).unwrap(), "sess-1");

        let sent = stub.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 2);
        assert!(sent[0].1.contains("method=auth.gettoken"));
        assert!(sent[0]
            .1
            .contains("api_sig=ae128d93175d69795e18dda7a7b9f34b"));
        assert!(sent[1]
            .1
            .contains("method=auth.getsession&api_key=key123&token=tok-1"));
    }
}
