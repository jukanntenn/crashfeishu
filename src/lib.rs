use chrono::{Local, Timelike};
use clap::Parser;
use log::{debug, error, info, warn};
use reqwest;
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(author = "jukanntenn <jukanntenn@outlook.com>", version)]
/// This event listener will push Feishu message when processes that are children of
/// supervisord transition unexpectedly to the EXITED state.
pub struct Args {
    /// Specify a supervisor process_name.
    ///
    /// Push Feishu notification when this process transitions to the EXITED state unexpectedly.
    /// If this process is part of a group, it can be specified using the 'group_name:process_name' syntax.
    /// This option can be specified multiple times, allowing for specification of multiple processes.
    /// If not specified, all processes will be monitored.
    #[arg(short, long)]
    pub program: Vec<String>,

    /// Specify a Feishu webhook URL to push notifications to.
    #[arg(short, long)]
    pub webhook: Option<String>,

    #[arg(
        short = 'b',
        long,
        value_name = "MINUTES",
        help = "Batching interval in minutes (requires TICK_60 in events, e.g., events=PROCESS_STATE,TICK_60)"
    )]
    pub batch_interval: Option<f64>,
}

type MyResult<T> = Result<T, Box<dyn Error>>;
type TokenSet = HashMap<String, String>;

#[derive(Serialize)]
struct FeishuContent {
    text: String,
}

#[derive(Serialize)]
struct FeishuWebhookPayload {
    msg_type: String,
    content: FeishuContent,
}

#[derive(Debug)]
struct BatchingState {
    batches: HashMap<String, Vec<String>>,
    batch_mins: f64,
    tick_received: bool,
    start: Instant,
    last_warn: Option<Instant>,
}

impl BatchingState {
    fn new() -> Self {
        Self {
            batches: HashMap::new(),
            batch_mins: 0.0,
            tick_received: false,
            start: Instant::now(),
            last_warn: None,
        }
    }

    fn add_crash(&mut self, key: String, msg: String) {
        self.batches.entry(key).or_default().push(msg);
    }

    fn add_tick(&mut self, tick_mins: f64, interval: f64) -> bool {
        self.tick_received = true;
        self.batch_mins += tick_mins;
        self.batch_mins >= interval
    }

    fn reset_timer(&mut self) {
        self.batch_mins = 0.0;
    }

    fn clear(&mut self) {
        self.batches.clear();
        self.batch_mins = 0.0;
    }

    fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    fn total_message_count(&self) -> usize {
        self.batches.values().map(|v| v.len()).sum()
    }

    fn maybe_warn_missing_tick(&mut self, warn_every: Duration) -> bool {
        if self.tick_received {
            return false;
        }

        let now = Instant::now();
        let should_warn = match self.last_warn {
            None => now.duration_since(self.start) >= warn_every,
            Some(last) => now.duration_since(last) >= warn_every,
        };

        if should_warn {
            self.last_warn = Some(now);
        }
        should_warn
    }

    fn format_batch_message(&self) -> String {
        let mut keys: Vec<&String> = self.batches.keys().collect();
        keys.sort();

        let mut result = String::from("⚠️ Crash Summary\n\n");

        for key in keys {
            if let Some(msgs) = self.batches.get(key) {
                result.push_str(&format!("{}: {} times\n", key, msgs.len()));
                for msg in msgs {
                    result.push_str(&format!("  - {}\n", msg));
                }
                result.push('\n');
            }
        }

        result
    }
}

fn get_webhook_url(arg_webhook: Option<String>) -> Option<String> {
    arg_webhook.or_else(|| {
        env::var("CRASHFEISHU_WEBHOOK")
            .ok()
            .filter(|s| !s.is_empty())
    })
}

fn parse_token_set(line: &str) -> TokenSet {
    line.trim()
        .split(' ')
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once(':').unwrap();
            (k.to_string(), v.to_string())
        })
        .collect()
}

fn should_monitor(full_name: &str, program: &Vec<String>) -> bool {
    if program.is_empty() {
        return true;
    }

    program.iter().any(|value| {
        if value.contains(':') {
            value == full_name
        } else {
            format!("{}:{}", value, value) == full_name
        }
    })
}

