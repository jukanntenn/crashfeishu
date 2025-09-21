# crashfeishu

简体中文 | [English](./README_en.md)

`crashfeishu` 是一个 Supervisor 事件监听器，用于在管理的进程意外崩溃时发送 Feishu 通知。

## 安装

下载可执行文件：

```bash
# for x86_64
curl -L https://github.com/jukanntenn/crashfeishu/releases/download/v0.1.2/crashfeishu-v0.1.2-x86_64-unknown-linux-musl.tar.gz | tar -xzv

# for arm64
curl -L https://github.com/jukanntenn/crashfeishu/releases/download/v0.1.2/crashfeishu-v0.1.2-aarch64-unknown-linux-gnu.tar.gz | tar -xzv
```

或者使用 cargo：

```bash
cargo install crashfeishu
```

## 配置 Supervisor

在 Supervisor 的配置文件中添加以下内容：

```ini
[eventlistener:crashfeishu]
command = /path/to/crashfeishu -w <webhook_url> -p <program_name>
events = PROCESS_STATE
```

crashfeishu 参数说明：

- `-w <webhook_url>`：Feishu Webhook URL，用于发送通知。
- `-p <program_name>`：监听的进程名称，支持 group_name:process_name 格式（用于进程组），可重复使用该参数监听多个进程，不指定则默认监听所有进程。

## 示例

### 1. 监听单个进程

假设要监听名为 `my_process` 的进程，Feishu Webhook URL 为 `https://open.feishu.cn/open-apis/bot/v2/hook/your-webhook-token`，则配置如下：

```ini
[eventlistener:crashfeishu]
command = /path/to/crashfeishu -w https://open.feishu.cn/open-apis/bot/v2/hook/your-webhook-token -p my_process
events = PROCESS_STATE
```

### 2. 监听多个进程（包含组进程）

```ini
[eventlistener:crashfeishu]
command = /path/to/crashfeishu -w https://open.feishu.cn/open-apis/bot/v2/hook/your-webhook-token -p my_group:my_process -p other_process
events = PROCESS_STATE
```

### 3. 监听所有进程

```ini
[eventlistener:crashfeishu]
command = /path/to/crashfeishu -w https://open.feishu.cn/open-apis/bot/v2/hook/your-webhook-token
events = PROCESS_STATE
```
