use std::path::PathBuf;

use clap::{Parser, Subcommand};
use lmb::Runner;
use serde_json::Value;
use tokio::{fs, io};

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

async fn try_main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    match opts.command {
        Command::Eval { file } => {
            let source = fs::read_to_string(&file).await?;
            let reader = io::stdin();
            let runner = Runner::builder(source, reader)
                .name(file.as_path().to_string_lossy())
                .build()?;
            let result = runner.invoke().call().await?;
            let value = result.result?;
            if let Value::String(s) = &value {
                println!("{s}");
            } else {
                println!("{value}");
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = try_main().await {
        eprintln!("{e}");
    }
}
