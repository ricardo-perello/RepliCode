use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BatchDirection {
    Incoming, // Consensus -> Runtime
    Outgoing, // Runtime -> Consensus
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    pub number: u64,
    pub direction: BatchDirection,
    pub data: Vec<u8>,
} 