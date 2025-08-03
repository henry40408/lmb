use std::{path::PathBuf, time::Duration};

use bat::PrettyPrinter;
use clap::{Parser, Subcommand};
use lmb::{
    LmbError, Runner,
    error::{ErrorReport, build_report, render_report},
};
use no_color::is_no_color;
use serde_json::Value;
use tokio::io::{self, AsyncWriteExt as _};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Opts {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Evaluate a file
    Eval {
        /// Path to the file to evaluate
        #[clap(long, value_parser)]
        file: PathBuf,
        /// Optional state to pass to the Lua script
        #[clap(long, value_parser)]
        state: Option<Value>,
        /// Timeout. Default is 30 seconds, set to 0 for no timeout
        #[clap(long)]
        timeout_ms: Option<u64>,
    },
}

async fn try_main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    match opts.command {
        Command::Eval {
            file,
            state,
            timeout_ms,
        } => {
            let reader = io::stdin();
            let path = file.as_path();

            let timeout = match timeout_ms {
                None => Some(Duration::from_millis(30)),
                Some(0) => None,
                Some(t) => Some(Duration::from_millis(t)),
            };
            let runner = match Runner::builder(path, reader).maybe_timeout(timeout).build() {
                Ok(runner) => runner,
                Err(e) => {
                    if let LmbError::ExpectedLuaFunction { .. } = e {
                        PrettyPrinter::new()
                            .input_from_bytes(include_bytes!("expect_lua_function.md"))
                            .colored_output(!is_no_color())
                            .language("markdown")
                            .print()?;
                    } else {
                        match build_report(path, &e)? {
                            ErrorReport::Report(report) => {
                                let mut s = String::new();
                                render_report(&mut s, &report);
                                io::stderr().write_all(s.as_bytes()).await?;
                            }
                            ErrorReport::String(msg) => eprintln!("{msg}"),
                        }
                    }
                    return Err(e.into());
                }
            };

            let result = runner.invoke().maybe_state(state).call().await?;
            match result.result {
                Ok(value) => {
                    if let Value::String(s) = &value {
                        println!("{s}");
                    } else {
                        println!("{value}");
                    }
                }
                Err(e) => {
                    match build_report(path, &e)? {
                        ErrorReport::Report(report) => {
                            let mut s = String::new();
                            render_report(&mut s, &report);
                            io::stderr().write_all(s.as_bytes()).await?;
                        }
                        ErrorReport::String(msg) => eprintln!("{msg}"),
                    }
                    return Err(e.into());
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = try_main().await {
        eprintln!("Error: {e}");

        #[allow(clippy::exit)]
        std::process::exit(101);
    }
}
