use anyhow::bail;
use bon::builder;
use clap::{Parser, Subcommand};
use clio::Input;
use comfy_table::{Table, presets};
use cron::Schedule;
use lmb::{
    DEFAULT_TIMEOUT, EXAMPLES, Error, Evaluation, GUIDES, LuaSource, PrintOptions, ScheduleOptions,
    Store,
};
use mlua::prelude::*;
use serde_json::json;
use serve::ServeOptions;
use std::{
    io::Read, net::SocketAddr, path::PathBuf, process::ExitCode, str::FromStr, time::Duration,
};
use termimad::MadSkin;
use tokio::{io, task::JoinSet};
use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

mod serve;

static VERSION: &str = env!("APP_VERSION");

/// lmb is a Lua function runner.
#[derive(Parser)]
#[command(about, author, version=VERSION)]
struct Cli {
    /// Allow environment variables to be used in the script.
    #[arg(long, env = "LMB_ALLOW_ENV", value_delimiter = ',')]
    allow_env: Option<Vec<Box<str>>>,

    /// Checks the syntax of the function before evaluation or serving,
    /// disabled by default for startup performance
    #[arg(long, env = "LMB_CHECK_SYNTAX")]
    check_syntax: bool,

    /// Debug mode
    #[arg(long, short = 'd', env = "DEBUG")]
    debug: bool,

    /// Enable JSON mode.
    /// When evaluating, output the solution in JSON format.
    /// When serving, always respond with the solution as a JSON value
    #[arg(long)]
    json: bool,

    /// Database lock timeout in milliseconds.
    #[arg(long, env = "LMB_BUSY_TIMEOUT_MS")]
    busy_timeout: Option<u64>,

    /// No color <https://no-color.org/>
    #[arg(long, env = "NO_COLOR")]
    no_color: bool,

    /// Migrate the store before startup.
    /// If the store path is not specified and the store is in-memory,
    /// it will be automatically migrated
    #[arg(long, env = "LMB_RUN_MIGRATIONS")]
    run_migrations: bool,

    /// Store path. By default, the store is in-memory,
    /// and changes will be lost when the program terminates.
    /// To persist values, a store path must be specified
    #[arg(long, env = "LMB_STORE_PATH")]
    store_path: Option<PathBuf>,