fn push_feishu(webhook: &str, msg: &str) -> MyResult<()> {
    let client = reqwest::blocking::Client::new();
    let payload = FeishuWebhookPayload {
        msg_type: "text".to_string(),
        content: FeishuContent {
            text: msg.to_string(),
        },
    };
    let payload_json = serde_json::to_string(&payload)?;
    debug!("Sending to Feishu webhook: {}", payload_json);
    let res = client
        .post(webhook)
        .header("Content-Type", "application/json")
        .body(payload_json)
        .send()?;

    if res.status().is_success() {
        Ok(())
    } else {
        let status = res.status();
        let text = res
            .text()
            .unwrap_or_else(|e| format!("failed to read response body: {}", e));

        Err(format!("{} {}", status, text).into())
    }
}

fn get_current_timestamp() -> String {
    let now = Local::now();
    format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second())
}

fn tick_minutes(eventname: &str) -> Option<f64> {
    let secs_str = eventname.strip_prefix("TICK_")?;
    let secs: f64 = secs_str.parse().ok()?;
    Some(secs / 60.0)
}

pub struct EventListenerProtocol {}

impl EventListenerProtocol {
    pub fn wait(
        &self,
        input: &mut impl BufRead,
        output: &mut impl Write,
    ) -> io::Result<(TokenSet, Vec<u8>)> {
        self.ready(output)?;

        let mut line = String::new();
        input.read_line(&mut line)?;
        let headers = parse_token_set(&line);

        let len = headers["len"].parse::<usize>().unwrap();
        let mut payload = vec![0; len];
        input.read_exact(&mut payload)?;

        Ok((headers, payload))
    }

    pub fn ready(&self, output: &mut impl Write) -> io::Result<()> {
        output.write_all(b"READY\n")?;
        output.flush()?;
        Ok(())
    }

    pub fn ok(&self, output: &mut impl Write) -> io::Result<()> {
        self.send("OK", output)
    }

    pub fn fail(&self, output: &mut impl Write) -> io::Result<()> {
        self.send("FAIL", output)
    }

    fn send(&self, data: &str, output: &mut impl Write) -> io::Result<()> {
        let n = data.len();
        let result = format!("RESULT {}\n{}", n, data);
        output.write_all(result.as_bytes())?;
        output.flush()?;
        Ok(())
    }
}

pub fn run(args: Args) -> MyResult<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let Args {
        program,
        webhook,
        batch_interval,
    } = args;

    let webhook = get_webhook_url(webhook);

    match batch_interval {
        Some(interval) => run_with_batching(program, webhook, interval),
        None => run_immediate(program, webhook),
    }
}

fn run_immediate(program: Vec<String>, webhook: Option<String>) -> MyResult<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    let listener = EventListenerProtocol {};
    loop {
        let (set, payload) = listener.wait(&mut stdin.lock(), &mut stdout)?;
        debug!("Event token set: {:?}", set);

        if set["eventname"] != "PROCESS_STATE_EXITED" {
            listener.ok(&mut stdout)?;
            continue;
        }

        let pset = parse_token_set(String::from_utf8(payload)?.as_str());
        debug!("Process token set: {:?}", pset);
        if pset["expected"].parse::<usize>()? == 1 {
            listener.ok(&mut stdout)?;
            continue;
        }

        let full_name = format!("{}:{}", pset["groupname"], pset["processname"]);
        if !should_monitor(&full_name, &program) {
            listener.ok(&mut stdout)?;
            continue;
        }

        let msg = format!(
            "Process {} in group {} exited unexpectedly (pid {}) from state {}",
            pset["processname"], pset["groupname"], pset["pid"], pset["from_state"],
        );
        debug!("{}", msg);

        if let Some(webhook) = &webhook {
            match push_feishu(webhook, &msg) {
                Ok(()) => {}
                Err(e) => {
                    error!("failed to push message to feishu: {}", e);
                }
            }
        } else {
            warn!("no webhook specified (neither --webhook argument nor CRASHFEISHU_WEBHOOK environment variable), message will not be pushed to feishu");
        }

        listener.ok(&mut stdout)?;
    }
}

