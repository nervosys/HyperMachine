//! Agent Communication - Inter-agent messaging and coordination
//!
//! This module provides communication primitives for AI agents:
//! - Message passing between agents
//! - Request/response patterns
//! - Pub/sub channels
//! - Agent discovery

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// Result type for communication operations
pub type CommResult<T> = Result<T, CommError>;

/// Communication errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommError {
    /// Agent not found
    AgentNotFound(String),
    /// Channel not found
    ChannelNotFound(String),
    /// Message delivery failed
    DeliveryFailed(String),
    /// Timeout waiting for response
    Timeout(String),
    /// Queue is full
    QueueFull(String),
    /// Invalid message format
    InvalidMessage(String),
    /// Agent is disconnected
    Disconnected(String),
}

impl std::fmt::Display for CommError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AgentNotFound(id) => write!(f, "Agent not found: {}", id),
            Self::ChannelNotFound(name) => write!(f, "Channel not found: {}", name),
            Self::DeliveryFailed(msg) => write!(f, "Delivery failed: {}", msg),
            Self::Timeout(msg) => write!(f, "Timeout: {}", msg),
            Self::QueueFull(msg) => write!(f, "Queue full: {}", msg),
            Self::InvalidMessage(msg) => write!(f, "Invalid message: {}", msg),
            Self::Disconnected(id) => write!(f, "Agent disconnected: {}", id),
        }
    }
}

impl std::error::Error for CommError {}

/// Message priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MessagePriority {
    /// Low priority - can be delayed
    Low = 0,
    /// Normal priority
    Normal = 1,
    /// High priority - process quickly
    High = 2,
    /// Critical - immediate processing
    Critical = 3,
}

impl Default for MessagePriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Message types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    /// Informational message (fire and forget)
    Info,
    /// Request expecting a response
    Request,
    /// Response to a request
    Response,
    /// Broadcast to multiple recipients
    Broadcast,
    /// Heartbeat/ping
    Heartbeat,
    /// Error notification
    Error,
    /// Control message (agent lifecycle)
    Control,
}

/// Message payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePayload {
    /// Content type (e.g., "application/json", "text/plain")
    pub content_type: String,
    /// Serialized data
    pub data: Vec<u8>,
}

impl MessagePayload {
    /// Create a JSON payload
    pub fn json<T: Serialize>(value: &T) -> CommResult<Self> {
        let data = serde_json::to_vec(value)
            .map_err(|e| CommError::InvalidMessage(e.to_string()))?;
        Ok(Self {
            content_type: "application/json".to_string(),
            data,
        })
    }
    
    /// Create a text payload
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content_type: "text/plain".to_string(),
            data: content.into().into_bytes(),
        }
    }
    
    /// Create a binary payload
    pub fn binary(data: Vec<u8>) -> Self {
        Self {
            content_type: "application/octet-stream".to_string(),
            data,
        }
    }
    
    /// Deserialize JSON payload
    pub fn as_json<T: for<'de> Deserialize<'de>>(&self) -> CommResult<T> {
        serde_json::from_slice(&self.data)
            .map_err(|e| CommError::InvalidMessage(e.to_string()))
    }
    
    /// Get as text
    pub fn as_text(&self) -> CommResult<String> {
        String::from_utf8(self.data.clone())
            .map_err(|e| CommError::InvalidMessage(e.to_string()))
    }
    
    /// Get the raw data
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    
    /// Get the size in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// A message between agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique message ID
    pub id: String,
    /// Sender agent ID
    pub sender: String,
    /// Recipient agent ID (or channel name for broadcasts)
    pub recipient: String,
    /// Message type
    pub message_type: MessageType,
    /// Priority level
    pub priority: MessagePriority,
    /// Message payload
    pub payload: MessagePayload,
    /// Correlation ID for request/response matching
    pub correlation_id: Option<String>,
    /// Timestamp when message was created
    pub timestamp: SystemTime,
    /// Time-to-live (message expires after this duration)
    pub ttl: Option<Duration>,
    /// Custom headers
    pub headers: HashMap<String, String>,
}

