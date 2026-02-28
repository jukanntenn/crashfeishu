# crashfeishu Verification Manual

## Prerequisites

### Environment

| Component      | Requirement           | Check Command           |
| -------------- | --------------------- | ----------------------- |
| Rust           | 1.70+                 | `rustc --version`       |
| Supervisor     | 3.0+                  | `supervisord --version` |
| Feishu Webhook | Valid bot webhook URL | -                       |

### Build

```bash
cd /path/to/crashfeishu
cargo build --release
./target/release/crashfeishu --version
./target/release/crashfeishu --help
```

### Webhook Configuration

Set via command line argument or environment variable:

```bash
# Command line
-w https://open.feishu.cn/open-apis/bot/v2/hook/xxx

# Environment variable (fallback)
export CRASHFEISHU_WEBHOOK="https://open.feishu.cn/open-apis/bot/v2/hook/xxx"
```

**Priority**: Command line argument > Environment variable

---

## Test Scenarios

### Scenario 1: Immediate Mode (Default)

**Purpose**: Verify immediate notification on each crash

#### Supervisor Config

`/etc/supervisor/conf.d/test-immediate.conf`:

```ini
[supervisord]
nodaemon=true
logfile=/tmp/supervisord.log

[program:crash_app]
command=/bin/bash -c 'echo "Starting..."; sleep 1; exit 1'
autorestart=true
startretries=999
stopasgroup=true
killasgroup=true

[eventlistener:crashfeishu]
command=/path/to/crashfeishu/target/release/crashfeishu -w $CRASHFEISHU_WEBHOOK -p crash_app
events=PROCESS_STATE
environment=RUST_LOG=debug,CRASHFEISHU_WEBHOOK=$CRASHFEISHU_WEBHOOK
stdout_logfile=/tmp/crashfeishu.log
stderr_logfile=/tmp/crashfeishu_err.log
```

#### Steps

```bash
sudo supervisorctl shutdown
sudo supervisord -c /etc/supervisor/conf.d/test-immediate.conf
tail -f /tmp/crashfeishu_err.log
```

#### Expected Results

| Check          | Expected Value                                                                          |
| -------------- | --------------------------------------------------------------------------------------- |
| crash_app      | Auto-restarts                                                                           |
| Each crash     | Immediate Feishu notification                                                           |
| Feishu message | `Process crash_app in group crash_app exited unexpectedly (pid XXX) from state RUNNING` |
| Timestamp      | Matches server local time (`date +"%H:%M:%S"`)                                          |
| Log output     | No "batch mode enabled" log                                                             |

#### Cleanup

```bash
sudo supervisorctl shutdown
sudo rm /etc/supervisor/conf.d/test-immediate.conf
```

---

### Scenario 2: Batch Mode

**Purpose**: Verify batch mode aggregates multiple crashes into periodic notifications

#### Supervisor Config

`/etc/supervisor/conf.d/test-batch.conf`:

```ini
[supervisord]
nodaemon=true
logfile=/tmp/supervisord.log

[program:batch_test_app]
command=/bin/bash -c 'echo "Starting..."; sleep 5; exit 1'
autorestart=true
startretries=999
stopasgroup=true
killasgroup=true

[eventlistener:crashfeishu_batch]
command=/path/to/crashfeishu/target/release/crashfeishu --batch-interval 1
events=PROCESS_STATE,TICK_60
environment=RUST_LOG=debug,CRASHFEISHU_WEBHOOK=$CRASHFEISHU_WEBHOOK
stdout_logfile=/tmp/crashfeishu_batch.log
stderr_logfile=/tmp/crashfeishu_batch_err.log
```

#### Steps

```bash
sudo supervisord -c /etc/supervisor/conf.d/test-batch.conf
tail -f /tmp/crashfeishu_batch_err.log
```

Wait 1-2 minutes for multiple crashes and TICK event to trigger batch notification.

#### Expected Results

| Check               | Expected Value                             |
| ------------------- | ------------------------------------------ |
| Startup log         | Contains `batch mode enabled (interval=1)` |
| Crash count         | ~10-12 crashes in 1 minute (5s interval)   |
| Feishu notification | 1 batch message after interval elapsed     |

#### Feishu Message Format

```
⚠️ Crash Summary

batch_test_app:batch_test_app: N times
  - 17:42:59 | Process batch_test_app in group batch_test_app exited unexpectedly (pid 12345) from state RUNNING
  - 17:43:04 | Process batch_test_app in group batch_test_app exited unexpectedly (pid 12346) from state RUNNING
  ...
```

**Timestamp**: Server local time (HH:MM:SS format)

---

### Scenario 3: Multi-Process Batching

**Purpose**: Verify crashes from different processes are grouped correctly

#### Supervisor Config

`/etc/supervisor/conf.d/test-multi.conf`:

```ini
[supervisord]
nodaemon=true
logfile=/tmp/supervisord.log

[program:app1]
command=/bin/bash -c 'sleep 3; exit 1'
autorestart=true
startretries=999

[program:app2]
command=/bin/bash -c 'sleep 4; exit 1'
autorestart=true
startretries=999

[group:mygroup]
programs=app1,app2

[eventlistener:crashfeishu_multi]
command=/path/to/crashfeishu/target/release/crashfeishu --batch-interval 1
events=PROCESS_STATE,TICK_60
environment=RUST_LOG=debug,CRASHFEISHU_WEBHOOK=$CRASHFEISHU_WEBHOOK
stdout_logfile=/tmp/crashfeishu_multi.log
stderr_logfile=/tmp/crashfeishu_multi_err.log
```

