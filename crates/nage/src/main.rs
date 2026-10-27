//! The `nage` binary: serve, health, init, user, check, backup, export, migrate.
//! Milestone 1 slice 2 adds `serve` and `health`. Only this binary reads the environment;
//! `nage-web` is handed values (decision 42).
//!
//! Exit codes for every subcommand (decision 45): 0 success, 2 usage and configuration
//! errors, 1 runtime failures.

mod cli;
mod config;
mod health;
mod serve;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};

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
