use std::{io::Read, time::Duration};

use anyhow::bail;
use bat::PrettyPrinter;
use clap::{Parser, Subcommand};
use clio::Input;
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
        file: Input,
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
            mut file,
            state,
            timeout_ms,
        } => {
            let reader = io::stdin();
            let source = if file.is_local() {
                None
            } else if file.is_std() {
                let mut buf = String::new();
                file.read_to_string(&mut buf)?;
                Some(buf)
            } else {
                bail!("Expected a local file or a stdin input, but got: {file}");
            };
            let timeout = match timeout_ms {
                None => Some(Duration::from_millis(30)),
                Some(0) => None,
                Some(t) => Some(Duration::from_millis(t)),
            };
            let runner = if let Some(source) = &source {
                Runner::builder(source, reader)
                    .maybe_timeout(timeout)
                    .build()
            } else {
                Runner::builder(file.path().path(), reader)
                    .maybe_timeout(timeout)
                    .build()
            };
            let runner = match runner {
                Ok(runner) => runner,
                Err(e) => {
                    if let LmbError::ExpectedLuaFunction { .. } = e {
                        PrettyPrinter::new()
                            .input_from_bytes(include_bytes!("expect_lua_function.md"))
                            .colored_output(!is_no_color())
                            .language("markdown")
                            .print()?;
                    } else {
                        let source = if let Some(source) = &source {
                            build_report(source, &e)?
                        } else {
                            build_report(file.path().path(), &e)?
                        };
                        match source {
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
                    let report = if let Some(source) = &source {
                        build_report(source, &e)?
                    } else {
                        build_report(file.path().path(), &e)?
                    };
                    match report {
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
