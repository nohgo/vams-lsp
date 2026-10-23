//! Stdio entry adapted from Veridian's Tower LSP service startup.
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
use tower_lsp::{LspService, Server};
use vams_lsp::{
    project::{self, Project},
    server::Backend,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut root = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("vams-lsp [--stdio] [--root DIR]\nvams-lsp --print-root FILE\nvams-lsp --check-project PATH\n\nOffline Verilog-A/AMS language server. Diagnostics never invoke Cadence or formatters.");
                return;
            }
            "--version" | "-V" => {
                println!("vams-lsp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--stdio" => {}
            "--root" => {
                root = Some(PathBuf::from(
                    args.next()
                        .unwrap_or_else(|| usage("--root needs a directory")),
                ));
            }
            "--print-root" | "--check-project" => {
                let path =
                    PathBuf::from(args.next().unwrap_or_else(|| usage("a path is required")));
                if args.next().is_some() {
                    usage("unexpected arguments");
                }
                let (root, reason) = project::find_root(&path);
                if arg == "--print-root" {
                    println!("{}", root.display());
                    return;
                }
                let scan = Project::new(root, reason).scan(Default::default());
                let mut info = scan.project.info();
                info["indexed_files"] = scan.files.len().into();
                info["warnings"] = serde_json::json!(scan.warnings);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&info).expect("JSON value")
                );
                if scan.project.config_error.is_some() {
                    std::process::exit(1);
                }
                return;
            }
            _ => usage(&format!("unknown argument: {arg}")),
        }
    }
    let runtime = tokio::runtime::Runtime::new().expect("Tokio runtime");
    let graceful = runtime.block_on(async move {
        let stopped = Arc::new(AtomicBool::new(false));
        let exit = Arc::new(tokio::sync::Notify::new());
        let (service, messages) =
            LspService::new(|client| Backend::new(client, root, stopped.clone()));
        let service = ExitService {
            inner: service,
            exit: exit.clone(),
        };
        // Tower LSP otherwise waits for another input frame after exit. Observe
        // that notification explicitly, even when the client keeps stdin open.
        tokio::select! {
            _=Server::new(tokio::io::stdin(),tokio::io::stdout(),messages).serve(service)=>true,
            _=exit.notified()=>stopped.load(Ordering::Relaxed),
        }
    });
    // Tokio's blocking stdin read cannot be cancelled; do not wait indefinitely
    // for it during runtime teardown after the protocol's exit notification.
    runtime.shutdown_timeout(std::time::Duration::from_millis(100));
    if !graceful {
        std::process::exit(1);
    }
}
fn usage(message: &str) -> ! {
    eprintln!("vams-lsp: {message}; see --help");
    std::process::exit(2)
}

struct ExitService<S> {
    inner: S,
    exit: Arc<tokio::sync::Notify>,
}
impl<S: tower::Service<tower_lsp::jsonrpc::Request>> tower::Service<tower_lsp::jsonrpc::Request>
    for ExitService<S>
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }
    fn call(&mut self, request: tower_lsp::jsonrpc::Request) -> Self::Future {
        if request.method() == "exit" {
            self.exit.notify_one();
        }
        self.inner.call(request)
    }
}