impl Message {
    /// Create a new message
    pub fn new(
        sender: impl Into<String>,
        recipient: impl Into<String>,
        payload: MessagePayload,
    ) -> Self {
        Self {
            id: uuid_v4(),
            sender: sender.into(),
            recipient: recipient.into(),
            message_type: MessageType::Info,
            priority: MessagePriority::Normal,
            payload,
            correlation_id: None,
            timestamp: SystemTime::now(),
            ttl: None,
            headers: HashMap::new(),
        }
    }
    
    /// Create a request message
    pub fn request(
        sender: impl Into<String>,
        recipient: impl Into<String>,
        payload: MessagePayload,
    ) -> Self {
        let mut msg = Self::new(sender, recipient, payload);
        msg.message_type = MessageType::Request;
        msg.correlation_id = Some(msg.id.clone());
        msg
    }
    
    /// Create a response to a request
    pub fn response(request: &Message, sender: impl Into<String>, payload: MessagePayload) -> Self {
        let mut msg = Self::new(sender, &request.sender, payload);
        msg.message_type = MessageType::Response;
        msg.correlation_id = request.correlation_id.clone();
        msg
    }
    
    /// Create a broadcast message
    pub fn broadcast(sender: impl Into<String>, channel: impl Into<String>, payload: MessagePayload) -> Self {
        let mut msg = Self::new(sender, channel, payload);
        msg.message_type = MessageType::Broadcast;
        msg
    }
    
    /// Set priority
    pub fn with_priority(mut self, priority: MessagePriority) -> Self {
        self.priority = priority;
        self
    }
    
    /// Set TTL
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }
    
    /// Add a header
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
    
    /// Check if message has expired
    pub fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl {
            if let Ok(elapsed) = self.timestamp.elapsed() {
                return elapsed > ttl;
            }
        }
        false
    }
    
    /// Check if this is a request
    pub fn is_request(&self) -> bool {
        self.message_type == MessageType::Request
    }
    
    /// Check if this is a response
    pub fn is_response(&self) -> bool {
        self.message_type == MessageType::Response
    }
}

/// Generate a simple UUID v4
fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:032x}", timestamp)
}

/// Agent registration info
#[derive(Debug, Clone)]
pub struct AgentInfo {
    /// Agent ID
    pub id: String,
    /// Agent name
    pub name: String,
    /// Agent capabilities/tags
    pub capabilities: Vec<String>,
    /// Connection timestamp
    pub connected_at: Instant,
    /// Last activity timestamp
    pub last_activity: Instant,
    /// Is agent currently active
    pub active: bool,
}

impl AgentInfo {
    /// Create new agent info
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Instant::now();
        Self {
            id: id.into(),
            name: name.into(),
            capabilities: Vec::new(),
            connected_at: now,
            last_activity: now,
            active: true,
        }
    }
    
    /// Add a capability
    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.push(capability.into());
        self
    }
    
    /// Check if agent has a capability
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }
    
    /// Update last activity
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }
    
    /// Get uptime
    pub fn uptime(&self) -> Duration {
        self.connected_at.elapsed()
    }
    
    /// Get idle time
    pub fn idle_time(&self) -> Duration {
        self.last_activity.elapsed()
    }
}

/// Message queue for an agent
#[derive(Debug)]
pub struct MessageQueue {
    /// Queued messages (sorted by priority)
    messages: VecDeque<Message>,
    /// Maximum queue size
    max_size: usize,
    /// Total messages received
    received_count: u64,
    /// Total messages dropped
    dropped_count: u64,
}

impl MessageQueue {
    /// Create a new message queue
    pub fn new(max_size: usize) -> Self {
        Self {
            messages: VecDeque::new(),
            max_size,
            received_count: 0,
            dropped_count: 0,
        }
    }
    
    /// Push a message to the queue
    pub fn push(&mut self, message: Message) -> CommResult<()> {
        self.received_count += 1;
        
        // Remove expired messages first
        self.messages.retain(|m| !m.is_expired());
        
        if self.messages.len() >= self.max_size {
            self.dropped_count += 1;
            return Err(CommError::QueueFull(format!(
                "Queue is full (max {})",
                self.max_size
            )));
        }
        
        // Insert based on priority (higher priority messages go to front)
        let pos = self
            .messages
            .iter()
            .position(|m| m.priority < message.priority)
            .unwrap_or(self.messages.len());
        
        self.messages.insert(pos, message);
        Ok(())
    }
    