#### Expected Message Format

```
⚠️ Crash Summary

mygroup:app1: N times
  - 17:42:59 | Process app1 in group mygroup exited unexpectedly (pid 12345) from state RUNNING
  ...

mygroup:app2: M times
  - 17:43:02 | Process app2 in group mygroup exited unexpectedly (pid 12346) from state RUNNING
  ...
```

---

### Scenario 4: Missing TICK_60 Warning

**Purpose**: Verify proper warning when TICK_60 is not configured in batch mode

#### Supervisor Config (Incorrect - Missing TICK_60)

```ini
[eventlistener:crashfeishu_no_tick]
command=/path/to/crashfeishu/target/release/crashfeishu --batch-interval 1
events=PROCESS_STATE
environment=RUST_LOG=debug,CRASHFEISHU_WEBHOOK=$CRASHFEISHU_WEBHOOK
```

#### Expected Behavior

| Timeline         | Expected Log Output                                                                                 |
| ---------------- | --------------------------------------------------------------------------------------------------- |
| At startup       | `batch mode enabled (interval=1 minutes); IMPORTANT: ensure TICK_60 is added...`                    |
| After ~3 min     | `batch mode requires TICK_60 in supervisor events; no TICK events observed`                         |
| Every ~3 min     | Warning repeats                                                                                     |
| Any time         | No batch Feishu notifications sent                                                                  |
| If crashes occur | `...; cleared N accumulated crash messages` (batched messages discarded due to missing TICK events) |

#### Correct Configuration

```ini
events=PROCESS_STATE,TICK_60
```

After adding TICK_60, warnings stop and batch notifications work.

---

### Scenario 5: Webhook Send Failure Handling

**Purpose**: Verify graceful handling of webhook send failures

#### Steps

```bash
# Use invalid webhook
export CRASHFEISHU_WEBHOOK="https://invalid-url.test/fail"

# Start batch mode supervisor
sudo supervisord -c /etc/supervisor/conf.d/test-batch.conf
```

#### Expected Behavior

| Event         | Expected Behavior                             |
| ------------- | --------------------------------------------- |
| Send fails    | Error logged, batch retained (not cleared)    |
| Next TICK_60  | Retries sending with all accumulated messages |
| Send succeeds | Batch cleared, timer reset                    |

Log output: `failed to push batch message to feishu: ...`

---

### Scenario 6: Process Filtering

**Purpose**: Verify `-p` parameter correctly filters which processes to monitor

#### Test Configurations

| Command Argument                | Monitors                             |
| ------------------------------- | ------------------------------------ |
| (none)                          | All processes                        |
| `-p myapp`                      | Only `myapp:myapp`                   |
| `-p mygroup:myapp`              | Only `mygroup:myapp`                 |
| `-p app1 -p app2`               | Only `app1:app1` and `app2:app2`     |
| `-p group1:app1 -p group2:app2` | Only `group1:app1` and `group2:app2` |

#### Verification

Use immediate mode with multiple crashing processes. Only specified processes should trigger Feishu notifications.

---

### Scenario 7: Timestamp Verification

**Purpose**: Verify timestamps use server local time

#### Steps

```bash
# Check server time and timezone
date
date +"%H:%M:%S %Z %z"

# Trigger a crash (immediate mode)
# Compare Feishu timestamp with server time
```

#### Verification

The timestamp in Feishu messages should match the server's local time:

```bash
# Example: Server in CST (UTC+8)
$ date +"%H:%M:%S %Z"
17:42:59 CST

# Feishu message shows:
17:42:59 | Process app2 in group mygroup exited unexpectedly (pid 1686675) from state RUNNING
```

**Format**: `HH:MM:SS` in server local timezone

---

## Verification Checklist

### Core Functionality

- [ ] Build succeeds: `cargo build --release`
- [ ] Unit tests pass: `cargo test`
- [ ] `--help` shows all options: `-p`, `-w`, `--batch-interval`
- [ ] Webhook can be set via `-w` argument
- [ ] Webhook can be set via `CRASHFEISHU_WEBHOOK` env var
- [ ] Command line takes priority over env var
- [ ] Immediate mode sends notification per crash
- [ ] Batch mode aggregates notifications
- [ ] Process filtering (`-p`) works correctly

### Batch Mode

- [ ] `--batch-interval` parameter available
- [ ] Startup log shows "batch mode enabled"
- [ ] Batch notifications sent when TICK_60 configured
- [ ] Warning shown every 3 minutes when TICK_60 missing
- [ ] Accumulated messages cleared when TICK_60 missing
- [ ] Multi-process crashes grouped by process name
- [ ] Webhook send failures are retried on next tick

### Message Format

- [ ] Timestamps present in batch messages
- [ ] Timestamps use server local time (verify with `date`)
- [ ] Timestamp format: `HH:MM:SS`
- [ ] Process name and group shown correctly
- [ ] PID included in message
- [ ] "from state" included in message
- [ ] Crash summary header "⚠️ Crash Summary" present
- [ ] Process count shown ("N times")
