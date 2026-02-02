#!/bin/bash
# rebuild_frame.sh - Offline rebuild and decode H.264 frame

set -e

if [ $# -lt 1 ]; then
    echo "Usage: $0 <frame_directory> [output_image]"
    echo "Example: $0 frames/frame_000001 output.png"
    exit 1
fi

FRAME_DIR=$1
OUTPUT_IMAGE=${2:-"frame.png"}

if [ ! -d "$FRAME_DIR" ]; then
    echo "Error: Directory $FRAME_DIR does not exist"
    exit 1
fi

echo "=========================================="
echo "H.264 Frame Rebuild & Decode"
echo "=========================================="
echo "Frame directory: $FRAME_DIR"
echo "Output image: $OUTPUT_IMAGE"
echo ""

cd "$FRAME_DIR"

# Check if packets exist
PACKET_COUNT=$(ls -1 packet_*.rtp 2>/dev/null | wc -l)
if [ $PACKET_COUNT -eq 0 ]; then
    echo "Error: No RTP packets found in $FRAME_DIR"
    exit 1
fi

echo "Found $PACKET_COUNT RTP packets"

# Show metadata
if [ -f "meta.json" ]; then
    echo ""
    echo "Metadata:"
    cat meta.json
    echo ""
fi

# Step 1: Strip RTP headers and extract H.264 payload
echo "Step 1: Stripping RTP headers (12 bytes each)..."

python3 << 'EOF'
import os
import glob
import struct

# Get all packet files, sorted by name
packet_files = sorted(glob.glob('packet_*.rtp'))
print(f"Processing {len(packet_files)} packets...")

h264_data = bytearray()
total_rtp_bytes = 0
total_payload_bytes = 0

for pkt_file in packet_files:
    with open(pkt_file, 'rb') as f:
        data = f.read()
        total_rtp_bytes += len(data)

        if len(data) < 12:
            print(f"Warning: {pkt_file} too short ({len(data)} bytes)")
            continue

        # RTP header is minimum 12 bytes
        # For simplicity, we skip first 12 bytes
        # Production code should parse CSRC count and extensions
        payload = data[12:]
        h264_data.extend(payload)
        total_payload_bytes += len(payload)

# Write H.264 stream
with open('frame.h264', 'wb') as f:
    f.write(h264_data)

print(f"Total RTP bytes: {total_rtp_bytes}")
print(f"Total H.264 payload: {total_payload_bytes}")
print(f"Header overhead: {total_rtp_bytes - total_payload_bytes} bytes")
print(f"Written: frame.h264 ({len(h264_data)} bytes)")
EOF

if [ ! -f "frame.h264" ]; then
    echo "Error: Failed to create frame.h264"
    exit 1
fi

H264_SIZE=$(wc -c < frame.h264)
echo "H.264 stream: $H264_SIZE bytes"
echo ""

# Step 2: Decode H.264 to image
echo "Step 2: Decoding H.264 to image with ffmpeg..."

if ! command -v ffmpeg &> /dev/null; then
    echo "Warning: ffmpeg not found. Skipping decode."
    echo "Install ffmpeg: brew install ffmpeg (macOS)"
    exit 0
fi

# Decode with ffmpeg
# -f h264: input format is raw H.264
# -i frame.h264: input file
# -frames:v 1: extract only first frame
ffmpeg -loglevel warning -f h264 -i frame.h264 -frames:v 1 "$OUTPUT_IMAGE" -y

if [ -f "$OUTPUT_IMAGE" ]; then
    echo ""
    echo "✅ Success!"
    echo "Output: $OUTPUT_IMAGE"
    ls -lh "$OUTPUT_IMAGE"
else
    echo ""
    echo "❌ Decode failed"
    exit 1
fi

echo ""
echo "=========================================="
echo "Files created:"
echo "  - frame.h264 (raw H.264 stream)"
echo "  - $OUTPUT_IMAGE (decoded image)"
echo "=========================================="
