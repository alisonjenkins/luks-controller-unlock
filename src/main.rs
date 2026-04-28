use clap::{Parser, Subcommand};
use std::process::ExitCode;
use tracing::error;

mod agent;
mod enroll;
mod error;
mod input;
mod keyscript;
mod pin;
mod selftest;
mod ui;

use error::Result;

#[derive(Parser, Debug)]
#[command(name = "luks-controller-unlock", version, about, long_about = None)]
struct Cli {
    /// Increase logging verbosity. Repeat for more.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Enroll a new controller-PIN keyslot on a LUKS2 device.
    Enroll(enroll::Args),
    /// Run as a systemd ask-password agent inside initrd (default for systemd initrds).
    Agent(agent::Args),
    /// Run as a crypttab keyscript (Debian/Ubuntu initramfs-tools).
    Keyscript(keyscript::Args),
    /// Verify host configuration is suitable for installation.
    Selftest,
    /// Render the unlock UI on the current DRM master without serving a request.
    TestUi(ui::TestArgs),
    /// Dump canonical button events from connected controllers to stderr.
    TestInput,
}

fn install_tracing(verbose: u8) {
    use tracing_subscriber::{EnvFilter, fmt};

    let env = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(match verbose {
            0 => "info",
            1 => "debug",
            _ => "trace",
        })
    });

    let _ = fmt()
        .with_env_filter(env)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}

fn run(cli: Cli) -> Result<()> {
    install_tracing(cli.verbose);
    match cli.command {
        Command::Enroll(args) => enroll::run(args),
        Command::Agent(args) => agent::run(args),
        Command::Keyscript(args) => keyscript::run(args),
        Command::Selftest => selftest::run(),
        Command::TestUi(args) => ui::run_test(args),
        Command::TestInput => input::run_test(),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("{e}");
            ExitCode::FAILURE
        }
    }
}
