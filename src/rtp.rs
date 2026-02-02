use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// RTP Packet structure
#[derive(Debug, Clone)]
pub struct RtpPacket {
    pub sequence_number: u16,
    pub timestamp: u32,
    pub marker: bool,
    #[allow(dead_code)]
    pub payload_type: u8,
    pub raw_data: Vec<u8>,
}

impl RtpPacket {
    /// Parse RTP packet from raw bytes
    /// RTP Header format (minimum 12 bytes):
    /// 0-1: V(2), P(1), X(1), CC(4), M(1), PT(7)
    /// 2-3: Sequence Number
    /// 4-7: Timestamp
    /// 8-11: SSRC
    pub fn parse(data: &[u8]) -> anyhow::Result<Self> {
        if data.len() < 12 {
            anyhow::bail!("RTP packet too short: {} bytes", data.len());
        }

        // Byte 0: V(2), P(1), X(1), CC(4)
        let version = (data[0] >> 6) & 0x03;
        if version != 2 {
            log::warn!("RTP version {} != 2", version);
        }

        // Byte 1: M(1), PT(7)
        let marker = (data[1] & 0x80) != 0;
        let payload_type = data[1] & 0x7F;

        // Bytes 2-3: Sequence Number
        let sequence_number = u16::from_be_bytes([data[2], data[3]]);

        // Bytes 4-7: Timestamp
        let timestamp = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

        log::trace!(
            "RTP: seq={}, ts={}, marker={}, pt={}, size={}",
            sequence_number,
            timestamp,
            marker,
            payload_type,
            data.len()
        );

        Ok(RtpPacket {
            sequence_number,
            timestamp,
            marker,
            payload_type,
            raw_data: data.to_vec(),
        })
    }

    /// Get RTP payload (skip header)
    pub fn get_payload(&self) -> &[u8] {
        // Basic RTP header is 12 bytes
        // TODO: Handle CSRC and extension headers if needed
        if self.raw_data.len() > 12 {
            &self.raw_data[12..]
        } else {
            &[]
        }
    }
}

/// Frame structure - collection of RTP packets with same timestamp
#[derive(Debug)]
pub struct Frame {
    pub frame_id: u64,
    pub rtp_timestamp: u32,
    pub packets: Vec<RtpPacket>,
    pub receive_start_time: SystemTime,
    pub receive_end_time: Option<SystemTime>,
}

impl Frame {
    pub fn duration_ms(&self) -> Option<u128> {
        self.receive_end_time.and_then(|end| {
            end.duration_since(self.receive_start_time)
                .ok()
                .map(|d| d.as_millis())
        })
    }
}

/// Frame metadata for JSON output
#[derive(Debug, Serialize, Deserialize)]
pub struct FrameMetadata {
    pub frame_id: u64,
    pub codec: String,
    pub rtp_timestamp: u32,
    pub sequence_start: u16,
    pub sequence_end: u16,
    pub packet_count: usize,
    pub receive_start_time: String,
    pub receive_end_time: String,
    pub receive_duration_ms: u128,
    pub packet_loss_detected: bool,
    pub total_bytes: usize,
    pub payload_bytes: usize,
}
