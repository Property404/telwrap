use bytes::{BufMut, BytesMut};
use std::{
    marker::Unpin,
    pin::Pin,
    task::{Context, Poll},
};
use telly::{
    errors::{TellyError, TellyResult},
    utils::TellyIterTraits,
    TelnetEvent, TelnetOption, TelnetParser,
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio_stream::Stream;

const BUFFER_SIZE: usize = 16;

pub struct TelnetStream<Accessor>
where
    Accessor: AsyncWrite + AsyncRead,
{
    // Underlying accessor
    accessor: Accessor,

    // Bytes read from accessor, waiting to be processed
    rx_buffer: BytesMut,

    parser: TelnetParser,
}

impl<Accessor: AsyncWrite + AsyncRead + Unpin> TelnetStream<Accessor> {
    /// Construct a TelnetStream from, e.g., a TcpStream
    pub fn from_accessor(accessor: Accessor) -> Self {
        const CAPACITY: usize = BUFFER_SIZE;
        Self {
            accessor,
            rx_buffer: BytesMut::with_capacity(CAPACITY),
            parser: TelnetParser::default(),
        }
    }

    /// Send a TelnetEvent to remote
    pub async fn send_event(&mut self, event: TelnetEvent) -> TellyResult {
        let bytes = event.into_bytes();
        self.send_raw_bytes(&bytes).await
    }

    /// Convenience function to send a WILL negotiation event
    pub async fn send_will(&mut self, option: TelnetOption) -> TellyResult {
        self.send_event(TelnetEvent::will(option)).await
    }

    /// Convenience function to send a DO negotiation event
    pub async fn send_do(&mut self, option: TelnetOption) -> TellyResult {
        self.send_event(TelnetEvent::r#do(option)).await
    }

    /// Convenience function to send a WONT negotiation event
    pub async fn send_wont(&mut self, option: TelnetOption) -> TellyResult {
        self.send_event(TelnetEvent::wont(option)).await
    }

    /// Convenience function to send a DONT negotiation event
    pub async fn send_dont(&mut self, option: TelnetOption) -> TellyResult {
        self.send_event(TelnetEvent::dont(option)).await
    }

    /// Convenience function to send ASCII data to remote.
    pub async fn send_str(&mut self, data: &str) -> TellyResult {
        let bytes: Vec<u8> = data.as_bytes().iter().copied().escape_iacs().collect();
        self.send_raw_bytes(&bytes).await
    }

    /// Convenience function to send ASCII data to remote.
    pub async fn send_data(&mut self, data: &[u8]) -> TellyResult {
        self.send_event(TelnetEvent::Data(Vec::from(data))).await
    }

    /// Send raw telnet data to remote. This does NOT escape ASCII data.
    async fn send_raw_bytes(&mut self, bytes: &[u8]) -> TellyResult {
        if self.accessor.write(bytes).await? != bytes.len() {
            return Err(TellyError::DidNotWriteAllBytes);
        }
        self.accessor.flush().await?;
        Ok(())
    }

    fn next_event_from_buffer(&mut self) -> Option<TelnetEvent> {
        self.parser.next_event(&mut self.rx_buffer)
    }
}

impl<T: AsyncWrite + AsyncRead + Unpin> Stream for TelnetStream<T> {
    type Item = TelnetEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut vec: Vec<u8> = vec![0; BUFFER_SIZE];
        let mut vec = ReadBuf::new(&mut vec);

        if let Some(event) = self.next_event_from_buffer() {
            return Poll::Ready(Some(event));
        }

        loop {
            match Pin::new(&mut self.accessor).poll_read(cx, &mut vec) {
                Poll::Pending => {
                    return Poll::Pending;
                }
                Poll::Ready(Err(err)) => {
                    println!("Read error: {err}!");
                    return Poll::Ready(None);
                }
                Poll::Ready(Ok(())) => {
                    /* TODO: Is this right? AsyncRead doc says I have to check the difference */
                    if vec.filled().is_empty() {
                        println!("next> End of accessor(0)!");
                        return Poll::Ready(None);
                    }
                    self.rx_buffer.put(vec.filled());
                    if let Some(event) = self.next_event_from_buffer() {
                        return Poll::Ready(Some(event));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use telly::TelnetCommand;
    use tokio::io::{duplex, DuplexStream};
    use tokio_stream::StreamExt;

    fn get_mock_objects() -> (TelnetStream<DuplexStream>, TelnetStream<DuplexStream>) {
        let (client, server) = duplex(BUFFER_SIZE * 2);

        let client = TelnetStream::from_accessor(client);
        let server = TelnetStream::from_accessor(server);
        (client, server)
    }

    #[tokio::test]
    async fn send_string() {
        let (mut client, mut server) = get_mock_objects();

        let test_string = "Hello World!";
        client.send_str(&test_string).await.unwrap();

        match server.next().await.unwrap() {
            TelnetEvent::Data(data) => {
                assert_eq!(String::from_utf8_lossy(&data).to_string(), test_string);
            }
            _ => {
                panic!("Received telnet command but should have received data");
            }
        }
    }

    #[tokio::test]
    async fn send_events() {
        let (mut client, mut server) = get_mock_objects();

        let events = [
            TelnetEvent::Data(vec![0x42]),
            TelnetEvent::Data(vec![0x42, 0xFF, 0x41]),
            TelnetEvent::Data(vec![0x42, 0xFF]),
            TelnetEvent::Data(vec![0xFF]),
            TelnetEvent::Data(vec![0xFF, 0xFF]),
            TelnetEvent::Command(TelnetCommand::Nop),
            TelnetEvent::will(TelnetOption::SuppressGoAhead),
            TelnetEvent::dont(TelnetOption::TimingMark),
            TelnetEvent::wont(TelnetOption::BinaryTransmission),
            //TelnetSubnegotiation::TerminalTypeResponse("xterm-turbo-edition".into()).into(),
        ];

        for event in events {
            client.send_event(event.clone()).await.unwrap();
            assert_eq!(server.next().await, Some(event));
        }
    }
}
