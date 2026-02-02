#!/bin/bash
# rebuild_frame.sh - Offline rebuild and decode H.264 frame

set -e

if [ $# -lt 1 ]; then
    echo "Usage: $0 <frame_directory> [output_image] [sps_pps_frame]"
    echo "Example: $0 frames/frame_000001 output.png"
    echo "Example: $0 frames/frame_000002 output.png frames/frame_000001"
    exit 1
fi

FRAME_DIR=$1
OUTPUT_IMAGE=${2:-"frame.png"}
SPS_PPS_FRAME=${3:-""}

if [ ! -d "$FRAME_DIR" ]; then
    echo "Error: Directory $FRAME_DIR does not exist"
    exit 1
fi

# Convert paths to absolute paths before changing directories
FRAME_DIR_ABS=$(cd "$FRAME_DIR" && pwd)
if [ -n "$SPS_PPS_FRAME" ] && [ -d "$SPS_PPS_FRAME" ]; then
    SPS_PPS_FRAME_ABS=$(cd "$SPS_PPS_FRAME" && pwd)
else
    SPS_PPS_FRAME_ABS=""
fi

echo "=========================================="
echo "H.264 Frame Rebuild & Decode"
echo "=========================================="
echo "Frame directory: $FRAME_DIR"
echo "Output image: $OUTPUT_IMAGE"
if [ -n "$SPS_PPS_FRAME_ABS" ]; then
    echo "SPS/PPS source: $SPS_PPS_FRAME"
fi
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

# Step 1: Strip RTP headers and extract H.264 payload with proper NAL handling
echo "Step 1: Processing H.264 RTP payloads..."

python3 << EOF
import os
import glob
import struct
import sys

sps_pps_frame = "${SPS_PPS_FRAME_ABS}"

def process_packets(directory):
    """Process RTP packets and extract NAL units"""
    os.chdir(directory)
    packet_files = sorted(glob.glob('packet_*.rtp'))

    h264_data = bytearray()
    total_rtp_bytes = 0
    total_payload_bytes = 0
    current_nal = bytearray()
    nal_count = 0
    fu_a_count = 0
    single_nal_count = 0
    sps_data = None
    pps_data = None

    for pkt_file in packet_files:
        with open(pkt_file, 'rb') as f:
            data = f.read()
            total_rtp_bytes += len(data)

            if len(data) < 12:
                continue

            payload = data[12:]
            total_payload_bytes += len(payload)

            if len(payload) < 1:
                continue

            # H.264 RTP Payload format (RFC 6184)
            nal_header = payload[0]
            nal_type = nal_header & 0x1F

            if nal_type == 28:  # FU-A (Fragmentation Unit)
                fu_a_count += 1
                if len(payload) < 2:
                    continue
                fu_header = payload[1]
                start_bit = (fu_header >> 7) & 1
                end_bit = (fu_header >> 6) & 1
                fu_nal_type = fu_header & 0x1F

                if start_bit:
                    if len(current_nal) > 0:
                        h264_data.extend(b'\x00\x00\x00\x01')
                        h264_data.extend(current_nal)
                        nal_count += 1
                        current_nal = bytearray()

                    reconstructed_nal_header = (nal_header & 0xE0) | fu_nal_type
                    current_nal.append(reconstructed_nal_header)
                    current_nal.extend(payload[2:])
                elif end_bit:
                    current_nal.extend(payload[2:])
                    h264_data.extend(b'\x00\x00\x00\x01')
                    h264_data.extend(current_nal)
                    nal_count += 1
                    current_nal = bytearray()
                else:
                    current_nal.extend(payload[2:])

            elif nal_type > 0 and nal_type < 24:  # Single NAL unit
                single_nal_count += 1
                if len(current_nal) > 0:
                    h264_data.extend(b'\x00\x00\x00\x01')
                    h264_data.extend(current_nal)
                    nal_count += 1
                    current_nal = bytearray()

                # Save SPS and PPS for later use
                if nal_type == 7:
                    sps_data = payload
                elif nal_type == 8:
                    pps_data = payload

                h264_data.extend(b'\x00\x00\x00\x01')
                h264_data.extend(payload)
                nal_count += 1

    if len(current_nal) > 0:
        h264_data.extend(b'\x00\x00\x00\x01')
        h264_data.extend(current_nal)
        nal_count += 1

    return {
        'data': h264_data,
        'total_rtp_bytes': total_rtp_bytes,
        'total_payload_bytes': total_payload_bytes,
        'fu_a_count': fu_a_count,
        'single_nal_count': single_nal_count,
        'nal_count': nal_count,
        'sps': sps_data,
        'pps': pps_data
    }

# Process current frame
current_dir = os.getcwd()
print(f"Processing {len(glob.glob('packet_*.rtp'))} packets...")
result = process_packets('.')

# Check if we need to prepend SPS/PPS from another frame
final_data = bytearray()
if sps_pps_frame and os.path.isdir(sps_pps_frame):
    print(f"Loading SPS/PPS from: {sps_pps_frame}")
    ref_result = process_packets(sps_pps_frame)
    os.chdir(current_dir)

    if ref_result['sps'] or ref_result['pps']:
        if ref_result['sps']:
            final_data.extend(b'\x00\x00\x00\x01')
            final_data.extend(ref_result['sps'])
            print(f"  Added SPS ({len(ref_result['sps'])} bytes)")
        if ref_result['pps']:
            final_data.extend(b'\x00\x00\x00\x01')
            final_data.extend(ref_result['pps'])
            print(f"  Added PPS ({len(ref_result['pps'])} bytes)")

final_data.extend(result['data'])

# Write H.264 stream
with open('frame.h264', 'wb') as f:
    f.write(final_data)

print(f"Total RTP bytes: {result['total_rtp_bytes']}")
print(f"Total H.264 payload: {result['total_payload_bytes']}")
print(f"Header overhead: {result['total_rtp_bytes'] - result['total_payload_bytes']} bytes")
print(f"FU-A packets: {result['fu_a_count']}, Single NAL packets: {result['single_nal_count']}")
print(f"NAL units reconstructed: {result['nal_count']}")
print(f"Written: frame.h264 ({len(final_data)} bytes)")
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
