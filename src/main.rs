#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_mut)]
#![allow(unused_variables)]
use clap::Parser;
use nix::{
    pty::{self, openpty, Winsize},
    sys::termios,
};
use std::{
    env,
    marker::Unpin,
    os::unix::{
        io::{AsRawFd, FromRawFd},
        process::CommandExt,
    },
    process::{self, Stdio},
    time::Duration,
};
use tokio::{
    fs::File,
    io::{duplex, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    join,
    process::Command,
    sync::mpsc,
    time::sleep,
};

use telwrap::server::{ReverseCallback, TelnetServer};

/// Wrap a program as a telnet server
#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// The executable to wrap
    program: String,

    /// Program arguments
    program_args: Vec<String>,
}

#[tokio::main]
async fn main() {
    const CHANNEL_WIDTH: usize = 64;
    let server = TelnetServer::new("127.0.0.1:9000").await;
    let args = Args::parse();
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
                let stdin = openpty(Some(&winsize), Some(&stdin_termios)).unwrap();
                let stdout = openpty(Some(&winsize), Some(&stdout_termios)).unwrap();
                let mut child = Command::new(&args.program)
                    .args(&args.program_args)
                    .stdin(unsafe { Stdio::from_raw_fd(stdin.master) })
                    .stdout(unsafe { Stdio::from_raw_fd(stdout.master) })
                    //.stdout(Stdio::piped())
                    .status();
                let stdin = unsafe { File::from_raw_fd(stdin.slave) };
                let stdout = unsafe { File::from_raw_fd(stdout.slave) };
                //let stdout = child.stdout.take().unwrap();

                callback.call(stdin, stdout).await;
                child.await.expect("!");
                //child.wait().await.unwrap();
            }
        }))
        .await;
}