    /// Pop the next message
    pub fn pop(&mut self) -> Option<Message> {
        // Remove expired messages first
        self.messages.retain(|m| !m.is_expired());
        self.messages.pop_front()
    }
    
    /// Peek at the next message without removing it
    pub fn peek(&self) -> Option<&Message> {
        self.messages.front()
    }
    
    /// Get queue length
    pub fn len(&self) -> usize {
        self.messages.len()
    }
    
    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
    
    /// Get statistics
    pub fn stats(&self) -> (u64, u64) {
        (self.received_count, self.dropped_count)
    }
    
    /// Clear the queue
    pub fn clear(&mut self) {
        self.messages.clear();
    }
}

/// A publish/subscribe channel
#[derive(Debug)]
pub struct Channel {
    /// Channel name
    name: String,
    /// Channel description
    description: String,
    /// Subscribed agent IDs
    subscribers: Vec<String>,
    /// Message history (for late joiners)
    history: VecDeque<Message>,
    /// Maximum history size
    max_history: usize,
    /// Total messages published
    published_count: u64,
}

impl Channel {
    /// Create a new channel
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            subscribers: Vec::new(),
            history: VecDeque::new(),
            max_history: 100,
            published_count: 0,
        }
    }
    
    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
    
    /// Set max history size
    pub fn with_max_history(mut self, max: usize) -> Self {
        self.max_history = max;
        self
    }
    
    /// Subscribe an agent to this channel
    pub fn subscribe(&mut self, agent_id: &str) {
        if !self.subscribers.contains(&agent_id.to_string()) {
            self.subscribers.push(agent_id.to_string());
        }
    }
    
    /// Unsubscribe an agent from this channel
    pub fn unsubscribe(&mut self, agent_id: &str) {
        self.subscribers.retain(|id| id != agent_id);
    }
    
    /// Check if agent is subscribed
    pub fn is_subscribed(&self, agent_id: &str) -> bool {
        self.subscribers.contains(&agent_id.to_string())
    }
    
    /// Get subscriber count
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }
    
    /// Get all subscribers
    pub fn subscribers(&self) -> &[String] {
        &self.subscribers
    }
    
    /// Add message to history
    pub fn add_to_history(&mut self, message: Message) {
        self.published_count += 1;
        self.history.push_back(message);
        while self.history.len() > self.max_history {
            self.history.pop_front();
        }
    }
    
    /// Get recent history
    pub fn recent_history(&self, count: usize) -> Vec<&Message> {
        self.history
            .iter()
            .rev()
            .take(count)
            .collect()
    }
    
    /// Get channel name
    pub fn name(&self) -> &str {
        &self.name
    }
    
    /// Get published message count
    pub fn published_count(&self) -> u64 {
        self.published_count
    }
}

/// Message router for inter-agent communication
#[derive(Debug)]
pub struct MessageRouter {
    /// Registered agents
    agents: HashMap<String, AgentInfo>,
    /// Message queues per agent
    queues: HashMap<String, MessageQueue>,
    /// Publish/subscribe channels
    channels: HashMap<String, Channel>,
    /// Default queue size
    default_queue_size: usize,
    /// Pending requests (correlation_id -> sender)
    pending_requests: HashMap<String, String>,
}

