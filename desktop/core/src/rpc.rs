//! Line-delimited JSON-RPC between the Electron shell and the core.
//!
//! The shell writes one request per line to stdin:
//! `{"id": 7, "method": "desktop_playback_seek", "params": {"seconds": 42}}`.
//! The core writes one message per line to stdout: a response
//! (`{"id": 7, "result": …}` or `{"id": 7, "error": …}`) or an event
//! (`{"event": "desktop-playback-state", "payload": …}`). Events named
//! `shell:*` ask the shell to act on its windows, tray or lifetime; every
//! other event is forwarded to the renderers as is. stdout carries nothing
//! else, so diagnostics go to stderr.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::io::Write;
use std::sync::Mutex;

#[derive(Debug, Deserialize)]
pub(crate) struct Request {
    pub(crate) id: u64,
    pub(crate) method: String,
    #[serde(default)]
    pub(crate) params: Value,
}

pub(crate) type Reply = Result<Value, Value>;

/// The core's stdout, shared by every thread that answers or emits.
pub(crate) struct Output {
    sink: Mutex<Box<dyn Write + Send>>,
}

impl Output {
    pub(crate) fn stdout() -> Self {
        Self::new(Box::new(std::io::stdout()))
    }

    pub(crate) fn new(sink: Box<dyn Write + Send>) -> Self {
        Self {
            sink: Mutex::new(sink),
        }
    }

    pub(crate) fn emit(&self, event: &str, payload: impl Serialize) {
        match serde_json::to_value(payload) {
            Ok(payload) => self.write(&json!({ "event": event, "payload": payload })),
            Err(error) => eprintln!("Desktop core event '{event}' could not be encoded: {error}"),
        }
    }

    pub(crate) fn respond(&self, id: u64, reply: Reply) {
        let message = match reply {
            Ok(result) => json!({ "id": id, "result": result }),
            Err(error) => json!({ "id": id, "error": error }),
        };
        self.write(&message);
    }

    fn write(&self, message: &Value) {
        let Ok(mut sink) = self.sink.lock() else {
            eprintln!("Desktop core output is unavailable.");
            return;
        };
        let result = serde_json::to_writer(&mut *sink, message)
            .map_err(std::io::Error::from)
            .and_then(|_| sink.write_all(b"\n"))
            .and_then(|_| sink.flush());
        if let Err(error) = result {
            eprintln!("Desktop core output write failed: {error}");
        }
    }
}

/// Serializes a command result the way the renderer expects it: the value on
/// success, the command's own error shape on failure.
pub(crate) fn reply<T: Serialize, E: Serialize>(result: Result<T, E>) -> Reply {
    match result {
        Ok(value) => serde_json::to_value(value).map_err(encoding_error),
        Err(error) => Err(serde_json::to_value(error).unwrap_or_else(encoding_error)),
    }
}

/// Reads one named (camelCase) argument; a missing one reads as `null`, so
/// optional arguments may be left out.
pub(crate) fn param<T: DeserializeOwned>(params: &Value, name: &str) -> Result<T, Value> {
    let value = params.get(name).cloned().unwrap_or(Value::Null);
    serde_json::from_value(value)
        .map_err(|error| Value::String(format!("Invalid '{name}' argument: {error}")))
}

fn encoding_error(error: serde_json::Error) -> Value {
    Value::String(format!("Desktop core reply could not be encoded: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("capture lock")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn lines(captured: &Captured) -> Vec<Value> {
        String::from_utf8(captured.0.lock().expect("capture lock").clone())
            .expect("utf-8 output")
            .lines()
            .map(|line| serde_json::from_str(line).expect("one JSON message per line"))
            .collect()
    }

    #[test]
    fn responses_and_events_are_one_json_message_per_line() {
        let captured = Captured::default();
        let output = Output::new(Box::new(captured.clone()));

        output.respond(1, Ok(json!({ "origin": "http://localhost:8080" })));
        output.respond(2, Err(json!({ "message": "nope" })));
        output.emit("desktop-playback-state", json!({ "status": "idle" }));

        assert_eq!(
            lines(&captured),
            [
                json!({ "id": 1, "result": { "origin": "http://localhost:8080" } }),
                json!({ "id": 2, "error": { "message": "nope" } }),
                json!({ "event": "desktop-playback-state", "payload": { "status": "idle" } }),
            ]
        );
    }

    #[test]
    fn requests_may_leave_out_params() {
        let request: Request =
            serde_json::from_str(r#"{"id":3,"method":"desktop_playback_next"}"#).expect("parse");
        assert_eq!(request.id, 3);
        assert_eq!(request.method, "desktop_playback_next");
        assert_eq!(request.params, Value::Null);
    }

    #[test]
    fn params_read_camel_case_names_and_missing_optionals_as_none() {
        let params = json!({ "currentIndex": 2, "deviceId": "hw:1" });
        assert_eq!(param::<Option<usize>>(&params, "currentIndex"), Ok(Some(2)));
        assert_eq!(param::<Option<usize>>(&params, "missing"), Ok(None));
        assert_eq!(param::<String>(&params, "deviceId"), Ok("hw:1".to_owned()));
        assert!(param::<f64>(&params, "seconds").is_err());
    }

    #[test]
    fn reply_keeps_the_command_error_shape() {
        #[derive(Serialize)]
        struct CommandError {
            code: &'static str,
        }
        assert_eq!(
            reply::<(), _>(Err(CommandError {
                code: "invalid_origin"
            })),
            Err(json!({ "code": "invalid_origin" }))
        );
        assert_eq!(reply::<_, ()>(Ok(None::<u8>)), Ok(Value::Null));
    }
}
