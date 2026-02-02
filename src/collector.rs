use crate::rtp::{Frame, RtpPacket};
use std::time::SystemTime;

/// Frame collector - manages frame assembly from RTP packets
pub struct FrameCollector {
    current_frame: Option<Frame>,
    frame_counter: u64,
    last_sequence: Option<u16>,
    packets_received: u64,
    frames_completed: u64,
}

impl FrameCollector {
    pub fn new() -> Self {
        log::info!("Frame collector initialized");
        Self {
            current_frame: None,
            frame_counter: 0,
            last_sequence: None,
            packets_received: 0,
            frames_completed: 0,
        }
    }

    /// Process incoming RTP packet
    /// Returns completed frame if frame boundary is detected
    pub fn process_packet(&mut self, packet: RtpPacket) -> Option<Frame> {
        self.packets_received += 1;

        // Check for sequence number gaps
        if let Some(last_seq) = self.last_sequence {
            let expected = last_seq.wrapping_add(1);
            if packet.sequence_number != expected {
                log::warn!(
                    "Sequence gap detected: expected {}, got {} (diff: {})",
                    expected,
                    packet.sequence_number,
                    packet.sequence_number.wrapping_sub(expected)
                );
            }
        }
        self.last_sequence = Some(packet.sequence_number);

        let result = match &mut self.current_frame {
            None => {
                // Start new frame
                self.start_new_frame(packet.clone());

                if packet.marker {
                    log::debug!(
                        "Single-packet frame {} (ts={})",
                        self.frame_counter,
                        packet.timestamp
                    );
                    self.finish_frame()
                } else {
                    None
                }
            }
            Some(frame) => {
                // Check if timestamp changed (new frame starts)
                if packet.timestamp != frame.rtp_timestamp {
                    log::debug!(
                        "Frame {} boundary detected: ts {} -> {}",
                        frame.frame_id,
                        frame.rtp_timestamp,
                        packet.timestamp
                    );

                    let finished = self.finish_frame();
                    self.start_new_frame(packet.clone());

                    finished
                } else {
                    // Add packet to current frame
                    frame.packets.push(packet.clone());

                    // Check marker bit (frame end)
                    if packet.marker {
                        log::debug!(
                            "Frame {} marker bit set (packets={})",
                            frame.frame_id,
                            frame.packets.len()
                        );
                        self.finish_frame()
                    } else {
                        None
                    }
                }
            }
        };

        if result.is_some() {
            self.frames_completed += 1;
            if self.frames_completed % 100 == 0 {
                log::info!(
                    "Progress: {} frames completed, {} packets received",
                    self.frames_completed,
                    self.packets_received
                );
            }
        }

        result
    }

    fn start_new_frame(&mut self, packet: RtpPacket) {
        self.frame_counter += 1;
        let now = SystemTime::now();

        log::debug!(
            "Starting frame {} (ts={}, seq={})",
            self.frame_counter,
            packet.timestamp,
            packet.sequence_number
        );

        self.current_frame = Some(Frame {
            frame_id: self.frame_counter,
            rtp_timestamp: packet.timestamp,
            packets: vec![packet],
            receive_start_time: now,
            receive_end_time: None,
        });
    }

    fn finish_frame(&mut self) -> Option<Frame> {
        if let Some(mut frame) = self.current_frame.take() {
            frame.receive_end_time = Some(SystemTime::now());

            // let duration = frame.duration_ms().unwrap_or(0);
            log::info!(
                "Frame {}: start receive at {} end at {} with {} packets",
                frame.frame_id,
                frame.receive_start_time
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
                frame.packets.len(),
            );

            return Some(frame);
        }
        None
    }

    pub fn get_stats(&self) -> (u64, u64) {
        (self.frames_completed, self.packets_received)
    }
}
