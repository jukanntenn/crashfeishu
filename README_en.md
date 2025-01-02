# crashfeishu

[简体中文](./README.md) | English

A Supervisor event listener that sends Feishu notifications when managed processes crash.

## Installation

Download the executable file:

```bash
curl -L https://github.com/jukanntenn/crashfeishu/releases/download/v0.1.0/crashfeishu-v0.1.0-x86_64-unknown-linux-musl.tar.gz | tar -xzv
```

Or use cargo:

```bash
cargo install crashfeishu
```

## Configure Supervisor

Add the following content to the Supervisor configuration file:

```ini
[eventlistener:crashfeishu]
command = /path/to/crashfeishu -w <webhook_url> -p <program_name>
events = PROCESS_STATE
```

Parameters description of crashfeishu:

- `-w <webhook_url>`: Feishu Webhook URL, used to send notifications.
- `-p <program_name>`: The name of the process to be monitored. The format of `group_name:process_name` is supported (for group processes). This parameter can be used repeatedly to monitor multiple processes. If not specified, all processes will be monitored by default.

## Examples

### 1. Monitor a single process

Assume that you want to monitor a process named `my_process`, and the Feishu Webhook URL is `https://open.feishu.cn/open-apis/bot/v2/hook/your-webhook-token`. The configuration is as follows:

```ini
[eventlistener:crashfeishu]
command = /path/to/crashfeishu -w https://open.feishu.cn/open-apis/bot/v2/hook/your-webhook-token -p my_process
events = PROCESS_STATE
```

### 2. Monitor multiple processes (including group processes)

```ini
[eventlistener:crashfeishu]
command = /path/to/crashfeishu -w https://open.feishu.cn/open-apis/bot/v2/hook/your-webhook-token -p my_group:my_process -p other_process
events = PROCESS_STATE
```

### 3. Monitor all processes

```ini
[eventlistener:crashfeishu]
command = /path/to/crashfeishu -w https://open.feishu.cn/open-apis/bot/v2/hook/your-webhook-token
events = PROCESS_STATE
```
