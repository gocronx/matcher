use crate::config::NetworkConfig;
use crate::core::MatchingEngine;
use crate::types::{Order, MatchResult, MatcherError};
use bytes::{Buf, BufMut, BytesMut};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{info, warn, error, debug};

/// Network message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    OrderSubmit(Order),
    OrderCancel { product_id: String, order_id: uuid::Uuid },
    MatchResult(MatchResult),
    Heartbeat,
}

/// Network handler for UDP multicast communication
pub struct NetworkHandler {
    config: NetworkConfig,
    engine: Arc<MatchingEngine>,
    
    // Sockets
    receive_socket: Arc<UdpSocket>,
    broadcast_socket: Arc<UdpSocket>,
    
    // Channels
    #[allow(dead_code)]
    message_tx: mpsc::UnboundedSender<NetworkMessage>,
    match_rx: mpsc::UnboundedReceiver<MatchResult>,
}

impl NetworkHandler {
    /// Create a new network handler
    pub async fn new(
        config: NetworkConfig,
        engine: Arc<MatchingEngine>,
    ) -> Result<Self, MatcherError> {
        // Create receive socket for multicast
        let receive_socket = Self::create_multicast_socket(&config.multicast_addr).await?;
        let receive_socket = Arc::new(receive_socket);
        
        // Create broadcast socket
        let broadcast_socket = UdpSocket::bind("0.0.0.0:0").await?;
        broadcast_socket.set_multicast_ttl_v4(64)?;
        let broadcast_socket = Arc::new(broadcast_socket);
        
        let (message_tx, _message_rx) = mpsc::unbounded_channel();
        let (_match_tx, match_rx) = mpsc::unbounded_channel();
        
        info!("Network handler initialized");
        info!("Listening on multicast: {}", config.multicast_addr);
        info!("Broadcasting to: {}", config.broadcast_addr);
        
        Ok(Self {
            config,
            engine,
            receive_socket,
            broadcast_socket,
            message_tx,
            match_rx,
        })
    }
    
    /// Start the network handler
    pub async fn start(self) -> Result<(), MatcherError> {
        info!("Starting network handler...");
        
        // Start receiver task
        let receive_socket = self.receive_socket.clone();
        let engine = self.engine.clone();
        let config = self.config.clone();
        
        let receive_task = tokio::spawn(async move {
            Self::run_receiver(receive_socket, engine, config).await
        });
        
        // Start broadcaster task
        let broadcast_socket = self.broadcast_socket.clone();
        let mut match_rx = self.match_rx;
        let broadcast_addr = self.config.broadcast_addr.clone();
        
        let broadcast_task = tokio::spawn(async move {
            Self::run_broadcaster(broadcast_socket, &mut match_rx, broadcast_addr).await
        });
        
        // Run both tasks concurrently
        tokio::select! {
            result = receive_task => {
                error!("Receiver task exited: {:?}", result);
                result.unwrap_or(Ok(()))
            }
            result = broadcast_task => {
                error!("Broadcaster task exited: {:?}", result);
                result.unwrap_or(Ok(()))
            }
        }
    }
    