impl Default for MessageRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageRouter {
    /// Create a new message router
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            queues: HashMap::new(),
            channels: HashMap::new(),
            default_queue_size: 1000,
            pending_requests: HashMap::new(),
        }
    }
    
    /// Set default queue size
    pub fn with_default_queue_size(mut self, size: usize) -> Self {
        self.default_queue_size = size;
        self
    }
    
    /// Register an agent
    pub fn register_agent(&mut self, agent: AgentInfo) {
        let agent_id = agent.id.clone();
        self.agents.insert(agent_id.clone(), agent);
        self.queues.insert(agent_id, MessageQueue::new(self.default_queue_size));
    }
    
    /// Unregister an agent
    pub fn unregister_agent(&mut self, agent_id: &str) -> Option<AgentInfo> {
        self.queues.remove(agent_id);
        
        // Unsubscribe from all channels
        for channel in self.channels.values_mut() {
            channel.unsubscribe(agent_id);
        }
        
        // Remove pending requests from this agent
        self.pending_requests.retain(|_, sender| sender != agent_id);
        
        self.agents.remove(agent_id)
    }
    
    /// Get agent info
    pub fn get_agent(&self, agent_id: &str) -> Option<&AgentInfo> {
        self.agents.get(agent_id)
    }
    
    /// Get mutable agent info
    pub fn get_agent_mut(&mut self, agent_id: &str) -> Option<&mut AgentInfo> {
        self.agents.get_mut(agent_id)
    }
    
    /// List all registered agents
    pub fn list_agents(&self) -> Vec<&AgentInfo> {
        self.agents.values().collect()
    }
    
    /// Find agents by capability
    pub fn find_agents_by_capability(&self, capability: &str) -> Vec<&AgentInfo> {
        self.agents
            .values()
            .filter(|a| a.has_capability(capability))
            .collect()
    }
    
    /// Create a channel
    pub fn create_channel(&mut self, channel: Channel) {
        self.channels.insert(channel.name.clone(), channel);
    }
    
    /// Delete a channel
    pub fn delete_channel(&mut self, name: &str) -> Option<Channel> {
        self.channels.remove(name)
    }
    
    /// Get a channel
    pub fn get_channel(&self, name: &str) -> Option<&Channel> {
        self.channels.get(name)
    }
    
    /// Get mutable channel
    pub fn get_channel_mut(&mut self, name: &str) -> Option<&mut Channel> {
        self.channels.get_mut(name)
    }
    
    /// List all channels
    pub fn list_channels(&self) -> Vec<&Channel> {
        self.channels.values().collect()
    }
    
    /// Subscribe an agent to a channel
    pub fn subscribe(&mut self, agent_id: &str, channel_name: &str) -> CommResult<()> {
        if !self.agents.contains_key(agent_id) {
            return Err(CommError::AgentNotFound(agent_id.to_string()));
        }
        
        let channel = self
            .channels
            .get_mut(channel_name)
            .ok_or_else(|| CommError::ChannelNotFound(channel_name.to_string()))?;
        
        channel.subscribe(agent_id);
        Ok(())
    }
    
    /// Unsubscribe an agent from a channel
    pub fn unsubscribe(&mut self, agent_id: &str, channel_name: &str) -> CommResult<()> {
        let channel = self
            .channels
            .get_mut(channel_name)
            .ok_or_else(|| CommError::ChannelNotFound(channel_name.to_string()))?;
        
        channel.unsubscribe(agent_id);
        Ok(())
    }
    
    /// Send a message to an agent
    pub fn send(&mut self, message: Message) -> CommResult<()> {
        // Validate sender exists
        if !self.agents.contains_key(&message.sender) {
            return Err(CommError::AgentNotFound(message.sender.clone()));
        }
        
        // Update sender's last activity
        if let Some(agent) = self.agents.get_mut(&message.sender) {
            agent.touch();
        }
        
        match message.message_type {
            MessageType::Broadcast => {
                // Send to all subscribers of the channel
                let channel = self
                    .channels
                    .get_mut(&message.recipient)
                    .ok_or_else(|| CommError::ChannelNotFound(message.recipient.clone()))?;
                
                let subscribers: Vec<String> = channel.subscribers().to_vec();
                channel.add_to_history(message.clone());
                
                for subscriber in subscribers {
                    if let Some(queue) = self.queues.get_mut(&subscriber) {
                        // Best effort delivery for broadcasts
                        let _ = queue.push(message.clone());
                    }
                }
                
                Ok(())
            }
            MessageType::Request => {
                // Track pending request
                if let Some(ref correlation_id) = message.correlation_id {
                    self.pending_requests
                        .insert(correlation_id.clone(), message.sender.clone());
                }
                
                // Validate recipient exists
                let queue = self
                    .queues
                    .get_mut(&message.recipient)
                    .ok_or_else(|| CommError::AgentNotFound(message.recipient.clone()))?;
                
                queue.push(message)
            }
            MessageType::Response => {
                // Validate this is a response to a pending request
                if let Some(ref correlation_id) = message.correlation_id {
                    self.pending_requests.remove(correlation_id);
                }
                
                // Validate recipient exists
                let queue = self
                    .queues
                    .get_mut(&message.recipient)
                    .ok_or_else(|| CommError::AgentNotFound(message.recipient.clone()))?;
                
                queue.push(message)
            }
            _ => {
                // Direct message to recipient
                let queue = self
                    .queues
                    .get_mut(&message.recipient)
                    .ok_or_else(|| CommError::AgentNotFound(message.recipient.clone()))?;
                
                queue.push(message)
            }
        }
    }
    
    /// Receive next message for an agent
    pub fn receive(&mut self, agent_id: &str) -> CommResult<Option<Message>> {
        let queue = self
            .queues
            .get_mut(agent_id)
            .ok_or_else(|| CommError::AgentNotFound(agent_id.to_string()))?;
        
        // Update agent's last activity
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.touch();
        }
        
        Ok(queue.pop())
    }
    
    /// Peek at next message without removing it
    pub fn peek(&self, agent_id: &str) -> CommResult<Option<&Message>> {
        let queue = self
            .queues
            .get(agent_id)
            .ok_or_else(|| CommError::AgentNotFound(agent_id.to_string()))?;
        
        Ok(queue.peek())
    }
    
    /// Get queue length for an agent
    pub fn queue_length(&self, agent_id: &str) -> CommResult<usize> {
        let queue = self
            .queues
            .get(agent_id)
            .ok_or_else(|| CommError::AgentNotFound(agent_id.to_string()))?;
        
        Ok(queue.len())
    }
    
    /// Get agent count
    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }
    
    /// Get channel count
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }
}

