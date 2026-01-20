use anyhow::Result;
use clap::Parser;
use iroh::{Endpoint, SecretKey, protocol::Router};
use iroh_ping::Ping;
use iroh_tickets::endpoint::EndpointTicket;

#[derive(Debug, Clone, clap::Parser)]
pub enum Role {
    /// Run a router to receive pings
    Receiver,
    /// Connect to and ping the specified endpoint
    Sender { endpoint: EndpointTicket },
}

#[tokio::main]
async fn main() -> Result<()> {
    let role = Role::parse();

    match role {
        Role::Receiver => run_receiver().await,
        Role::Sender { endpoint } => {
            // let ticket = EndpointTicket::deserialize(&endpoint)
            //     .map_err(|e| anyhow!("failed to parse ticket: {}", e))?;

            run_sender(endpoint).await
        }
    }
}

async fn run_receiver() -> Result<()> {
    const BYTES: [u8; 32] = [
        131, 220, 235, 236, 181, 111, 162, 191, 226, 11, 31, 101, 238, 234, 128, 4, 161, 164, 63,
        31, 42, 71, 177, 92, 10, 177, 168, 252, 221, 181, 244, 180,
    ];

    let secret_key = SecretKey::from_bytes(&BYTES);

    // Create an endpoint, it allows creating and accepting
    // connections in the iroh p2p world
    let endpoint = Endpoint::builder().secret_key(secret_key).bind().await?;

    dbg!(endpoint.addr());

    // Wait for the endpoint to be accessible by others on the internet
    endpoint.online().await;

    // Then we initialize a struct that can accept ping requests over iroh connections
    let ping = Ping::new();

    // get the address of this endpoint to share with the sender
    let ticket = EndpointTicket::new(endpoint.addr());
    println!("{ticket}");

    // receiving ping requests
    let _router = Router::builder(endpoint)
        .accept(iroh_ping::ALPN, ping)
        .spawn();

    // Keep the receiver running until Ctrl+C
    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn run_sender(ticket: EndpointTicket) -> Result<()> {
    let send_ep = Endpoint::bind().await?;
    let send_pinger = Ping::new();
    let rtt = send_pinger
        .ping(&send_ep, ticket.endpoint_addr().clone())
        .await?;
    println!("ping took: {:?} to complete", rtt);
    Ok(())
}
