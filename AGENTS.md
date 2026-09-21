# AGENTS.md

面向在本仓库工作的 AI 编码代理（Claude Code / Codex / Cursor / Copilot 等）。
本文件是**操作性约定**，不是产品文档。产品用法见 `README.md`。

---

## 1. 项目一句话

Rust 编写的轻量 TCP 端口转发器，**主要服务于测试场景**：把打进本机端口的连接透明转发到另一台机器的端口。

```
客户端 → 本机 :100 ──[tcp-transfer]──→ 目标主机 :2000
```

---

## 2. 技术栈与事实基线

| 项 | 值 | 说明 |
|---|---|---|
| 语言 | Rust **edition 2021** | 无 `rust-toolchain` 文件，用本机默认工具链 |
| 构建 | Cargo（本地实测 1.97.1） | |
| 异步运行时 | `tokio`（`rt-multi-thread` + `macros` + `net` + `io-util` + `time` + `sync` + `signal`） | |
| CLI | `clap` v4（derive） | |
| 日志 | `tracing` + `tracing-subscriber`（`env-filter` + `fmt` + `json`） | |
| 错误 | `anyhow` | |
| 目标平台 | Windows（开发） / Linux x86_64-musl（部署，静态） | |

**当前仓库状态（改动前请先确认是否已变化）**
- ❌ 无 `tests/` 目录 —— **目前没有任何自动化测试**
- ❌ 无 `.github/` —— **没有 CI**
- ❌ 不是 git 仓库（可能已初始化，请以 `git status` 为准）

---

## 3. 目录结构

```
.
├── Cargo.toml
├── Cargo.lock
├── Dockerfile-linux          # Linux 静态构建（两阶段）
├── .dockerignore
├── README.md                 # 产品用法（面向用户）
├── AGENTS.md                 # 本文件（面向代理）
├── src/
│   ├── main.rs               # 入口：解析 CLI → 初始化日志 → 调 proxy::run_forward
│   ├── cli.rs                # clap 定义 + listen_addr() / target_addr() 辅助方法
│   ├── proxy.rs              # 核心：accept 循环 + 双向 pipe + 连接统计
│   └── stats.rs              # 无锁原子计数器 + human_bytes 格式化
├── dist/                     # 构建产物导出目录（.gitignore 忽略）
└── target/                   # Cargo 输出（.gitignore 忽略）
```

---

## 4. 代码结构与职责边界

### `src/cli.rs` — 参数定义
**平铺参数，没有子命令**；监听与目标**都写成完整 `host:port`**（2026-09-21 两次重构的结果，勿回退）。

```rust
pub struct Cli {
    pub listen: SocketAddr,     // -l / --listen   必填，如 0.0.0.0:444（clap 自动校验格式）
    pub target: String,         // -t / --target   必填，如 192.168.2.203:333 或 example.com:80
    pub timeout: u64,           // -T / --timeout  默认 0（不超时）
    pub log_level: String,      // -v / --log-level 默认 info
    pub json_log: bool,         // --json-log
}
```

**为什么 `listen` 是 `SocketAddr` 而 `target` 是 `String`**（这是刻意的，不要"统一"它们）：
- `listen` 一定是本机 IP 字面量，用 `SocketAddr` 可让 clap 在**启动前**就拒绝 `0.0.0.0`（漏端口）这类错误。
- `target` 允许域名（`example.com:80`），而域名**无法**解析为 `SocketAddr`，只能用 `String` 交给
  `TcpStream::connect` 去解析。

唯一辅助方法 `target_addr()` 只是 `self.target.clone()`，保留它是为了让 `run_forward` 的调用点
不直接依赖字段名，改动成本也低。

⚠️ **历史**：曾有过 `-p/--port` + `--target` + `--target-port` 三段式，以及更早的
`forward -l ... -t ...` 子命令形式。**均已移除，不要恢复。** 用户明确要求最简参数。


### `src/main.rs` — 入口
只做三件事：解析 CLI → 初始化日志 → 调用 `proxy::run_forward`，把 `Result` 映射为 `ExitCode`。
**不要往这里塞业务逻辑。**

（历史：`listen` 曾是 `IpAddr` + 单独 `port`，需要在入口拼 `SocketAddr` 并在绑定 0.0.0.0 时打提示。
现在 `listen` 直接就是 `SocketAddr`，那两段代码已删除——别再把它们加回来。）

### `src/proxy.rs` — 核心转发
- `run_forward(listen, target, idle_timeout)`：bind → accept 循环 → 每连接 `tokio::spawn`
- `handle_conn()`：连目标 → `into_split()` → 起两个方向的任务 → 双向 `pipe()`
- `pipe()`：`BUF_SIZE = 8 KiB` 固定缓冲区循环读写；`idle_timeout > 0` 时用 `tokio::time::timeout` 包裹每次 read
- accept 失败**只 warn 不退出**（保持服务存活），连接内错误 warn 后结束该连接

