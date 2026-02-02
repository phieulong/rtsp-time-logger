mod collector;
mod rtp;
mod writer;

use anyhow::Result;
use collector::FrameCollector;
use gstreamer::prelude::*;
use gstreamer_app::{AppSink, AppSrc};
use rtp::Frame;
use std::env;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use writer::FrameWriter;

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("========================================");
    log::info!("H264 RTP Frame Logger - Starting");
    log::info!("========================================");

    // Get RTSP URL from command line or use default
    let rtsp_url = env::args()
        .nth(1)
        .unwrap_or_else(|| "rtsp://192.168.1.100:554/stream".to_string());

    log::info!("RTSP URL: {}", rtsp_url);

    // Get output directory
    let output_dir = env::args().nth(2).unwrap_or_else(|| "frames".to_string());
    log::info!("Output directory: {}", output_dir);

    // Create channel for sending frames to decoder thread
    let (frame_tx, frame_rx) = channel::<Frame>();

    // Clone output_dir for decoder thread
    let decoder_output_dir = output_dir.clone();

    // Start decoder thread
    let decoder_thread = thread::spawn(move || {
        if let Err(e) = run_decoder_thread(frame_rx, &decoder_output_dir) {
            log::error!("Decoder thread error: {}", e);
        }
    });
    log::info!("Decoder thread started");

    // Initialize GStreamer
    gstreamer::init()?;
    log::info!("GStreamer initialized");

    // Build pipeline
    // IMPORTANT: No decode, no depay - raw RTP packets only
    let pipeline_str = format!(
        "rtspsrc location={} latency=0 protocols=udp ! appsink name=sink",
        rtsp_url
    );

    log::info!("Pipeline: {}", pipeline_str);

    let pipeline = gstreamer::parse::launch(&pipeline_str)?
        .dynamic_cast::<gstreamer::Pipeline>()
        .map_err(|_| anyhow::anyhow!("Failed to cast to Pipeline"))?;
    log::info!("Pipeline created");

    // Get AppSink
    let appsink = pipeline
        .by_name("sink")
        .ok_or_else(|| anyhow::anyhow!("Failed to get appsink"))?
        .dynamic_cast::<AppSink>()
        .map_err(|_| anyhow::anyhow!("Failed to cast to AppSink"))?;

    log::info!("AppSink configured");

    // Create shared state
    let collector = Arc::new(Mutex::new(FrameCollector::new()));
    let writer = Arc::new(Mutex::new(FrameWriter::new(&output_dir)?));

    // Clone for callback
    let collector_clone = collector.clone();
    let writer_clone = writer.clone();
    let frame_tx_clone = frame_tx.clone();

    // Set up AppSink callbacks
    appsink.set_callbacks(
        gstreamer_app::AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                match process_sample(sink, &collector_clone, &writer_clone, &frame_tx_clone) {
                    Ok(_) => Ok(gstreamer::FlowSuccess::Ok),
                    Err(e) => {
                        log::error!("Sample processing error: {}", e);
                        Err(gstreamer::FlowError::Error)
                    }
                }
            })
            .build(),
    );

    // Start pipeline
    log::info!("Setting pipeline to PLAYING state...");
    pipeline
        .set_state(gstreamer::State::Playing)
        .map_err(|_| anyhow::anyhow!("Failed to set pipeline to Playing"))?;

    log::info!("Pipeline is PLAYING - receiving RTP packets...");
    log::info!("Press Ctrl+C to stop");

    // Wait for EOS or error
    let bus = pipeline
        .bus()
        .ok_or_else(|| anyhow::anyhow!("Failed to get bus"))?;

    for msg in bus.iter_timed(gstreamer::ClockTime::NONE) {
        use gstreamer::MessageView;
        match msg.view() {
            MessageView::Eos(..) => {
                log::info!("End of stream");
                break;
            }
            MessageView::Error(err) => {
                log::error!(
                    "Error from {:?}: {} ({:?})",
                    err.src().map(|s| s.path_string()),
                    err.error(),
                    err.debug()
                );
                break;
            }
            MessageView::Warning(warn) => {
                log::warn!(
                    "Warning from {:?}: {} ({:?})",
                    warn.src().map(|s| s.path_string()),
                    warn.error(),
                    warn.debug()
                );
            }
            MessageView::StateChanged(state_changed) => {
                if let Some(src) = msg.src() {
                    if src.downcast_ref::<gstreamer::Pipeline>() == Some(&pipeline) {
                        log::info!(
                            "Pipeline state: {:?} -> {:?}",
                            state_changed.old(),
                            state_changed.current()
                        );
                    }
                }
            }
            _ => (),
        }
    }

    // Cleanup
    log::info!("Shutting down...");
    pipeline
        .set_state(gstreamer::State::Null)
        .map_err(|_| anyhow::anyhow!("Failed to set pipeline to Null"))?;

    // Drop frame sender to signal decoder thread to finish
    drop(frame_tx);

    // Wait for decoder thread to finish
    log::info!("Waiting for decoder thread to finish...");
    if let Err(e) = decoder_thread.join() {
        log::error!("Decoder thread panicked: {:?}", e);
    }

    // Print final statistics
    let (frames, packets) = collector.lock().unwrap().get_stats();
    let written = writer.lock().unwrap().get_frames_written();

    log::info!("========================================");
    log::info!("Final Statistics:");
    log::info!("  Frames completed: {}", frames);
    log::info!("  Packets received: {}", packets);
    log::info!("  Frames written: {}", written);
    log::info!("========================================");

    Ok(())
}