    /// Start the message receiver task
    async fn run_receiver(
        socket: Arc<UdpSocket>,
        engine: Arc<MatchingEngine>,
        config: NetworkConfig,
    ) -> Result<(), MatcherError> {
        let mut buffer = vec![0u8; config.buffer_size];
        
        info!("Starting message receiver");
        
        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((size, addr)) => {
                    debug!("Received {} bytes from {}", size, addr);
                    
                    // Parse the message
                    match Self::parse_message(&buffer[..size]) {
                        Ok(message) => {
                            if let Err(e) = Self::handle_message(message, &engine).await {
                                warn!("Failed to handle message: {}", e);
                                engine.metrics().record_error();
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse message: {}", e);
                            engine.metrics().record_error();
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to receive message: {}", e);
                    engine.metrics().record_error();
                }
            }
        }
    }
    
    /// Start the match result broadcaster task
    async fn run_broadcaster(
        socket: Arc<UdpSocket>,
        match_rx: &mut mpsc::UnboundedReceiver<MatchResult>,
        broadcast_addr_str: String,
    ) -> Result<(), MatcherError> {
        let broadcast_addr: SocketAddr = broadcast_addr_str.parse()
            .map_err(|e| MatcherError::Config(format!("Invalid broadcast address: {}", e)))?;
        
        info!("Starting match result broadcaster");
        
        while let Some(match_result) = match_rx.recv().await {
            let message = NetworkMessage::MatchResult(match_result);
            
            match Self::serialize_message(&message) {
                Ok(data) => {
                    if let Err(e) = socket.send_to(&data, broadcast_addr).await {
                        error!("Failed to broadcast match result: {}", e);
                    } else {
                        debug!("Broadcasted match result");
                    }
                }
                Err(e) => {
                    error!("Failed to serialize match result: {}", e);
                }
            }
        }
        
        Ok(())
    }
    
    /// Handle an incoming network message
    async fn handle_message(
        message: NetworkMessage,
        engine: &Arc<MatchingEngine>,
    ) -> Result<(), MatcherError> {
        match message {
            NetworkMessage::OrderSubmit(order) => {
                debug!("Processing order submission: {}", order.id);
                
                let matches = engine.submit_order(order).await?;
                
                // Send matches to broadcaster (this would need proper channel setup)
                for match_result in matches {
                    debug!("Generated match: {:?}", match_result);
                }
                
                Ok(())
            }
            NetworkMessage::OrderCancel { product_id, order_id } => {
                debug!("Processing order cancellation: {}", order_id);
                
                let _cancelled_order = engine.cancel_order(&product_id, order_id).await?;
                
                Ok(())
            }
            NetworkMessage::MatchResult(_) => {
                // Match results are outbound only
                warn!("Received unexpected match result message");
                Ok(())
            }
            NetworkMessage::Heartbeat => {
                debug!("Received heartbeat");
                Ok(())
            }
        }
    }
    
    /// Parse a network message from bytes
    fn parse_message(data: &[u8]) -> Result<NetworkMessage, MatcherError> {
        // Simple JSON deserialization (in production, consider binary protocols)
        serde_json::from_slice(data).map_err(|e| e.into())
    }
    
    /// Serialize a network message to bytes
    fn serialize_message(message: &NetworkMessage) -> Result<Vec<u8>, MatcherError> {
        // Simple JSON serialization (in production, consider binary protocols)
        serde_json::to_vec(message).map_err(|e| e.into())
    }
    
    /// Create a multicast UDP socket
    async fn create_multicast_socket(multicast_addr: &str) -> Result<UdpSocket, MatcherError> {
        use socket2::{Domain, Protocol, Socket, Type};
        use std::net::{IpAddr, Ipv4Addr};
        
        // Parse multicast address
        let addr: SocketAddr = multicast_addr.parse()
            .map_err(|e| MatcherError::Config(format!("Invalid multicast address: {}", e)))?;
        
        let multicast_ip = match addr.ip() {
            IpAddr::V4(ip) => ip,
            _ => return Err(MatcherError::Config("Multicast address must be IPv4".to_string())),
        };
        
        // Create socket
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        
        // Set socket options
        socket.set_reuse_address(true)?;
        socket.set_nonblocking(true)?;
        
        // Bind to the multicast port
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), addr.port());
        socket.bind(&bind_addr.into())?;
        
        // Join multicast group
        socket.join_multicast_v4(&multicast_ip, &Ipv4Addr::UNSPECIFIED)?;
        
        // Convert to tokio socket
        let std_socket: std::net::UdpSocket = socket.into();
        let tokio_socket = UdpSocket::from_std(std_socket)?;
        
        Ok(tokio_socket)
    }
}

/// Message codec for efficient serialization/deserialization
pub struct MessageCodec;

impl MessageCodec {
    /// Encode a message to bytes with length prefix
    pub fn encode(message: &NetworkMessage) -> Result<BytesMut, MatcherError> {
        let json_data = serde_json::to_vec(message)?;
        let mut buf = BytesMut::with_capacity(4 + json_data.len());
        
        // Write length prefix (4 bytes, big-endian)
        buf.put_u32(json_data.len() as u32);
        
        // Write message data
        buf.put_slice(&json_data);
        
        Ok(buf)
    }
    
    /// Decode a message from bytes with length prefix
    pub fn decode(mut buf: BytesMut) -> Result<Option<NetworkMessage>, MatcherError> {
        if buf.len() < 4 {
            return Ok(None); // Need more data for length prefix
        }
        
        let length = buf.get_u32() as usize;
        
        if buf.len() < length {
            return Ok(None); // Need more data for complete message
        }
        
        let message_data = buf.split_to(length);
        let message: NetworkMessage = serde_json::from_slice(&message_data)?;
        
        Ok(Some(message))
    }
}