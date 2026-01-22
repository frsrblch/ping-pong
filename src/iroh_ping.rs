#![allow(dead_code)]

use std::time::{Duration, Instant};

use iroh::{
    Endpoint, EndpointAddr,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};

/// Each protocol is identified by its ALPN string.
///
/// The ALPN, or application-layer protocol negotiation, is exchanged in the connection handshake,
/// and the connection is aborted unless both nodes pass the same bytestring.
pub const ALPN: &[u8] = b"iroh/ping/0";

/// Ping is our protocol struct.
///
/// We'll implement [`ProtocolHandler`] on this struct so we can use it with
/// an [`iroh::protocol::Router`].
/// It's also fine to keep state in this struct for use across many incoming
/// connections, in this case we'll keep metrics about the amount of pings we
/// sent or received.
#[derive(Debug, Clone)]
pub struct Ping;

impl Default for Ping {
    fn default() -> Self {
        Self::new()
    }
}

impl Ping {
    /// Creates new ping state.
    pub fn new() -> Self {
        Self
    }

    /// Sends a ping on the provided endpoint to a given node address.
    pub async fn ping(&self, endpoint: &Endpoint, addr: EndpointAddr) -> anyhow::Result<Duration> {
        // Open a connection to the accepting node
        let conn = endpoint.connect(addr, ALPN).await?;

        // Open a bidirectional QUIC stream
        let (mut send, mut recv) = conn.open_bi().await?;

        let start = Instant::now();

        // Send some data to be pinged
        send.write_all(b"PING").await?;

        // Signal the end of data for this particular stream
        send.finish()?;

        // read the response, which must be PONG as bytes
        let response = recv.read_to_end(4).await?;
        assert_eq!(&response, b"PONG");

        let ping = start.elapsed();

        // Explicitly close the whole connection, as we're the last ones to receive data
        // and know there's nothing else more to do in the connection.
        conn.close(0u32.into(), b"bye!");

        Ok(ping)
    }

    pub async fn ping_connected(&self, conn: &Connection) -> anyhow::Result<Duration> {
        let start = Instant::now();

        let (mut send, mut recv) = conn.open_bi().await?;

        // Send some data to be pinged
        send.write_all(b"PING").await?;

        // Signal the end of data for this particular stream
        send.finish()?;

        // read the response, which must be PONG as bytes
        let response = recv.read_to_end(4).await?;
        assert_eq!(&response, b"PONG");

        let ping = start.elapsed();

        Ok(ping)
    }
}

impl ProtocolHandler for Ping {
    /// The `accept` method is called for each incoming connection for our ALPN.
    ///
    /// The returned future runs on a newly spawned tokio task, so it can run as long as
    /// the connection lasts.
    async fn accept(&self, connection: Connection) -> n0_error::Result<(), AcceptError> {
        // We can get the remote's node id from the connection.
        let node_id = connection.remote_id();
        println!("accepted connection from {node_id}");

        // Our protocol is a simple request-response protocol, so we expect the
        // connecting peer to open a single bi-directional stream.
        loop {
            let (mut send, mut recv) = connection.accept_bi().await?;

            let req = recv.read_to_end(4).await.map_err(AcceptError::from_err)?;
            assert_eq!(&req, b"PING");

            // send back "PONG" bytes
            send.write_all(b"PONG")
                .await
                .map_err(AcceptError::from_err)?;

            // By calling `finish` on the send stream we signal that we will not send anything
            // further, which makes the receive stream on the other end terminate.
            send.finish()?;
        }

        // // Wait until the remote closes the connection, which it does once it
        // // received the response.
        // connection.closed().await;

        // Ok(())
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Context, Result};
    use iroh::{
        Endpoint,
        discovery::{Discovery, EndpointData},
        protocol::Router,
    };
    use tokio_stream::StreamExt;

    use super::*;

    #[tokio::test]
    async fn test_ping() -> Result<()> {
        let server_endpoint =
            anyhow::Context::context(Endpoint::builder().bind().await, "server bind")?;

        let server_router = Router::builder(server_endpoint)
            .accept(ALPN, Ping::new())
            .spawn();

        let server_addr = server_router.endpoint().addr();

        let client_endpoint =
            anyhow::Context::context(Endpoint::builder().bind().await, "bind client")?;

        let client_ping = Ping::new();

        let res = client_ping
            .ping(&client_endpoint, server_addr.clone())
            .await
            .context("ping 1")?;

        println!("ping response: {res:?}");

        let res = client_ping
            .ping(&client_endpoint, server_addr.clone())
            .await
            .context("ping 2")?;

        println!("ping response: {res:?}");

        client_endpoint.close().await;
        server_router.shutdown().await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_dht_pint() -> Result<()> {
        let dht_discovery = || iroh::discovery::pkarr::dht::DhtDiscovery::builder();

        let server_endpoint = Endpoint::empty_builder(iroh::RelayMode::Default)
            .discovery(dht_discovery())
            .bind()
            .await
            .context("server endpoint")?;

        let secret = server_endpoint.secret_key().clone();

        let server_router = Router::builder(server_endpoint)
            .accept(ALPN, Ping::new())
            .spawn();

        {
            let server_addr = server_router.endpoint().addr();

            server_router
                .endpoint()
                .discovery()
                .publish(&EndpointData::from(server_addr));
        }

        let client_endpoint = Endpoint::empty_builder(iroh::RelayMode::Default)
            .discovery(dht_discovery())
            .bind()
            .await
            .context("bind client")?;

        let start = std::time::Instant::now();

        let discovery_item = loop {
            if start.elapsed() > std::time::Duration::from_secs(60) {
                anyhow::bail!("no discovery");
            }

            let mut stream = client_endpoint
                .discovery()
                .resolve(secret.public())
                .context("resolve")?;

            if let Some(result) = stream.next().await {
                break result.context("stream item")?;
            }
        };

        let client_ping = Ping::new();

        let res = client_ping
            .ping(&client_endpoint, discovery_item.to_endpoint_addr())
            .await
            .context("ping 1")?;

        println!("ping response: {res:?}");

        let res = client_ping
            .ping(&client_endpoint, discovery_item.to_endpoint_addr())
            .await
            .context("ping 2")?;

        println!("ping response: {res:?}");

        client_endpoint.close().await;
        server_router.shutdown().await?;

        Ok(())
    }
}