### `src/stats.rs` — 统计
全 `AtomicU64` + `Ordering::Relaxed`（统计精度不重要，性能优先）。每 10 秒由 `proxy.rs` 里的后台任务打一条 `stats` 日志。

---

## 5. 构建与验证命令

> Windows 环境下 Git Bash 执行 docker 命令**必须**加 `MSYS_NO_PATHCONV=1`，否则路径会被错误改写成 `d:\d\...`。

### 本地原生构建（开发时用）
```bash
cargo build --release
# 产物：target/release/tcp-transfer.exe (Windows) 或 target/release/tcp-transfer (Linux)
```

### 快速自检（提交前至少跑这三条）
```bash
cargo build --release          # 必须零警告通过
cargo clippy -- -D warnings    # 若 clippy 可用
./target/release/tcp-transfer --help   # 确认 CLI 未被破坏
```

### Linux 静态构建（部署产物）
```bash
docker build --pull=false -f Dockerfile-linux -t tcp-transfer:linux .
```

**`--pull=false` 是必需的**，原因见第 7 节。基础镜像固定为内网私服：
`zhcoder-docker-registry.com:8000/builder/rust:1.96-slim-musl`

导出静态二进制到 `dist/`：
```bash
docker create --name tcp-transfer-tmp tcp-transfer:linux
MSYS_NO_PATHCONV=1 docker cp tcp-transfer-tmp:/usr/local/bin/tcp-transfer dist/tcp-transfer
docker rm tcp-transfer-tmp
```

---

## 6. 验证纪律（重要）

**本仓库没有自动化测试，所以"改完必须手工验证"是硬性要求。**

任何涉及转发路径的改动，至少完成以下三项验证：

| # | 验证项 | 做法 | 期望 |
|---|---|---|---|
| 1 | 编译 | `cargo build --release` | 零警告通过 |
| 2 | CLI 未破坏 | `--help` / 参数解析 | 参数名与默认值不变 |
| 3 | **端到端转发** | 起一个后端 + 起转发器 + 客户端经转发器请求 | 响应与**直连后端完全一致** |

### 端到端验证模板（Docker，跨平台可复现）

```bash
docker network create e2e-net

# 后端：监听 333
docker run -d --name e2e-backend --network e2e-net -w /tmp python:3.12-alpine \
  python -m http.server 333

# 转发器：-l 0.0.0.0:444 → 后端 :333
docker run -d --name e2e-fwd --network e2e-net tcp-transfer:linux \
  -l 0.0.0.0:444 -t e2e-backend:333

# 客户端经转发器请求（应与直连 e2e-backend:333 的响应逐字一致）
docker run --rm --network e2e-net alpine:3.20 \
  sh -c 'printf "GET / HTTP/1.0\r\n\r\n" | nc e2e-fwd 444'
```

### 大流量完整性验证（改了 `pipe()` / 缓冲区 / 超时逻辑时**必做**）

用 python socket 比对 MD5，**不要用 `nc -l` 循环装置**——它有时序缺陷，会给出 0 字节的假失败：

```bash
# 后端：发送 1MB 随机数据，自报 MD5
docker run -d --name e2e-big --network e2e-net python:3.12-alpine python -c "
import socket,os,hashlib
data=os.urandom(1024*1024)
print('MD5', hashlib.md5(data).hexdigest(), flush=True)
s=socket.socket(); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
s.bind(('0.0.0.0',3000)); s.listen(5)
while True:
    c,_=s.accept(); c.sendall(data); c.close()
"
# 客户端经转发器取回并比对 MD5（应与后端自报值一致）
```

**排查原则**：若怀疑转发器丢数据，**先验证直连后端是否也异常**。若直连同样异常，问题在测试装置而非代码——不要急着改 `proxy.rs`。

---

## 7. 已知陷阱（踩过的坑，勿重复）

### 7.1 内网私服只支持明文 HTTP
`zhcoder-docker-registry.com:8000` 是 Harbor，**不支持 TLS**（`https://` 直接连接重置）。
- Docker 29 对它执行 `docker login` 会报 `503 Service Unavailable`。
- **结论：构建时加 `--pull=false`，依赖本地已有镜像，不要尝试登录。**

### 7.2 `crt-static` 不能靠 `RUSTFLAGS` 环境变量
基础镜像的 `~/.cargo/config.toml` 为 musl target 固定了 per-target 数组：
```toml
[target.x86_64-unknown-linux-musl]
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```
per-target 的 `rustflags` **数组会整体替换** `RUSTFLAGS` 环境变量而非合并。
→ 直接 `export RUSTFLAGS="-C target-feature=+crt-static"` **完全无效**。
→ 正确做法：用 `sed` 往原数组**追加** `crt-static`（见 `Dockerfile-linux` 注释）。

