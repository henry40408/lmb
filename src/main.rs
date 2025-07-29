use std::path::PathBuf;

use clap::{Parser, Subcommand};
use lmb::{
    Runner,
    error::{ErrorReport, build_report, render_report},
};
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
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    match opts.command {
        Command::Eval { file } => {
            let reader = io::stdin();
            let path = file.as_path();

            let runner = match Runner::builder(path, reader).build() {
                Ok(runner) => runner,
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
            };

            let result = runner.invoke().call().await?;
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
