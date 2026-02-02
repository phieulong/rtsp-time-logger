#!/bin/bash
# run_example.sh - Test với mock RTSP stream

set -e

echo "=========================================="
echo "H264 RTP Frame Logger - Example Run"
echo "=========================================="
echo ""

# Check if GStreamer is installed
if ! command -v gst-launch-1.0 &> /dev/null; then
    echo "❌ Error: GStreamer not found"
    echo ""
    echo "Install GStreamer:"
    echo "  macOS: brew install gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly"
    echo "  Ubuntu: sudo apt-get install gstreamer1.0-tools gstreamer1.0-plugins-good gstreamer1.0-plugins-bad"
    exit 1
fi

echo "✅ GStreamer found"

# Build project
echo ""
echo "Building project..."
cargo build --release

if [ $? -ne 0 ]; then
    echo "❌ Build failed"
    exit 1
fi

echo "✅ Build successful"
echo ""

# Show usage
echo "=========================================="
echo "Usage Examples:"
echo "=========================================="
echo ""
echo "1. Test with your camera:"
echo "   ./target/release/rtsp-time-logger rtsp://admin:password@192.168.1.100:554/stream"
echo ""
echo "2. Test with custom output dir:"
echo "   ./target/release/rtsp-time-logger rtsp://YOUR_CAMERA my_frames"
echo ""
echo "3. Debug mode:"
echo "   RUST_LOG=debug ./target/release/rtsp-time-logger rtsp://YOUR_CAMERA"
echo ""
echo "4. Trace mode (all packets):"
echo "   RUST_LOG=trace ./target/release/rtsp-time-logger rtsp://YOUR_CAMERA"
echo ""
echo "=========================================="
echo "Rebuild a frame:"
echo "=========================================="
echo ""
echo "   ./rebuild_frame.sh frames/frame_000001"
echo ""
echo "=========================================="
echo ""

# Ask if user wants to test with a camera
read -p "Do you have an RTSP camera URL to test? (y/n): " -n 1 -r
echo ""

if [[ $REPLY =~ ^[Yy]$ ]]; then
    read -p "Enter RTSP URL: " RTSP_URL

    if [ -z "$RTSP_URL" ]; then
        echo "❌ No URL provided"
        exit 1
    fi

    echo ""
    echo "Testing connection with GStreamer..."
    timeout 5 gst-launch-1.0 rtspsrc location="$RTSP_URL" latency=0 protocols=udp ! fakesink 2>&1 | head -20 || true

    echo ""
    read -p "Connection looks good? Start logging? (y/n): " -n 1 -r
    echo ""

    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo ""
        echo "Starting logger (Ctrl+C to stop)..."
        echo ""
        RUST_LOG=info ./target/release/rtsp-time-logger "$RTSP_URL"
    fi
else
    echo ""
    echo "To test later, use:"
    echo "  ./target/release/rtsp-time-logger rtsp://YOUR_CAMERA"
fi
