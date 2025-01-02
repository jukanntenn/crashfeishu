use clap::Parser;
use log::{debug, error, warn};
use reqwest;
use std::collections::HashMap;
use std::error::Error;
use std::io::{self, BufRead, Write};

#[derive(Parser, Debug)]
#[clap(author = "jukanntenn <jukanntenn@outlook.com>", version, about, long_about = None)]
pub struct Args {
    /// Specify a supervisor process_name. Send Feishu message when this process
    /// transitions to the EXITED state unexpectedly. If this process is
    /// part of a group, it can be specified using the
    /// 'group_name:process_name' syntax.
    ///
    /// Example:
    ///   -p my_process
    ///   -p my_group:my_process
    #[clap(
        short = 'p',
        long = "program",
        help = "Specify the supervisor process name or group:process to monitor."
    )]
    pub program: Vec<String>,

    /// Specify the Feishu webhook URL to send messages to.
    ///
    /// Example:
    ///   -w https://open.feishu.cn/open-apis/bot/v2/hook/your-webhook-token
    #[clap(
        short = 'w',
        long = "webhook",
        help = "Specify the Feishu webhook URL to send crash notifications."
    )]
    pub webhook: Option<String>,
}

type MyResult<T> = Result<T, Box<dyn Error>>;
type TokenSet = HashMap<String, String>;

fn parse_token_set(line: &str) -> TokenSet {
    let mut set = HashMap::new();
    for pair in line.trim().split(' ') {
        let mut iter = pair.splitn(2, ':');
        let k = iter.next().unwrap().to_string();
        let v = iter.next().unwrap().to_string();
        set.insert(k, v);
    }
    set
}

fn is_wanted_program(full_name: &str, program: &Vec<String>) -> bool {
    if program.len() == 0 {
        return true;
    }

    for value in program {
        if value.contains(':') {
            if value == full_name {
                return true;
            }
        } else {
            if full_name == format!("{}:{}", value, value) {
                return true;
            }
        }
    }

    false
}

fn send_feishu(webhook: &str, msg: &str) -> MyResult<()> {
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
        let code = res.status();
        let text = res
            .text()
            .unwrap_or_else(|e| format!("Failed to read response body: {}", e));
        Err(format!(
            "Failed to send message to Feishu. Status code: {}. Response: {}",
            code, text
        )
        .into())
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
        if !is_wanted_program(&full_name, &args.program) {
            listener.ok(&mut stdout)?;
            continue;
        }

        let msg = format!(
            "Process {} in group {} exited unexpectedly (pid {}) from state {}",
            pset["processname"], pset["groupname"], pset["pid"], pset["from_state"],
        );

        debug!("{}", msg);
        if let Some(webhook) = args.webhook.as_ref() {
            match send_feishu(webhook, &msg) {
                Ok(()) => {}
                Err(e) => {
                    error!("Failed to send message to Feishu: {}", e);
                }
            }
        } else {
            warn!("No webhook specified, message not sent to Feishu");
        }

        listener.ok(&mut stdout)?;
    }
}

#[cfg(test)]
mod tests {
    use super::is_wanted_program;
    use super::parse_token_set;
    use std::collections::HashMap;

    #[test]
    fn test_parse_token_set_single_pair() {
        let line = "key:value";
        let result = parse_token_set(line);
        let mut expected = HashMap::new();
        expected.insert("key".to_string(), "value".to_string());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_token_set_multiple_pairs() {
        let line = "  key1:value1 key2:value2 ";
        let result = parse_token_set(line);
        let mut expected = HashMap::new();
        expected.insert("key1".to_string(), "value1".to_string());
        expected.insert("key2".to_string(), "value2".to_string());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_is_wanted_program_empty_program() {
        let full_name = "test_name";
        let program: Vec<String> = Vec::new();
        assert!(is_wanted_program(full_name, &program));
    }

    #[test]
    fn test_is_wanted_program_with_colon_match() {
        let full_name = "name:value";
        let program = vec!["name:value".to_string(), "other_value".to_string()];
        assert!(is_wanted_program(full_name, &program));
    }

    #[test]
    fn test_is_wanted_program_without_colon_match() {
        let full_name = "name:name";
        let program = vec!["name".to_string(), "other_name".to_string()];
        assert!(is_wanted_program(full_name, &program));
    }

    #[test]
    fn test_is_wanted_program_no_match() {
        let full_name = "unmatched_name";
        let program = vec!["name:value".to_string(), "other_value".to_string()];
        assert!(!is_wanted_program(full_name, &program));
    }
}
