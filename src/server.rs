use crate::telnet::TelnetStream;
use std::{future::Future, marker::Unpin};
use telly::{TelnetEvent, TelnetOption};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    select,
};
use tokio_stream::StreamExt;

const READ_BUFFER_SIZE: usize = 256;

pub trait ReadableWritable: AsyncRead + AsyncWrite + Unpin {}

pub struct TelnetServer {
    listener: TcpListener,
}

pub struct ReverseCallback {
    tcp_stream: TcpStream,
}

impl ReverseCallback {
    fn new(tcp_stream: TcpStream) -> Self {
        Self { tcp_stream }
    }
    pub async fn call(self, stdin: impl AsyncWrite + Unpin, stdout: impl AsyncRead + Unpin) {
        handle_client(self.tcp_stream, stdin, stdout).await;
    }
}

impl TelnetServer {
    pub async fn new(host: &str) -> Self {
        Self {
            listener: TcpListener::bind(host).await.unwrap(),
        }
    }

    pub async fn listen<Fut>(
        &self,
        //todo - think of a better name than porky
        porky: Box<dyn Fn(ReverseCallback) -> Fut>,
    ) where
        Fut: Future<Output = ()>,
    {
        //TODO: This does not handle multiple client simulataneously
        loop {
            match self.listener.accept().await {
                Ok((connection, _)) => {
                    let reverse_callback = ReverseCallback::new(connection);
                    porky(reverse_callback).await;
                }
                Err(err) => {
                    panic!("Error: {err}");
                }
            }
        }
    }
}

async fn handle_client(
    accessor: impl AsyncWrite + AsyncRead + Unpin,
    mut stdin: impl AsyncWrite + Unpin,
    mut stdout: impl AsyncRead + Unpin,
) {
    let mut telnet = TelnetStream::from_accessor(accessor);

    telnet.send_will(TelnetOption::Echo).await.unwrap();
    telnet
        .send_will(TelnetOption::SuppressGoAhead)
        .await
        .unwrap();
    telnet
        .send_do(TelnetOption::NegotiateAboutWindowSize)
        .await
        .unwrap();

    loop {
        let mut data = vec![0; READ_BUFFER_SIZE];
        select! {
            // A small read buffer size will truncate output here
            bytes_read = stdout.read(&mut data) => match bytes_read{
                Ok(bytes_read) =>
                    if bytes_read != 0 {
                        // Pipe stdout to client
                        telnet.send_data(&data[0..bytes_read]).await.unwrap();
                        /*
                        for data in &data[0..bytes_read] {
                            print!("{}", *data as char);
                            std::io::stdout().flush().expect("Flush failed");
                        }
                        */
                    } else {
                        // Reading's done
                        println!("No more book learnin'");
                        break;
                    },
                Err(err) => {
                    println!("ERROR WHILE READING: {err}");
                    break;
                }
            },

            event = telnet.next() => {
                match event {
                    Some(TelnetEvent::Data(data)) => {
                        // Pipe data to program's stdin
                        for data in data {
                            let _ = stdin.write(&[data]).await.unwrap();
                        }
                    }
                    Some(other) => {
                        println!("Received Telnet stuff: {other:?}!");
                    }
                    None => {
                        println!("End o' stream");
                        break;
                    }
                }
            }
        }
    }
}