    /// Theme. Checkout `list-themes` for available themes
    #[arg(long, env = "LMB_THEME")]
    theme: Option<Box<str>>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check syntax of script
    Check {
        /// Script paths. Specify "-" or omit to load the script from standard input
        #[arg(long = "file", value_parser, default_value = "-")]
        files: Vec<Input>,
    },
    /// Evaluate a script file
    #[command(alias = "eval")]
    Evaluate {
        /// Script paths. Specify "-" or omit to load the script from standard input
        #[arg(long = "file", value_parser, default_value = "-")]
        files: Vec<Input>,
        /// Timeout in seconds
        #[arg(long, default_value_t = DEFAULT_TIMEOUT.as_secs())]
        timeout: u64,
    },
    /// Check out examples and evaluate or serve them
    #[command(subcommand)]
    Example(ExampleCommands),
    /// Guide commands
    #[command(subcommand)]
    Guide(GuideCommands),
    /// List available themes
    ListThemes,
    /// Schedule the script as a cron job
    Schedule {
        /// Exit immediately upon N number of errors. 0 to disable.
        #[arg(long, default_value_t = 1)]
        bail: usize,
        /// Cron
        #[arg(long)]
        cron: Box<str>,
        /// Run the script at startup even if the next execution is not due
        #[arg(long)]
        initial_run: bool,
        /// Script paths. Specify "-" or omit to load the script from standard input
        #[arg(long = "file", value_parser, default_value = "-")]
        files: Vec<Input>,
    },
    /// Handle HTTP requests with the script
    /// If more than one script is provided, note that the scripts will NOT be evaluated concurrently.
    /// Instead, the last script will be the main script to handle requests, and the rest will be treated as middlewares.
    /// For example: lmb serve --file m1.lua --file m2.lua --file main.lua
    /// 1. main.lua will handle HTTP requests
    /// 2. main.lua may call m2.lua with `next()`
    /// 3. m2.lua may call m1.lua with `next()`
    Serve {
        /// Bind the server to a specific host and port
        #[arg(long, default_value = "127.0.0.1:0")]
        bind: Box<str>,
        /// Script paths. Specify "-" or omit to load the script from standard input
        #[arg(long = "file", value_parser, default_value = "-")]
        files: Vec<Input>,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Store commands
    #[command(subcommand)]
    Store(StoreCommands),
}

#[derive(Parser)]
enum ExampleCommands {
    /// Print script of example
    Cat {
        /// Example name
        #[arg(long)]
        name: Box<str>,
    },
    /// Evaluate the example
    #[command(alias = "eval")]
    Evaluate {
        /// Example name
        #[arg(long)]
        name: Box<str>,
    },
    /// Handle HTTP requests with the example
    Serve {
        /// Bind the server to a specific host and port
        #[arg(long, default_value = "127.0.0.1:0")]
        bind: Box<str>,
        /// Example name
        #[arg(long)]
        name: Box<str>,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// List examples
    #[command(alias = "ls")]
    List,
}

#[derive(Parser)]
enum GuideCommands {
    /// Read a guide
    Cat {
        /// Name
        #[arg(long)]
        name: Box<str>,
    },
    /// List available guides
    List,
}

#[derive(Parser)]
enum StoreCommands {
    /// Delete a value
    Delete {
        /// Name
        #[arg(long)]
        name: Box<str>,
    },
    /// Get a value
    Get {
        /// Name
        #[arg(long)]
        name: Box<str>,
    },
    /// List values
    List,
    /// Migrate the store
    Migrate {
        /// Target version. Specify 0 to revert ALL migrations. Omit to migrate to the latest
        #[arg(long)]
        version: Option<usize>,
    },
    /// Insert or update a value
    Put {
        /// Name
        #[arg(long)]
        name: Box<str>,
        /// Consider value as plain string instead of JSON value
        #[arg(long)]
        plain: bool,
        /// Value, the content should be a valid JSON value e.g. true or "string" or 1
        #[arg(long, value_parser, default_value = "-")]
        value: Input,
    },
    /// Show current version
    Version,
}

fn do_check_syntax(source: &LuaSource) -> anyhow::Result<()> {
    if let Err(err) = source.check() {
        let mut buf = String::new();
        let err: Vec<&Error> = err.iter().collect();
        source.write_errors(&mut buf, err).call()?;
        bail!(buf.trim().to_owned().into_boxed_str());
    }
    Ok(())
}

fn read_script(input: &mut Input) -> anyhow::Result<LuaSource> {
    let path = input.path().to_string_lossy().into_owned().into_boxed_str();
    let mut script = String::new();
    input.read_to_string(&mut script)?;
    Ok(LuaSource::builder(script).name(path).build())
}

#[builder]
fn prepare_store(
    #[builder(required)] busy_timeout_ms: Option<u64>,
    run_migrations: bool,
    #[builder(required)] store_path: Option<PathBuf>,
) -> anyhow::Result<Store> {
    let store = if let Some(store_path) = store_path {
        Store::builder()
            .maybe_busy_timeout(busy_timeout_ms.map(Duration::from_millis))
            .path(&store_path)
            .run_migrations(run_migrations)
            .build()?
    } else {
        Store::default()
    };
    Ok(store)
}

async fn try_main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let default_directive = if cli.debug {
        Level::DEBUG.into()
    } else {
        Level::INFO.into()
    };
    let env_filter = EnvFilter::builder()
        .with_default_directive(default_directive)
        .from_env_lossy();
    let span_events = env_filter.max_level_hint().map_or_else(
        || FmtSpan::CLOSE,
        |l| {
            if l >= Level::DEBUG {
                FmtSpan::CLOSE
            } else {
                FmtSpan::NONE
            }
        },
    );
    tracing_subscriber::fmt()
        .with_ansi(!cli.no_color)
        .with_env_filter(env_filter)
        .with_span_events(span_events)
        .compact()
        .init();

