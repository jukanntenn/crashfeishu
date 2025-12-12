use clap::Parser;
use log::{debug, error, warn};
use reqwest;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::io::{self, BufRead, Write};

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
}

type MyResult<T> = Result<T, Box<dyn Error>>;
type TokenSet = HashMap<String, String>;

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
    let payload = format!(r#"{{"msg_type":"text","content":{{"text":"{}"}}}}"#, msg);
    let res = client
        .post(webhook)
        .header("Content-Type", "application/json")
        .body(payload)
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
    env_logger::init();

    let webhook = get_webhook_url(args.webhook);

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
        if !should_monitor(&full_name, &args.program) {
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

#[cfg(test)]
mod tests {
    use super::parse_token_set;
    use super::should_monitor;
    use super::EventListenerProtocol;
    use std::collections::HashMap;
    use std::io::Cursor;

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
        let input_data = b"len:5 event:PROCESS_STATE_EXITED\nHello";
        let mut input = Cursor::new(input_data.to_vec());
        let mut output = Vec::new();

        let (headers, payload) = protocol.wait(&mut input, &mut output).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "READY\n");

        let mut expected_headers = HashMap::new();
        expected_headers.insert("len".to_string(), "5".to_string());
        expected_headers.insert("event".to_string(), "PROCESS_STATE_EXITED".to_string());
        assert_eq!(headers, expected_headers);

        assert_eq!(payload, b"Hello");
    }
}