fn process_sample(
    sink: &AppSink,
    collector: &Arc<Mutex<FrameCollector>>,
    writer: &Arc<Mutex<FrameWriter>>,
    frame_tx: &Sender<Frame>,
) -> Result<()> {
    // Pull sample from appsink
    let sample = sink
        .pull_sample()
        .map_err(|_| anyhow::anyhow!("Failed to pull sample"))?;

    // Get buffer
    let buffer = sample
        .buffer()
        .ok_or_else(|| anyhow::anyhow!("No buffer in sample"))?;

    // Map buffer for reading
    let map = buffer
        .map_readable()
        .map_err(|_| anyhow::anyhow!("Failed to map buffer"))?;

    let data = map.as_slice();

    // Parse RTP packet
    match rtp::RtpPacket::parse(data) {
        Ok(packet) => {
            // Process packet through collector
            let mut col = collector.lock().unwrap();
            if let Some(frame) = col.process_packet(packet) {
                // Frame completed - write to disk
                drop(col); // Release lock before writing

                let mut wrt = writer.lock().unwrap();
                if let Err(e) = wrt.write_frame(&frame) {
                    log::error!("Failed to write frame {}: {}", frame.frame_id, e);
                }

                // Send frame to decoder thread
                if let Err(e) = frame_tx.send(frame) {
                    log::error!("Failed to send frame to decoder thread: {}", e);
                }
            }
        }
        Err(e) => {
            log::error!("Failed to parse RTP packet: {}", e);
        }
    }

    Ok(())
}