    let print_options = PrintOptions::builder()
        .no_color(cli.no_color)
        .maybe_theme(cli.theme)
        .build();
    match cli.command {
        Commands::Check { files } => {
            let mut set = JoinSet::new();
            for mut file in files {
                set.spawn(async move {
                    let source = read_script(&mut file)?;
                    do_check_syntax(&source)?;
                    Ok::<(), anyhow::Error>(())
                });
            }
            while let Some(next) = set.join_next().await {
                next??;
            }
            Ok(())
        }
        Commands::Evaluate { files, timeout } => {
            let store = prepare_store()
                .busy_timeout_ms(cli.busy_timeout)
                .run_migrations(cli.run_migrations)
                .store_path(cli.store_path)
                .call()?;
            let mut set = JoinSet::new();
            for mut file in files {
                let store = store.clone();
                let allow_env = cli.allow_env.clone();
                set.spawn(async move {
                    let source = read_script(&mut file)?;
                    if cli.check_syntax {
                        do_check_syntax(&source)?;
                    }
                    let e = match Evaluation::builder(source.clone(), io::stdin())
                        .store(store.clone())
                        .timeout(Duration::from_secs(timeout))
                        .maybe_allowed_env_vars(allow_env)
                        .build()
                    {
                        Ok(e) => e,
                        Err(err) => {
                            eprint!("{err}");
                            return Err(err.into());
                        }
                    };
                    let mut buf = String::new();
                    match e.evaluate_async().call().await {
                        Ok(s) => {
                            s.write(&mut buf).json(cli.json).call()?;
                            print!("{buf}");
                            Ok::<(), anyhow::Error>(())
                        }
                        Err(err) => {
                            e.write_errors(&mut buf, vec![&err])?;
                            eprint!("{buf}");
                            Err(err.into())
                        }
                    }
                });
            }
            for result in set.join_all().await {
                result?;
            }
            Ok(())
        }
        Commands::Example(ExampleCommands::Cat { name }) => {
            let name = &*name;
            let Some(found) = EXAMPLES.iter().find(|e| e.name() == name) else {
                bail!("example with {name} not found");
            };
            let mut buf = String::new();
            found.source.write_script(&mut buf, &print_options)?;
            println!("{buf}");
            Ok(())
        }
        Commands::Example(ExampleCommands::Evaluate { name }) => {
            let name = &*name;
            let Some(found) = EXAMPLES.iter().find(|e| e.name() == name) else {
                bail!("example with {name} not found");
            };
            let script = found.source.script.trim();
            let store = prepare_store()
                .busy_timeout_ms(cli.busy_timeout)
                .run_migrations(cli.run_migrations)
                .store_path(cli.store_path)
                .call()?;
            let source = LuaSource::builder(script).name(name.into()).build();
            let e = Evaluation::builder(source, io::stdin())
                .store(store)
                .build()?;
            let mut buf = String::new();
            match e.evaluate().call() {
                Ok(s) => {
                    s.write(&mut buf).json(cli.json).call()?;
                    print!("{buf}");
                    Ok(())
                }
                Err(err) => {
                    e.write_errors(&mut buf, vec![&err])?;
                    eprint!("{buf}");
                    Err(err.into())
                }
            }
        }
        Commands::Example(ExampleCommands::List) => {
            let mut table = Table::new();
            table.load_preset(presets::NOTHING);
            table.set_header(["name", "description"]);
            for e in EXAMPLES.iter() {
                table.add_row([e.name(), &e.description]);
            }
            println!("{table}");
            Ok(())
        }
        Commands::Example(ExampleCommands::Serve {
            bind,
            name,
            timeout,
        }) => {
            let name = &*name;
            let Some(found) = EXAMPLES.iter().find(|e| e.name() == name) else {
                bail!("example with {name} not found");
            };
            let bind = bind.parse::<SocketAddr>()?;
            let timeout = timeout.map(Duration::from_secs);
            let options = ServeOptions::builder(bind, found.source.clone())
                .json(cli.json)
                .maybe_store_path(cli.store_path)
                .maybe_timeout(timeout)
                .run_migrations(cli.run_migrations)
                .build();
            serve::serve_file(&options).await?;
            Ok(())
        }
        Commands::Guide(GuideCommands::List) => {
            let mut table = Table::new();
            table.load_preset(presets::NOTHING);
            table.set_header(["name", "title"]);
            for guide in GUIDES.iter() {
                table.add_row([&guide.name, &guide.title]);
            }
            print!("{table}");
            Ok(())
        }
        Commands::Guide(GuideCommands::Cat { name }) => {
            let Some(guide) = GUIDES.iter().find(|g| name == g.name) else {
                bail!("guide with {name} not found");
            };
            let skin = MadSkin::default();
            println!("{}", skin.term_text(&guide.content));
            Ok(())
        }
        Commands::ListThemes => {
            let p = bat::PrettyPrinter::new();
            for t in p.themes() {
                println!("{t}");
            }
            Ok(())
        }
        Commands::Schedule {
            bail,
            cron,
            files,
            initial_run,
        } => {
            let store = prepare_store()
                .busy_timeout_ms(cli.busy_timeout)
                .run_migrations(cli.run_migrations)
                .store_path(cli.store_path)
                .call()?;
            let schedule = Schedule::from_str(&cron)?;
            let mut set = JoinSet::new();
            for mut file in files {
                let store = store.clone();
                let schedule = schedule.clone();
                set.spawn(async move {
                    let source = read_script(&mut file)?;
                    let options = ScheduleOptions::builder()
                        .bail(bail)
                        .initial_run(initial_run)
                        .schedule(schedule.clone())
                        .build();
                    let e = Evaluation::builder(source, io::stdin())
                        .store(store.clone())
                        .build()?;
                    e.schedule_async(&options).await;
                    Ok::<(), anyhow::Error>(())
                });
            }
            for result in set.join_all().await {
                result?;
            }
            Ok(())
        }
        Commands::Serve {
            bind,
            mut files,
            timeout,
        } => {
            files.reverse();

            let mut sources = vec![];
            for file in &mut files {
                sources.push(read_script(file)?);
            }

            let mut first_source = sources[0].clone();
            let mut head = &mut first_source;
            for source in &sources[1..] {
                head.next = Some(Box::new(source.clone()));
                head = head.next.as_mut().expect("should not be empty");
            }

            let timeout = timeout.map(Duration::from_secs);
            let bind = bind.parse::<SocketAddr>()?;
            let options = ServeOptions::builder(bind, first_source)
                .json(cli.json)
                .maybe_store_path(cli.store_path)
                .maybe_timeout(timeout)
                .run_migrations(cli.run_migrations)
                .build();
            serve::serve_file(&options).await?;

            Ok(())
        }
        Commands::Store(c) => {
            let Some(store_path) = cli.store_path else {
                bail!("store_path is required");
            };
            let store = Store::builder()
                .path(&store_path)
                .run_migrations(cli.run_migrations)
                .build()?;
            match c {
                StoreCommands::Delete { name } => {
                    let affected = store.delete(name)?;
                    print!("{affected}");
                    Ok(())
                }
                StoreCommands::Get { name } => {
                    let value = store.get(name)?;
                    let value = serde_json::to_string(&value)?;
                    print!("{value}");
                    Ok(())
                }
                StoreCommands::List => {
                    let metadata_rows = store.list()?;
                    let mut table = Table::new();
                    table.load_preset(presets::NOTHING);
                    table.set_header(["name", "type", "size", "created at", "updated at"]);
                    for m in metadata_rows.iter() {
                        table.add_row([
                            &m.name,
                            &m.type_hint,
                            &m.size.to_string().into_boxed_str(),
                            &m.created_at.to_rfc3339().into_boxed_str(),
                            &m.updated_at.to_rfc3339().into_boxed_str(),
                        ]);
                    }
                    println!("{table}");
                    Ok(())
                }
                StoreCommands::Migrate { version } => {
                    store.migrate(version)?;
                    Ok(())
                }
                StoreCommands::Put {
                    name,
                    plain,
                    mut value,
                } => {
                    let mut buf = String::new();
                    value.read_to_string(&mut buf)?;
                    let value = if plain {
                        json!(buf)
                    } else {
                        serde_json::from_str(&buf)?
                    };
                    let affected = store.put(name, &value)?;
                    print!("{affected}");
                    Ok(())
                }
                StoreCommands::Version => {
                    let version = store.current_version()?;
                    println!("{version}");
                    Ok(())
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(e) = try_main().await {
        match e.downcast_ref::<Error>() {
            // the following errors are handled, do nothing
            Some(&Error::Lua(LuaError::RuntimeError(_) | LuaError::SyntaxError { .. })) => {}
            _ => eprintln!("{e}"),
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