### 7.3 musl 的 `ldd` 会误报
对**已静态链接**的二进制，musl `ldd` 会打印 `/lib/ld-musl-x86_64.so.1 (...)`，看起来像动态链接——**这是误报**。
- 判静态的正确方法：① `readelf -l` 查**无 `PT_INTERP` 段**；② 放进 `FROM scratch` 容器能跑。
- **不要在 Dockerfile 里用 `ldd` 做静态性断言**（已改为 `readelf` 检查）。

实测本仓库产物的真实特征（别被 `DYN` 吓到）：

```
$ readelf -l dist/tcp-transfer | grep -c INTERP   →  0        # 无 PT_INTERP = 无需 loader
$ readelf -h dist/tcp-transfer | grep Type:       →  DYN ...  # static-PIE，正常
```

`Type: DYN` 是 **static PIE**（现代 Rust 默认），**不等于动态链接**——判据是 `PT_INTERP` 是否存在，不是 `e_type`。

### 7.4 不要用 `--mount=type=bind` 优化源码 COPY
曾尝试用 bind mount 挂 `src` 以省去一次 COPY，结果产出 **389KB 的坏二进制**（`--version` 无输出），因为依赖缓存层被绕过、只链接了 stub。
→ 保持现有的**双层 COPY**（先 `Cargo.toml`/`Cargo.lock` 建依赖缓存，再 `COPY src`）结构，并在 rebuild 前 `touch src/main.rs` 保证失效。

### 7.5 Windows Git Bash 路径改写
`docker cp` / `docker build` 的路径参数在 Git Bash 下会被 MSYS 改写。
→ 加 `MSYS_NO_PATHCONV=1` 前缀，或使用 `D:/...` 正斜杠形式。

---

## 8. 编码约定

- **错误处理**：库函数返回 `anyhow::Result`；用 `.with_context(|| ...)` 附加上下文（现有代码风格：`format!("connect {}", target)`）。可恢复错误用 `tracing::warn!`，不要 `panic!`。
- **日志**：统一用 `tracing` 宏 + 结构化字段（`tracing::info!(%listen, target = %target, "listening")`）。**禁止 `println!`**。
- **并发**：每连接 `tokio::spawn`，共享状态一律 `Arc<AtomicU64>`，用 `Ordering::Relaxed`。
- **命名**：函数/变量 snake_case；错误消息句式统一为小写动词短语（`"accept failed"`、`"connect {}"`）。
- **注释**：只在**非显而易见的决策**上写注释（如上面 7.2 的静默失效），不复述代码在做什么。
- **依赖**：新增依赖前先确认必要。本工具定位是"轻量"，能不加就不加。

---

## 9. 改动红线

以下变更**必须**先与维护者确认，不要擅自做：

| 内容 | 原因 |
|---|---|
| 改 `-l` / `-t` 的参数名或其"完整 host:port"语义 | 用户明确要求的最简形式，已在用 |
| 恢复 `-p` / `--target` / `--target-port` 等拆分写法 | 用户明确要求删除，见 §4 历史注记 |
| 拆分成 `listen_addr()` + 独立 port 字段 | 同上 |
| 引入子命令或其他 CLI 结构变更 | 已两次重构收敛到当前形式 |
| 把 `listen` 从 `SocketAddr` 改成 `IpAddr`/`String` | 会丢失 clap 的启动前格式校验，见 §4 |
| 把 `target` 从 `String` 改成 `SocketAddr` | **会破坏域名支持**，见 §4 |
| 更换基础镜像 / 上游 registry | 内网环境约束，见 7.1 |
| 增加非必要依赖 | 与"轻量"定位冲突 |

---

## 10. 待办 / 已知缺口

若有能力，以下方向是明确有价值的（**做之前先与维护者确认优先级**）：

- [ ] **补充自动化测试**：目前零测试。优先补 `cli.rs` 的参数解析测试（`-l` 格式校验、
  `-t` 接受域名、非法输入被拒），再补 `proxy.rs` 的转发集成测试（可用 `tokio::net` 起临时 listener）。
- [ ] **CI**：无。建议加 GitHub Actions 跑 `cargo build` + `clippy` + `cargo test`。
- [ ] **多端口批量转发**：一次起多条规则（配置文件）。
- [ ] **UDP 支持**：当前仅 TCP。

> 已解决：早期 `--target 192.168.2.203:2000` 会拼出畸形地址 `192.168.2.203:2000:100` 的缺陷，
> 在改用 `-l` / `-t` 完整 `host:port` 形式后**自然消失**（不再有端口拼接逻辑）。

---

## 11. 术语

| 词 | 含义 |
|---|---|
| **listener** | 本机监听端（接收客户端连接） |
| **target** | 转发目标（`-t` 指定的完整 `host:port`） |
| **in / out** | 统计口径：`bytes_in` = 客户端→目标；`bytes_out` = 目标→客户端。**以 listener 侧为参照系**，不要按直觉反向理解 |
| **idle_timeout** | 单次 read 的空闲上限（非整个连接生命周期），0 表示禁用 |