/// Decoder thread - receives frames and decodes them to images
fn run_decoder_thread(
    frame_rx: std::sync::mpsc::Receiver<Frame>,
    output_dir: &str,
) -> Result<()> {
    log::info!("Decoder thread: initializing GStreamer...");
    gstreamer::init()?;

    // Create decoding pipeline: appsrc -> h264parse -> avdec_h264 -> videoconvert -> jpegenc -> appsink
    let pipeline_str = "appsrc name=src format=time is-live=false ! h264parse ! avdec_h264 ! videoconvert ! jpegenc ! appsink name=sink sync=false";
    log::info!("Decoder pipeline: {}", pipeline_str);

    let pipeline = gstreamer::parse::launch(pipeline_str)?
        .dynamic_cast::<gstreamer::Pipeline>()
        .map_err(|_| anyhow::anyhow!("Failed to cast to Pipeline"))?;

    let appsrc = pipeline
        .by_name("src")
        .ok_or_else(|| anyhow::anyhow!("Failed to get appsrc"))?
        .dynamic_cast::<AppSrc>()
        .map_err(|_| anyhow::anyhow!("Failed to cast to AppSrc"))?;

    let appsink = pipeline
        .by_name("sink")
        .ok_or_else(|| anyhow::anyhow!("Failed to get appsink"))?
        .dynamic_cast::<AppSink>()
        .map_err(|_| anyhow::anyhow!("Failed to cast to AppSink"))?;

    // Start pipeline
    pipeline.set_state(gstreamer::State::Playing)
        .map_err(|_| anyhow::anyhow!("Failed to set pipeline to Playing"))?;
    log::info!("Decoder pipeline started");

    let mut decoded_count = 0;
    let output_path = std::path::PathBuf::from(output_dir);

    // Process frames from channel
    while let Ok(frame) = frame_rx.recv() {
        log::debug!("Decoder: processing frame {}", frame.frame_id);

        // Reconstruct H264 data from RTP payloads (RFC 6184)
        let mut h264_data = Vec::new();
        let start_code = [0u8, 0, 0, 1];

        for packet in &frame.packets {
            let payload = packet.get_payload();
            if payload.is_empty() {
                continue;
            }

            let nalu_type = payload[0] & 0x1F;

            if nalu_type >= 1 && nalu_type <= 23 {
                // Single NAL unit packet
                h264_data.extend_from_slice(&start_code);
                h264_data.extend_from_slice(payload);
            } else if nalu_type == 28 {
                // FU-A fragmentation unit
                if payload.len() < 2 {
                    continue;
                }
                let fu_header = payload[1];
                let start_bit = (fu_header & 0x80) != 0;
                let actual_nalu_type = fu_header & 0x1F;
                let nri = (payload[0] & 0x60) >> 5;
                let first_byte = (nri << 5) | actual_nalu_type;

                if start_bit {
                    h264_data.extend_from_slice(&start_code);
                    h264_data.push(first_byte);
                }
                h264_data.extend_from_slice(&payload[2..]);
            } else if nalu_type == 24 {
                // STAP-A (Single-time aggregation packet)
                let mut pos = 1;
                while pos + 2 <= payload.len() {
                    let size = u16::from_be_bytes([payload[pos], payload[pos + 1]]) as usize;
                    pos += 2;
                    if pos + size <= payload.len() {
                        h264_data.extend_from_slice(&start_code);
                        h264_data.extend_from_slice(&payload[pos..pos + size]);
                        pos += size;
                    } else {
                        break;
                    }
                }
            } else {
                log::warn!("Unsupported NALU type: {}", nalu_type);
            }
        }

        log::debug!(
            "Decoder: frame {} reconstructed {} bytes from {} packets",
            frame.frame_id,
            h264_data.len(),
            frame.packets.len()
        );

        // Push H264 data to appsrc
        let buffer = gstreamer::Buffer::from_slice(h264_data);
        
        if let Err(e) = appsrc.push_buffer(buffer) {
            log::error!("Failed to push buffer for frame {}: {:?}", frame.frame_id, e);
            continue;
        }

        // Pull decoded images (may be multiple or none due to decoding delay)
        while let Some(sample) = appsink.try_pull_sample(gstreamer::ClockTime::ZERO) {
            if let Some(buffer) = sample.buffer() {
                if let Ok(map) = buffer.map_readable() {
                    let jpeg_data = map.as_slice();
                    decoded_count += 1;
                    let image_path = output_path.join(format!("frame_{:06}.jpg", decoded_count));
                    if let Err(e) = std::fs::write(&image_path, jpeg_data) {
                        log::error!("Failed to write image {}: {}", image_path.display(), e);
                    } else {
                        log::info!(
                            "Decoded image {} -> {} ({} bytes)",
                            decoded_count,
                            image_path.display(),
                            jpeg_data.len()
                        );
                    }
                }
            }
        }
    }

    // Cleanup decoder pipeline
    log::info!("Decoder thread: finishing up...");
    let _ = appsrc.end_of_stream();

    pipeline.set_state(gstreamer::State::Null)
        .map_err(|_| anyhow::anyhow!("Failed to set pipeline to Null"))?;

    log::info!("Decoder thread: decoded {} images", decoded_count);
    Ok(())
}