/// Thread-safe message router wrapper
#[derive(Debug, Clone)]
pub struct SharedRouter {
    inner: Arc<Mutex<MessageRouter>>,
}

impl Default for SharedRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedRouter {
    /// Create a new shared router
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MessageRouter::new())),
        }
    }
    
    /// Register an agent
    pub fn register_agent(&self, agent: AgentInfo) {
        self.inner.lock().unwrap().register_agent(agent);
    }
    
    /// Unregister an agent
    pub fn unregister_agent(&self, agent_id: &str) -> Option<AgentInfo> {
        self.inner.lock().unwrap().unregister_agent(agent_id)
    }
    
    /// Send a message
    pub fn send(&self, message: Message) -> CommResult<()> {
        self.inner.lock().unwrap().send(message)
    }
    
    /// Receive a message
    pub fn receive(&self, agent_id: &str) -> CommResult<Option<Message>> {
        self.inner.lock().unwrap().receive(agent_id)
    }
    
    /// Create a channel
    pub fn create_channel(&self, channel: Channel) {
        self.inner.lock().unwrap().create_channel(channel);
    }
    
    /// Subscribe to a channel
    pub fn subscribe(&self, agent_id: &str, channel_name: &str) -> CommResult<()> {
        self.inner.lock().unwrap().subscribe(agent_id, channel_name)
    }
    
    /// Get agent count
    pub fn agent_count(&self) -> usize {
        self.inner.lock().unwrap().agent_count()
    }
    
    /// Get channel count
    pub fn channel_count(&self) -> usize {
        self.inner.lock().unwrap().channel_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comm_error_display() {
        let err = CommError::AgentNotFound("agent-1".into());
        assert!(err.to_string().contains("Agent not found"));
        
        let err = CommError::QueueFull("max 100".into());
        assert!(err.to_string().contains("Queue full"));
    }

    #[test]
    fn test_message_priority_ordering() {
        assert!(MessagePriority::Critical > MessagePriority::High);
        assert!(MessagePriority::High > MessagePriority::Normal);
        assert!(MessagePriority::Normal > MessagePriority::Low);
    }

    #[test]
    fn test_message_payload_json() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct TestData {
            value: i32,
            name: String,
        }
        
        let data = TestData {
            value: 42,
            name: "test".to_string(),
        };
        
        let payload = MessagePayload::json(&data).unwrap();
        assert_eq!(payload.content_type, "application/json");
        
        let decoded: TestData = payload.as_json().unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_message_payload_text() {
        let payload = MessagePayload::text("Hello, world!");
        assert_eq!(payload.content_type, "text/plain");
        assert_eq!(payload.as_text().unwrap(), "Hello, world!");
    }

    #[test]
    fn test_message_payload_binary() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let payload = MessagePayload::binary(data.clone());
        
        assert_eq!(payload.content_type, "application/octet-stream");
        assert_eq!(payload.as_bytes(), &data[..]);
        assert_eq!(payload.size(), 4);
    }

    #[test]
    fn test_message_creation() {
        let msg = Message::new("sender", "recipient", MessagePayload::text("test"));
        
        assert_eq!(msg.sender, "sender");
        assert_eq!(msg.recipient, "recipient");
        assert_eq!(msg.message_type, MessageType::Info);
        assert_eq!(msg.priority, MessagePriority::Normal);
        assert!(!msg.id.is_empty());
    }

    #[test]
    fn test_message_request_response() {
        let request = Message::request("client", "server", MessagePayload::text("ping"));
        
        assert!(request.is_request());
        assert!(request.correlation_id.is_some());
        
        let response = Message::response(&request, "server", MessagePayload::text("pong"));
        
        assert!(response.is_response());
        assert_eq!(response.correlation_id, request.correlation_id);
        assert_eq!(response.recipient, "client");
    }

    #[test]
    fn test_message_broadcast() {
        let msg = Message::broadcast("sender", "announcements", MessagePayload::text("hello"));
        
        assert_eq!(msg.message_type, MessageType::Broadcast);
        assert_eq!(msg.recipient, "announcements");
    }

    #[test]
    fn test_message_with_options() {
        let msg = Message::new("sender", "recipient", MessagePayload::text("test"))
            .with_priority(MessagePriority::High)
            .with_ttl(Duration::from_secs(60))
            .with_header("X-Custom", "value");
        
        assert_eq!(msg.priority, MessagePriority::High);
        assert!(msg.ttl.is_some());
        assert_eq!(msg.headers.get("X-Custom"), Some(&"value".to_string()));
    }

    #[test]
    fn test_message_expiration() {
        let msg = Message::new("sender", "recipient", MessagePayload::text("test"))
            .with_ttl(Duration::from_millis(1));
        
        // Wait for message to expire
        std::thread::sleep(Duration::from_millis(10));
        assert!(msg.is_expired());
        
        let fresh_msg = Message::new("sender", "recipient", MessagePayload::text("test"));
        assert!(!fresh_msg.is_expired());
    }

    #[test]
    fn test_agent_info() {
        let agent = AgentInfo::new("agent-1", "Test Agent")
            .with_capability("vm-management")
            .with_capability("monitoring");
        
        assert_eq!(agent.id, "agent-1");
        assert!(agent.has_capability("vm-management"));
        assert!(!agent.has_capability("unknown"));
        assert!(agent.active);
    }

    #[test]
    fn test_agent_info_timing() {
        let mut agent = AgentInfo::new("agent-1", "Test Agent");
        std::thread::sleep(Duration::from_millis(10));
        
        assert!(agent.uptime() >= Duration::from_millis(10));
        assert!(agent.idle_time() >= Duration::from_millis(10));
        
        agent.touch();
        assert!(agent.idle_time() < Duration::from_millis(5));
    }

    #[test]
    fn test_message_queue_push_pop() {
        let mut queue = MessageQueue::new(100);
        
        queue.push(Message::new("a", "b", MessagePayload::text("1"))).unwrap();
        queue.push(Message::new("a", "b", MessagePayload::text("2"))).unwrap();
        
        assert_eq!(queue.len(), 2);
        
        let msg = queue.pop().unwrap();
        assert_eq!(msg.payload.as_text().unwrap(), "1");
        
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn test_message_queue_priority() {
        let mut queue = MessageQueue::new(100);
        
        queue.push(
            Message::new("a", "b", MessagePayload::text("low"))
                .with_priority(MessagePriority::Low)
        ).unwrap();
        queue.push(
            Message::new("a", "b", MessagePayload::text("high"))
                .with_priority(MessagePriority::High)
        ).unwrap();
        queue.push(
            Message::new("a", "b", MessagePayload::text("normal"))
                .with_priority(MessagePriority::Normal)
        ).unwrap();
        
        // Should get high priority first
        assert_eq!(queue.pop().unwrap().payload.as_text().unwrap(), "high");
        assert_eq!(queue.pop().unwrap().payload.as_text().unwrap(), "normal");
        assert_eq!(queue.pop().unwrap().payload.as_text().unwrap(), "low");
    }

    #[test]
    fn test_message_queue_full() {
        let mut queue = MessageQueue::new(2);
        
        queue.push(Message::new("a", "b", MessagePayload::text("1"))).unwrap();
        queue.push(Message::new("a", "b", MessagePayload::text("2"))).unwrap();
        
        let result = queue.push(Message::new("a", "b", MessagePayload::text("3")));
        assert!(matches!(result, Err(CommError::QueueFull(_))));
        
        let (received, dropped) = queue.stats();
        assert_eq!(received, 3);
        assert_eq!(dropped, 1);
    }

    #[test]
    fn test_message_queue_expired_cleanup() {
        let mut queue = MessageQueue::new(100);
        
        queue.push(
            Message::new("a", "b", MessagePayload::text("expires"))
                .with_ttl(Duration::from_millis(1))
        ).unwrap();
        queue.push(
            Message::new("a", "b", MessagePayload::text("stays"))
        ).unwrap();
        
        std::thread::sleep(Duration::from_millis(10));
        
        // Expired message should be removed during pop
        let msg = queue.pop().unwrap();
        assert_eq!(msg.payload.as_text().unwrap(), "stays");
    }

    #[test]
    fn test_channel_subscription() {
        let mut channel = Channel::new("test-channel")
            .with_description("Test channel")
            .with_max_history(50);
        
        assert_eq!(channel.name(), "test-channel");
        assert_eq!(channel.subscriber_count(), 0);
        
        channel.subscribe("agent-1");
        channel.subscribe("agent-2");
        
        assert!(channel.is_subscribed("agent-1"));
        assert_eq!(channel.subscriber_count(), 2);
        
        channel.unsubscribe("agent-1");
        assert!(!channel.is_subscribed("agent-1"));
    }

    #[test]
    fn test_channel_history() {
        let mut channel = Channel::new("test-channel")
            .with_max_history(3);
        
        channel.add_to_history(Message::broadcast("a", "test-channel", MessagePayload::text("1")));
        channel.add_to_history(Message::broadcast("a", "test-channel", MessagePayload::text("2")));
        channel.add_to_history(Message::broadcast("a", "test-channel", MessagePayload::text("3")));
        channel.add_to_history(Message::broadcast("a", "test-channel", MessagePayload::text("4")));
        
        assert_eq!(channel.published_count(), 4);
        
        let history = channel.recent_history(10);
        assert_eq!(history.len(), 3); // Limited by max_history
    }

    #[test]
    fn test_router_agent_registration() {
        let mut router = MessageRouter::new();
        
        router.register_agent(AgentInfo::new("agent-1", "Agent One"));
        router.register_agent(AgentInfo::new("agent-2", "Agent Two"));
        
        assert_eq!(router.agent_count(), 2);
        assert!(router.get_agent("agent-1").is_some());
        
        router.unregister_agent("agent-1");
        assert_eq!(router.agent_count(), 1);
        assert!(router.get_agent("agent-1").is_none());
    }

    #[test]
    fn test_router_find_by_capability() {
        let mut router = MessageRouter::new();
        
        router.register_agent(
            AgentInfo::new("agent-1", "Agent One")
                .with_capability("monitoring")
        );
        router.register_agent(
            AgentInfo::new("agent-2", "Agent Two")
                .with_capability("monitoring")
                .with_capability("alerting")
        );
        router.register_agent(
            AgentInfo::new("agent-3", "Agent Three")
                .with_capability("alerting")
        );
        
        let monitoring_agents = router.find_agents_by_capability("monitoring");
        assert_eq!(monitoring_agents.len(), 2);
        
        let alerting_agents = router.find_agents_by_capability("alerting");
        assert_eq!(alerting_agents.len(), 2);
    }

    #[test]
    fn test_router_direct_messaging() {
        let mut router = MessageRouter::new();
        
        router.register_agent(AgentInfo::new("sender", "Sender"));
        router.register_agent(AgentInfo::new("recipient", "Recipient"));
        
        let msg = Message::new("sender", "recipient", MessagePayload::text("hello"));
        router.send(msg).unwrap();
        
        assert_eq!(router.queue_length("recipient").unwrap(), 1);
        
        let received = router.receive("recipient").unwrap().unwrap();
        assert_eq!(received.payload.as_text().unwrap(), "hello");
    }

    #[test]
    fn test_router_send_to_unknown_agent() {
        let mut router = MessageRouter::new();
        router.register_agent(AgentInfo::new("sender", "Sender"));
        
        let msg = Message::new("sender", "unknown", MessagePayload::text("hello"));
        let result = router.send(msg);
        
        assert!(matches!(result, Err(CommError::AgentNotFound(_))));
    }

    #[test]
    fn test_router_broadcast() {
        let mut router = MessageRouter::new();
        
        router.register_agent(AgentInfo::new("sender", "Sender"));
        router.register_agent(AgentInfo::new("sub-1", "Subscriber 1"));
        router.register_agent(AgentInfo::new("sub-2", "Subscriber 2"));
        
        router.create_channel(Channel::new("announcements"));
        router.subscribe("sub-1", "announcements").unwrap();
        router.subscribe("sub-2", "announcements").unwrap();
        
        let msg = Message::broadcast("sender", "announcements", MessagePayload::text("broadcast"));
        router.send(msg).unwrap();
        
        // Both subscribers should have the message
        assert_eq!(router.queue_length("sub-1").unwrap(), 1);
        assert_eq!(router.queue_length("sub-2").unwrap(), 1);
    }

    #[test]
    fn test_router_request_response() {
        let mut router = MessageRouter::new();
        
        router.register_agent(AgentInfo::new("client", "Client"));
        router.register_agent(AgentInfo::new("server", "Server"));
        
        // Client sends request
        let request = Message::request("client", "server", MessagePayload::text("ping"));
        let correlation_id = request.correlation_id.clone();
        router.send(request.clone()).unwrap();
        
        // Server receives and responds
        let received = router.receive("server").unwrap().unwrap();
        let response = Message::response(&received, "server", MessagePayload::text("pong"));
        router.send(response).unwrap();
        
        // Client receives response
        let reply = router.receive("client").unwrap().unwrap();
        assert!(reply.is_response());
        assert_eq!(reply.correlation_id, correlation_id);
        assert_eq!(reply.payload.as_text().unwrap(), "pong");
    }

    #[test]
    fn test_router_channel_management() {
        let mut router = MessageRouter::new();
        
        router.create_channel(Channel::new("channel-1"));
        router.create_channel(Channel::new("channel-2"));
        
        assert_eq!(router.channel_count(), 2);
        assert!(router.get_channel("channel-1").is_some());
        
        router.delete_channel("channel-1");
        assert_eq!(router.channel_count(), 1);
    }

    #[test]
    fn test_router_unsubscribe() {
        let mut router = MessageRouter::new();
        
        router.register_agent(AgentInfo::new("agent-1", "Agent 1"));
        router.create_channel(Channel::new("events"));
        
        router.subscribe("agent-1", "events").unwrap();
        assert!(router.get_channel("events").unwrap().is_subscribed("agent-1"));
        
        router.unsubscribe("agent-1", "events").unwrap();
        assert!(!router.get_channel("events").unwrap().is_subscribed("agent-1"));
    }

    #[test]
    fn test_shared_router() {
        let router = SharedRouter::new();
        
        router.register_agent(AgentInfo::new("agent-1", "Agent 1"));
        router.register_agent(AgentInfo::new("agent-2", "Agent 2"));
        
        assert_eq!(router.agent_count(), 2);
        
        let msg = Message::new("agent-1", "agent-2", MessagePayload::text("hello"));
        router.send(msg).unwrap();
        
        let received = router.receive("agent-2").unwrap().unwrap();
        assert_eq!(received.payload.as_text().unwrap(), "hello");
    }

    #[test]
    fn test_shared_router_channels() {
        let router = SharedRouter::new();
        
        router.register_agent(AgentInfo::new("agent-1", "Agent 1"));
        router.create_channel(Channel::new("events"));
        router.subscribe("agent-1", "events").unwrap();
        
        assert_eq!(router.channel_count(), 1);
    }
}
