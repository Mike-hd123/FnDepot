//! waliapi-web：headless 服务模式（无桌面窗口），供 Docker / 无显示 Linux 部署。
//!
//! 用法：
//!   waliapi-web [--host 0.0.0.0] [--port 8777] [--data-dir /data]   （默认即启动服务）
//!   waliapi-web start [...同上参数...]
//!
//! 环境变量：WALIAPI_SERVER_HOST / WALIAPI_SERVER_PORT / WALIAPI_DATA_DIR / XDG_DATA_HOME

use waliapi_lib::web_server::{resolve_data_dir, run, WebServerConfig};
use tracing_subscriber::prelude::*;

fn print_usage() {
    println!(
        "waliapi-web — WaLiAPI headless 服务模式（LLM 网关 + Web 管理面板）

用法:
  waliapi-web [start] [--host <地址>] [--port <端口>] [--data-dir <目录>]

说明:
  不带任何参数直接启动服务（start 为可选子命令，语义相同）。

选项:
  --host       监听地址（默认读取 WALIAPI_SERVER_HOST 或设置，缺省 127.0.0.1）
  --port       监听端口（默认读取 WALIAPI_SERVER_PORT 或设置，缺省 8777）
  --data-dir   数据目录（默认读取 WALIAPI_DATA_DIR / XDG_DATA_HOME，再缺省为平台应用数据目录）
  -h, --help   显示帮助
"
    );
}

fn parse_args(args: &[String]) -> Result<WebServerConfig, String> {
    let mut host = None;
    let mut port = None;
    let mut data_dir = None;
    let mut i = 0;
    while i < args.len() {
        let mut value_of = |i: &mut usize, name: &str| -> Result<String, String> {
            *i += 1;
            args.get(*i)
                .cloned()
                .ok_or_else(|| format!("{name} 缺少参数值"))
        };
        match args[i].as_str() {
            "--host" => host = Some(value_of(&mut i, "--host")?),
            "--port" => {
                let raw = value_of(&mut i, "--port")?;
                port = Some(
                    raw.trim()
                        .parse::<u16>()
                        .map_err(|_| format!("--port 无效: {raw}"))?,
                );
            }
            "--data-dir" => data_dir = Some(value_of(&mut i, "--data-dir")?),
            other => return Err(format!("未知参数: {other}")),
        }
        i += 1;
    }
    Ok(WebServerConfig {
        host,
        port,
        data_dir: resolve_data_dir(data_dir),
    })
}

#[tokio::main]
async fn main() {
    // 日志目录：优先数据目录下（容器内 /data/logs，waliapi 用户有写权限），
    // 回退到可执行文件同级 logs/。
    let data_log_dir = std::env::var("WALIAPI_DATA_DIR")
        .ok()
        .map(|d| std::path::PathBuf::from(d.trim()).join("logs"));
    let exe_log_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|parent| parent.join("logs")))
        .unwrap_or_else(|| std::path::PathBuf::from("logs"));
    let log_dir = data_log_dir.unwrap_or_else(|| exe_log_dir.clone());
    std::fs::create_dir_all(&log_dir).ok();

    // 按天滚动日志文件（如 waliapi.log.2026-08-25），最多保留 7 个文件
    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("waliapi.log")
        .max_log_files(7)
        .build(&log_dir)
        .ok();

    // 同时输出到 stdout（docker logs 可见）和日志文件。
    // 文件写入失败不影响 stdout，保证容器日志始终可见。
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout);
    let file_layer = file_appender.map(|w| {
        tracing_subscriber::fmt::layer().with_writer(w)
    });
    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    // 帮助优先于参数路由：-h/--help（含 start 子命令后）打印用法并正常退出
    let first = args.first().map(String::as_str);
    let help_requested = matches!(first, Some("-h") | Some("--help"))
        || (first == Some("start")
            && matches!(args.get(1).map(String::as_str), Some("-h") | Some("--help")));
    if help_requested {
        print_usage();
        return;
    }
    // 不带参数、直接带选项（waliapi-web --port 9000）、或显式 start 子命令，均启动服务
    let start_args: Option<&[String]> = match first {
        None => Some(&[]),
        Some("start") => Some(&args[1..]),
        Some(flag) if flag.starts_with("--") => Some(&args[..]),
        _ => None,
    };
    match start_args {
        Some(rest) => {
            let cfg = match parse_args(rest) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("参数错误: {e}\n");
                    print_usage();
                    std::process::exit(2);
                }
            };
            tracing::info!("数据目录: {}", cfg.data_dir.display());
            if let Err(e) = run(cfg).await {
                eprintln!("服务异常退出: {e}");
                std::process::exit(1);
            }
        }
        None => {
            // 帮助已在上方拦截；到这里的只会是非选项的未知子命令
            let other = first.expect("start_args 为 None 时必存在首参数");
            eprintln!("未知命令: {other}\n");
            print_usage();
            std::process::exit(2);
        }
    }
}
