use clap::Parser;
use nix::{
    pty::{openpty, Winsize},
    sys::termios,
    unistd::dup,
};
use std::{
    os::unix::io::{AsRawFd, FromRawFd},
    process::Stdio,
};
use tokio::{fs::File, process::Command};

use telwrap::server::{ReverseCallback, TelnetServer};

/// Wrap a program as a telnet server
#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// The host IP address.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// The Port
    #[arg(short, long, default_value = "23")]
    port: u16,
    /// The executable to wrap
    program: String,
    /// Program arguments
    program_args: Vec<String>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let host = format!("{}:{}", args.host, args.port);
    let server = TelnetServer::new(&host).await;
    println!("Listening on {host}");
    server
        .listen(Box::new(move |callback: ReverseCallback| {
            let args = args.clone();
            async move {
                let winsize = Winsize {
                    ws_col: 55,
                    ws_row: 30,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                let stdin_termios = termios::tcgetattr(std::io::stdin().as_raw_fd()).unwrap();
                let stdout_termios = termios::tcgetattr(std::io::stdout().as_raw_fd()).unwrap();
                assert_eq!(stdin_termios, stdout_termios);
                let pty = openpty(Some(&winsize), Some(&stdout_termios)).unwrap();
                let child = Command::new(&args.program)
                    .args(&args.program_args)
                    .stdin(unsafe { Stdio::from_raw_fd(dup(pty.slave).unwrap()) })
                    .stdout(unsafe { Stdio::from_raw_fd(dup(pty.slave).unwrap()) })
                    .stderr(unsafe { Stdio::from_raw_fd(pty.slave) })
                    .status();
                let stdin = unsafe { File::from_raw_fd(dup(pty.master).unwrap()) };
                let stdout = unsafe { File::from_raw_fd(pty.master) };

                callback.call(stdin, stdout).await;
                child.await.expect("!");
            }
        }))
        .await;
}
