//! The `nage` binary: serve, health, init, user, check, migrate, backup, export.
//! Milestone 1 slice 2 added `serve` and `health`, milestone 2 slice 3 `check`, milestone 2
//! slice 5 `init`, `user add` and `user reset-password`, milestone 2 slice 8 `migrate` and the
//! form of everything the commands print (decision 157). Only this binary reads the
//! environment; `nage-web` is handed values (decision 42).
//!
//! Exit codes for every subcommand (decision 45): 0 success, 2 usage and configuration
//! errors, 1 runtime failures and refusals. Results go to standard output, problems to standard
//! error (assumption A1 of PLAN M2 slice 8).

mod check;
mod cli;
mod config;
mod database;
mod health;
mod init;
mod log;
mod messages;
mod migrate;
mod mistyped;
mod prompt;
mod serve;
mod shape;
mod user;

use std::ffi::OsString;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ColorChoice, CommandFactory as _, FromArgMatches as _};

use crate::cli::{
    Cli, Command, InitArgs, MigrateArgs, UserAddArgs, UserArgs, UserCommand, UserResetPasswordArgs,
};
use crate::config::Env;
use crate::messages::{Done, Stop, Task};
use crate::prompt::{Source, Tty};
use crate::shape::{Doing, Paint, Refused, Screen};

fn main() -> ExitCode {
    let env = |name: &str| -> Option<OsString> { std::env::var_os(name) };
    let no_color = shape::no_color(env("NO_COLOR").as_deref());
    let screen = Screen {
        out: Paint::new(io::stdout().is_terminal(), no_color),
        err: Paint::new(io::stderr().is_terminal(), no_color),
    };
    let typed: Vec<OsString> = std::env::args_os().collect();
    // The library's help and version follow the same rule as Nage's labels, set here by Nage
    // (decision 158 as the pause amended it): colour exactly when standard output is a terminal and
    // NO_COLOR is unset, whatever CLICOLOR_FORCE or TERM say.
    let color = if screen.out.colours() {
        ColorChoice::Always
    } else {
        ColorChoice::Never
    };
    let mut parser = Cli::command().color(color);
    let cli = match parser
        .try_get_matches_from_mut(&typed)
        .and_then(|matches| Cli::from_arg_matches(&matches))
    {
        Ok(cli) => cli,
        Err(error) => return mistyped::stop(&error.format(&mut parser), &typed, screen),
    };
    match cli.command {
        Command::Serve(args) => {
            let config = match config::resolve_serve(args, &env) {
                Ok(config) => config,
                Err(error) => return screen.refused(&error.refused(Doing::Serve), 2),
            };
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => return runtime_error(Doing::Serve, &error, screen),
            };
            match runtime.block_on(serve::run(config)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => serve::failed(&error, screen),
            }
        }
        Command::Health(args) => {
            let bind = match config::resolve_health_bind(args.bind, &env) {
                Ok(bind) => bind,
                Err(error) => return screen.refused(&error.refused(Doing::Health), 2),
            };
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => return runtime_error(Doing::Health, &error, screen),
            };
            runtime.block_on(health::run(bind, screen))
        }
        Command::Init(InitArgs {
            data_dir,
            book_name,
            currency,
            username,
            display_name,
            no_starter_chart,
            password_stdin,
        }) => command(
            Task::SetUp,
            data_dir,
            password_stdin,
            &env,
            screen,
            async |data_dir, source| {
                let init = init::Init {
                    data_dir,
                    book_name,
                    currency,
                    username,
                    display_name,
                    starter_chart: !no_starter_chart,
                };
                init::run(init, source).await
            },
        ),
        Command::User(UserArgs {
            command:
                UserCommand::Add(UserAddArgs {
                    data_dir,
                    by,
                    username,
                    display_name,
                    password_stdin,
                }),
        }) => command(
            Task::AddUser,
            data_dir,
            password_stdin,
            &env,
            screen,
            async |data_dir, source| {
                let add = user::Add {
                    data_dir,
                    by,
                    username,
                    display_name,
                };
                user::add(add, source).await
            },
        ),
        Command::User(UserArgs {
            command:
                UserCommand::ResetPassword(UserResetPasswordArgs {
                    data_dir,
                    by,
                    username,
                    password_stdin,
                }),
        }) => command(
            Task::ResetPassword,
            data_dir,
            password_stdin,
            &env,
            screen,
            async |data_dir, source| {
                let reset = user::Reset {
                    data_dir,
                    by,
                    username,
                };
                user::reset_password(reset, source).await
            },
        ),
        Command::Check(args) => {
            let data_dir = match config::resolve_data_dir(args.data_dir, &env) {
                Ok(data_dir) => data_dir,
                Err(error) => return screen.refused(&error.refused(Doing::Check), 2),
            };
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => return runtime_error(Doing::Check, &error, screen),
            };
            runtime.block_on(check::run(data_dir, screen))
        }
        Command::Migrate(MigrateArgs { data_dir, dry_run }) => {
            // Known before the data folder is resolved, so that a dry run's wrong setting has the
            // dry run's own first line.
            let doing = if dry_run {
                Doing::DryRun
            } else {
                Doing::Migrate
            };
            let data_dir = match config::resolve_data_dir(data_dir, &env) {
                Ok(data_dir) => data_dir,
                Err(error) => return screen.refused(&error.refused(doing), 2),
            };
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => return runtime_error(doing, &error, screen),
            };
            if dry_run {
                runtime.block_on(migrate::dry_run(data_dir, screen))
            } else {
                runtime.block_on(migrate::run(data_dir, screen))
            }
        }
    }
}

/// Runs `init`, `user add` or `user reset-password` (decisions 113 and 157): resolves the data
/// directory as `check` does, hands `run` the place the password comes from (standard input
/// with `--password-stdin`, read only when the command gets as far as the password; the
/// terminal otherwise) and prints what the command ends with: its result on standard output
/// and exit 0, or three labelled lines on standard error and the exit code of what stopped it.
/// Nothing else is printed, on either stream.
fn command(
    task: Task,
    data_dir: Option<PathBuf>,
    password_stdin: bool,
    env: Env,
    screen: Screen,
    run: impl AsyncFnOnce(PathBuf, &mut Source<'_>) -> Result<Done, Stop>,
) -> ExitCode {
    let data_dir = match config::resolve_data_dir(data_dir, env) {
        Ok(data_dir) => data_dir,
        Err(error) => return screen.refused(&error.refused(task.doing()), 2),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return runtime_error(task.doing(), &error, screen),
    };
    let mut stdin = io::stdin().lock();
    let mut terminal = Tty;
    let mut source = if password_stdin {
        Source::Stdin(&mut stdin)
    } else {
        Source::Terminal(&mut terminal)
    };
    match runtime.block_on(run(data_dir, &mut source)) {
        Ok(done) => screen.finished(&done.finished()),
        Err(stop) => screen.refused(&stop.refused(task), stop.exit_code()),
    }
}

/// The runtime could not be started: three lines, exit 1 (decision 159).
fn runtime_error(doing: Doing, error: &io::Error, screen: Screen) -> ExitCode {
    let refused = Refused::new(
        doing.not_done(),
        format!(
            "The program could not start its runtime ({}).",
            shape::system_words(error)
        ),
        format!("Run the command again; {}", shape::IF_IT_FAILS_AGAIN),
    );
    screen.refused(&refused, 1)
}
