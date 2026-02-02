use crate::rtp::{Frame, FrameMetadata};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::fs;
use std::path::PathBuf;

/// Frame writer - saves frames to disk
pub struct FrameWriter {
    output_dir: PathBuf,
    frames_written: u64,
}

impl FrameWriter {
    pub fn new(output_dir: &str) -> Result<Self> {
        let path = PathBuf::from(output_dir);

        // Create output directory
        if !path.exists() {
            fs::create_dir_all(&path)?;
            log::info!("Created output directory: {}", output_dir);
        } else {
            log::info!("Using existing output directory: {}", output_dir);
        }

        Ok(Self {
            output_dir: path,
            frames_written: 0,
        })
    }

    /// Write frame to disk
    pub fn write_frame(&mut self, frame: &Frame) -> Result<()> {
        let frame_dir = self
            .output_dir
            .join(format!("frame_{:06}", frame.frame_id));

        // Create frame directory
        fs::create_dir_all(&frame_dir)?;
        log::debug!("Writing frame {} to {:?}", frame.frame_id, frame_dir);

        // Write each RTP packet
        let mut total_bytes = 0;
        let mut payload_bytes = 0;

        for (idx, packet) in frame.packets.iter().enumerate() {
            let packet_path = frame_dir.join(format!("packet_{:04}.rtp", idx + 1));
            fs::write(&packet_path, &packet.raw_data)?;

            total_bytes += packet.raw_data.len();
            payload_bytes += packet.get_payload().len();
        }

        log::debug!(
            "Frame {}: wrote {} packets, {} total bytes, {} payload bytes",
            frame.frame_id,
            frame.packets.len(),
            total_bytes,
            payload_bytes
        );

        // Detect packet loss
        let packet_loss = self.detect_packet_loss(&frame.packets);
        if packet_loss {
            log::warn!("Frame {}: packet loss detected!", frame.frame_id);
        }

        // Create metadata
        let metadata = FrameMetadata {
            frame_id: frame.frame_id,
            codec: "H264".to_string(),
            rtp_timestamp: frame.rtp_timestamp,
            sequence_start: frame.packets.first().unwrap().sequence_number,
            sequence_end: frame.packets.last().unwrap().sequence_number,
            packet_count: frame.packets.len(),
            receive_start_time: system_time_to_utc(frame.receive_start_time),
            receive_end_time: system_time_to_utc(
                frame.receive_end_time.unwrap_or(frame.receive_start_time),
            ),
            receive_duration_ms: frame.duration_ms().unwrap_or(0),
            packet_loss_detected: packet_loss,
            total_bytes,
            payload_bytes,
        };

        // Write metadata JSON
        let meta_path = frame_dir.join("meta.json");
        let json = serde_json::to_string_pretty(&metadata)?;
        fs::write(meta_path, json)?;

        self.frames_written += 1;

        if self.frames_written % 50 == 0 {
            log::info!("Total frames written to disk: {}", self.frames_written);
        }

        Ok(())
    }

    /// Detect packet loss by checking sequence number continuity
    fn detect_packet_loss(&self, packets: &[crate::rtp::RtpPacket]) -> bool {
        if packets.len() < 2 {
            return false;
        }

        for i in 1..packets.len() {
            let expected = packets[i - 1].sequence_number.wrapping_add(1);
            if packets[i].sequence_number != expected {
                log::debug!(
                    "Packet loss: seq {} -> {} (expected {})",
                    packets[i - 1].sequence_number,
                    packets[i].sequence_number,
                    expected
                );
                return true;
            }
        }
        false
    }

    pub fn get_frames_written(&self) -> u64 {
        self.frames_written
    }
}

/// Convert SystemTime to UTC string
fn system_time_to_utc(time: std::time::SystemTime) -> String {
    let datetime: DateTime<Utc> = time.into();
    datetime.to_rfc3339()
}