fn run_with_batching(program: Vec<String>, webhook: Option<String>, interval: f64) -> MyResult<()> {
    if interval <= 0.0 {
        return Err("--batch-interval must be positive".into());
    }

    info!(
        "batch mode enabled (interval={} minutes); IMPORTANT: ensure TICK_60 is added to supervisor events configuration",
        interval
    );

    let mut state = BatchingState::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let listener = EventListenerProtocol {};
    let warn_every = Duration::from_secs(180);

    loop {
        let (set, payload) = listener.wait(&mut stdin.lock(), &mut stdout)?;
        debug!("Event token set: {:?}", set);

        if state.maybe_warn_missing_tick(warn_every) {
            if !state.is_empty() {
                let count = state.total_message_count();
                warn!("batch mode requires TICK_60 in supervisor events; no TICK events observed; cleared {} accumulated crash messages", count);
                state.clear();
            } else {
                warn!("batch mode requires TICK_60 in supervisor events; no TICK events observed");
            }
        }

        let eventname = set["eventname"].as_str();
        match eventname {
            "PROCESS_STATE_EXITED" => {
                let pset = parse_token_set(String::from_utf8(payload)?.as_str());
                debug!("Process token set: {:?}", pset);

                if pset["expected"].parse::<usize>()? == 1 {
                    listener.ok(&mut stdout)?;
                    continue;
                }

                let full_name = format!("{}:{}", pset["groupname"], pset["processname"]);
                if !should_monitor(&full_name, &program) {
                    listener.ok(&mut stdout)?;
                    continue;
                }

                let timestamp = get_current_timestamp();
                let msg = format!(
                    "{} | Process {} in group {} exited unexpectedly (pid {}) from state {}",
                    timestamp,
                    pset["processname"],
                    pset["groupname"],
                    pset["pid"],
                    pset["from_state"],
                );
                debug!("{}", msg);

                state.add_crash(full_name, msg);
                listener.ok(&mut stdout)?;
            }
            _ => {
                if let Some(tick_mins) = tick_minutes(eventname) {
                    if state.add_tick(tick_mins, interval) {
                        if state.is_empty() {
                            state.reset_timer();
                        } else if let Some(webhook) = &webhook {
                            let batch_msg = state.format_batch_message();
                            match push_feishu(webhook, &batch_msg) {
                                Ok(()) => {
                                    state.clear();
                                }
                                Err(e) => {
                                    error!("failed to push batch message to feishu: {}", e);
                                }
                            }
                        } else {
                            warn!("no webhook specified (neither --webhook argument nor CRASHFEISHU_WEBHOOK environment variable), batch message will not be pushed to feishu");
                            state.clear();
                        }
                    }
                }

                listener.ok(&mut stdout)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::get_current_timestamp;
    use super::parse_token_set;
    use super::should_monitor;
    use super::tick_minutes;
    use super::BatchingState;
    use super::EventListenerProtocol;
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::time::Duration;

    #[test]
    fn test_parse_token_set_single_pair() {
        let line = "key:value\n";
        let result = parse_token_set(line);
        let mut expected = HashMap::new();
        expected.insert("key".to_string(), "value".to_string());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_token_set_multiple_pairs() {
        let line = "  key1:value1 key2:value2 \n";
        let result = parse_token_set(line);
        let mut expected = HashMap::new();
        expected.insert("key1".to_string(), "value1".to_string());
        expected.insert("key2".to_string(), "value2".to_string());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_should_monitor_empty_program() {
        let full_name = "test_name";
        let program: Vec<String> = Vec::new();
        assert!(should_monitor(full_name, &program));
    }

    #[test]
    fn test_should_monitor_with_colon_match() {
        let full_name = "name:value";
        let program = vec!["name:value".to_string(), "other_value".to_string()];
        assert!(should_monitor(full_name, &program));
    }

    #[test]
    fn test_should_monitor_without_colon_match() {
        let full_name = "name:name";
        let program = vec!["name".to_string(), "other_name".to_string()];
        assert!(should_monitor(full_name, &program));
    }

    #[test]
    fn test_should_monitor_no_match() {
        let full_name = "unmatched_name";
        let program = vec!["name:value".to_string(), "other_value".to_string()];
        assert!(!should_monitor(full_name, &program));
    }

    #[test]
    fn test_event_listener_ready() {
        let protocol = EventListenerProtocol {};
        let mut output = Vec::new();

        protocol.ready(&mut output).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "READY\n");
    }

    #[test]
    fn test_event_listener_ok() {
        let protocol = EventListenerProtocol {};
        let mut output = Vec::new();

        protocol.ok(&mut output).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "RESULT 2\nOK");
    }

    #[test]
    fn test_event_listener_fail() {
        let protocol = EventListenerProtocol {};
        let mut output = Vec::new();

        protocol.fail(&mut output).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "RESULT 4\nFAIL");
    }

    #[test]
    fn test_event_listener_wait() {
        let protocol = EventListenerProtocol {};
        let input_data = b"len:5 eventname:PROCESS_STATE_EXITED\nHello";
        let mut input = Cursor::new(input_data.to_vec());
        let mut output = Vec::new();

        let (headers, payload) = protocol.wait(&mut input, &mut output).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "READY\n");

        let mut expected_headers = HashMap::new();
        expected_headers.insert("len".to_string(), "5".to_string());
        expected_headers.insert("eventname".to_string(), "PROCESS_STATE_EXITED".to_string());
        assert_eq!(headers, expected_headers);

        assert_eq!(payload, b"Hello");
    }

    #[test]
    fn test_tick_minutes() {
        assert_eq!(tick_minutes("TICK_60"), Some(1.0));
        assert_eq!(tick_minutes("TICK_5"), Some(5.0 / 60.0));
        assert_eq!(tick_minutes("PROCESS_STATE_EXITED"), None);
    }

    #[test]
    fn test_get_current_timestamp_format() {
        let ts = get_current_timestamp();
        assert_eq!(ts.len(), 8);
        assert_eq!(ts.chars().nth(2), Some(':'));
        assert_eq!(ts.chars().nth(5), Some(':'));
    }

    #[test]
    fn test_batching_state_add_crash_groups() {
        let mut state = BatchingState::new();
        state.add_crash("g:p".to_string(), "14:30:00 | a".to_string());
        state.add_crash("g:p".to_string(), "14:31:00 | b".to_string());
        state.add_crash("x:y".to_string(), "14:32:00 | c".to_string());

        let msg = state.format_batch_message();
        assert!(msg.contains("g:p: 2 times"));
        assert!(msg.contains("x:y: 1 times"));
        assert!(msg.contains("14:31:00"));
        assert!(msg.contains("14:32:00"));
    }

    #[test]
    fn test_batching_state_tick_and_clear() {
        let mut state = BatchingState::new();
        state.add_crash("g:p".to_string(), "14:30:00 | a".to_string());
        assert!(!state.add_tick(1.0, 2.0));
        assert!(state.add_tick(1.0, 2.0));
        state.clear();
        assert!(state.is_empty());
        assert_eq!(state.batch_mins, 0.0);
    }

    #[test]
    fn test_batch_message_format() {
        let mut state = BatchingState::new();
        state.add_crash("g:p".to_string(), "14:30:00 | Test crash".to_string());
        let msg = state.format_batch_message();

        // Verify the message doesn't contain characters that would break JSON
        assert!(
            !msg.contains('"'),
            "Batch message should not contain unescaped quotes"
        );
        assert!(
            !msg.contains('\\'),
            "Batch message should not contain backslashes"
        );

        // Verify basic format
        assert!(msg.contains("⚠️ Crash Summary"));
        assert!(msg.contains("g:p: 1 times"));
    }

    #[test]
    fn test_maybe_warn_missing_tick_returns_bool() {
        let mut state = BatchingState::new();
        let short_dur = Duration::from_millis(10);

        // Initially no tick, not enough time passed
        assert!(!state.maybe_warn_missing_tick(short_dur));

        // Wait and warn
        std::thread::sleep(short_dur);
        assert!(state.maybe_warn_missing_tick(short_dur));

        // Should not warn again immediately
        assert!(!state.maybe_warn_missing_tick(short_dur));

        // Add tick, should never warn
        state.add_tick(1.0, 1.0);
        assert!(!state.maybe_warn_missing_tick(short_dur));
    }

    #[test]
    fn test_messages_cleared_on_tick_warning() {
        let mut state = BatchingState::new();
        let short_dur = Duration::from_millis(10);

        // Add some crashes
        state.add_crash("g:p".to_string(), "msg1".to_string());
        state.add_crash("g:p".to_string(), "msg2".to_string());
        assert_eq!(state.total_message_count(), 2);

        // Wait for warning to trigger
        std::thread::sleep(short_dur);
        assert!(state.maybe_warn_missing_tick(short_dur));
        assert_eq!(state.total_message_count(), 2); // Still there before explicit clear

        // Simulate the clear logic from run_with_batching
        if !state.is_empty() {
            state.clear();
        }
        assert!(state.is_empty());
    }
}
