//! The `nage` binary: serve, health, init, user, check, backup, export, migrate.
//! Milestone 1 slice 2 added `serve` and `health`, milestone 2 slice 3 `check`, milestone 2
//! slice 5 `init`, `user add` and `user reset-password`. Only this binary reads the
//! environment; `nage-web` is handed values (decision 42).
//!
//! Exit codes for every subcommand (decision 45): 0 success, 2 usage and configuration
//! errors, 1 runtime failures and refusals.

mod check;
mod cli;
mod config;
mod database;
mod health;
mod init;
mod messages;
mod prompt;
mod serve;
mod user;

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{
    Cli, Command, InitArgs, UserAddArgs, UserArgs, UserCommand, UserResetPasswordArgs,
};
use crate::config::Env;
use crate::messages::{Done, Stop};
use crate::prompt::{Source, Tty};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let env = |name: &str| -> Option<OsString> { std::env::var_os(name) };
    match cli.command {
        Command::Serve(args) => {
            let config = match config::resolve_serve(args, &env) {
                Ok(config) => config,
                Err(error) => return config_error(&error),
            };
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => return runtime_error(&format!("cannot start the runtime: {error}")),
            };
            match runtime.block_on(serve::run(config)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => runtime_error(&error),
            }
        }
        Command::Health(args) => {
            let bind = match config::resolve_health_bind(args.bind, &env) {
                Ok(bind) => bind,
                Err(error) => return config_error(&error),
            };
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => return runtime_error(&format!("cannot start the runtime: {error}")),
            };
            runtime.block_on(health::run(bind))
        }
        Command::Init(InitArgs {
            data_dir,
            book_name,
            currency,
            username,
            display_name,
            no_starter_chart,
            password_stdin,
        }) => command(data_dir, password_stdin, &env, async |data_dir, source| {
            let init = init::Init {
                data_dir,
                book_name,
                currency,
                username,
                display_name,
                starter_chart: !no_starter_chart,
            };
            init::run(init, source).await
        }),
        Command::User(UserArgs {
            command:
                UserCommand::Add(UserAddArgs {
                    data_dir,
                    by,
                    username,
                    display_name,
                    password_stdin,
                }),
        }) => command(data_dir, password_stdin, &env, async |data_dir, source| {
            let add = user::Add {
                data_dir,
                by,
                username,
                display_name,
            };
            user::add(add, source).await
        }),
        Command::User(UserArgs {
            command:
                UserCommand::ResetPassword(UserResetPasswordArgs {
                    data_dir,
                    by,
                    username,
                    password_stdin,
                }),
        }) => command(data_dir, password_stdin, &env, async |data_dir, source| {
            let reset = user::Reset {
                data_dir,
                by,
                username,
            };
            user::reset_password(reset, source).await
        }),
        Command::Check(args) => {
            let data_dir = match config::resolve_data_dir(args.data_dir, &env) {
                Ok(data_dir) => data_dir,
                Err(error) => return config_error(&error),
            };
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => return runtime_error(&format!("cannot start the runtime: {error}")),
            };
            runtime.block_on(check::run(data_dir))
        }
    }
}

/// Runs `init`, `user add` or `user reset-password` (decision 113): resolves the data
/// directory as `check` does, hands `run` the place the password comes from (standard input
/// with `--password-stdin`, read only when the command gets as far as the password; the
/// terminal otherwise) and prints what the command ends with: its result on standard output
/// and exit 0, or one `nage: ` line on standard error and the exit code of what stopped it.
/// Nothing else is printed, on either stream.
fn command(
    data_dir: Option<PathBuf>,
    password_stdin: bool,
    env: Env,
    run: impl AsyncFnOnce(PathBuf, &mut Source<'_>) -> Result<Done, Stop>,
) -> ExitCode {
    let data_dir = match config::resolve_data_dir(data_dir, env) {
        Ok(data_dir) => data_dir,
        Err(error) => return config_error(&error),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return runtime_error(&format!("cannot start the runtime: {error}")),
    };
    let mut stdin = io::stdin().lock();
    let mut terminal = Tty;
    let mut source = if password_stdin {
        Source::Stdin(&mut stdin)
    } else {
        Source::Terminal(&mut terminal)
    };
    match runtime.block_on(run(data_dir, &mut source)) {
        Ok(done) => {
            println!("{done}");
            ExitCode::SUCCESS
        }
        Err(stop) => {
            eprintln!("nage: {stop}");
            ExitCode::from(stop.exit_code())
        }
    }
}

/// A configuration error: one `nage: ` line, exit 2.
fn config_error(error: &config::ConfigError) -> ExitCode {
    eprintln!("nage: {error}");
    ExitCode::from(2)
}

/// A runtime failure: one `nage: ` line, exit 1.
fn runtime_error(error: &dyn std::fmt::Display) -> ExitCode {
    eprintln!("nage: {error}");
    ExitCode::FAILURE
}
