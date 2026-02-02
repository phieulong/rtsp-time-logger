mod collector;
mod rtp;
mod writer;

use anyhow::Result;
use collector::FrameCollector;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;
use std::env;
use std::sync::{Arc, Mutex};
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

    // Set up AppSink callbacks
    appsink.set_callbacks(
        gstreamer_app::AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                match process_sample(sink, &collector_clone, &writer_clone) {
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
            }
        }
        Err(e) => {
            log::error!("Failed to parse RTP packet: {}", e);
        }
    }

    Ok(())
}
