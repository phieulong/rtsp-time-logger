# H264 RTP Frame Logger

Raw H.264 RTP packet logger từ RTSP camera với timestamp tracking - không decode realtime.

## 📋 Features

✅ Nhận raw RTP packets từ RTSP/UDP (không decode)  
✅ Detect frame boundaries (RTP timestamp + marker bit)  
✅ Log thời điểm bắt đầu/kết thúc nhận mỗi frame  
✅ Lưu từng frame vào thư mục riêng  
✅ Detect packet loss  
✅ Offline rebuild + decode  
✅ Comprehensive logging cho debug  

## 🔧 Requirements

### System Dependencies

**macOS:**
```bash
brew install gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly ffmpeg
```

**Ubuntu/Debian:**
```bash
sudo apt-get install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
    gstreamer1.0-rtsp ffmpeg python3
```

### Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## 🚀 Build

```bash
cargo build --release
```

## 📖 Usage

### Basic Usage

```bash
# Với default URL
cargo run --release

# Với custom RTSP URL
cargo run --release -- rtsp://192.168.1.100:554/stream

# Với custom output directory
cargo run --release -- rtsp://192.168.1.100:554/stream my_frames
```

### Log Levels

```bash
# INFO level (default)
RUST_LOG=info cargo run --release

# DEBUG level (detailed frame info)
RUST_LOG=debug cargo run --release

# TRACE level (every RTP packet)
RUST_LOG=trace cargo run --release
```

## 📂 Output Structure

```
frames/
├── frame_000001/
│   ├── meta.json
│   ├── packet_0001.rtp
│   ├── packet_0002.rtp
│   └── ...
├── frame_000002/
│   ├── meta.json
│   └── ...
```

### meta.json Format

```json
{
  "frame_id": 1,
  "codec": "H264",
  "rtp_timestamp": 123456,
  "sequence_start": 1000,
  "sequence_end": 1006,
  "packet_count": 7,
  "receive_start_time": "2025-02-02T10:30:45.123Z",
  "receive_end_time": "2025-02-02T10:30:45.145Z",
  "receive_duration_ms": 22,
  "packet_loss_detected": false,
  "total_bytes": 8400,
  "payload_bytes": 8316
}
```

## 🔄 Offline Rebuild & Decode

Rebuild frame từ RTP packets và decode thành ảnh:

```bash
# Rebuild frame 1
./rebuild_frame.sh frames/frame_000001

# Custom output filename
./rebuild_frame.sh frames/frame_000001 output.png
```

Script sẽ:
1. Strip RTP headers (12 bytes mỗi packet)
2. Ghép H.264 payload → `frame.h264`
3. Decode với ffmpeg → `frame.png`

## 🐛 Debug Guide

### Không nhận được packets

**Check 1: GStreamer**
```bash
gst-launch-1.0 rtspsrc location=rtsp://YOUR_CAMERA latency=0 protocols=udp ! fakesink
```

**Check 2: Network**
```bash
# Test RTSP connection
ffplay rtsp://YOUR_CAMERA
```

**Check 3: Logs**
```bash
RUST_LOG=debug cargo run --release -- rtsp://YOUR_CAMERA
```

### Pipeline errors

Nếu thấy lỗi "no element rtpbin":
```bash
# macOS
brew reinstall gst-plugins-good

# Ubuntu
sudo apt-get install --reinstall gstreamer1.0-plugins-good
```

### RTP parsing errors

Log sẽ hiển thị:
- `RTP version != 2` - packet không phải RTP
- `RTP packet too short` - packet bị truncate
- `Sequence gap detected` - packet loss

### Frame boundary issues

- Frame không kết thúc → check marker bit trong log
- Nhiều single-packet frames → có thể không phải H.264

## 📊 Performance Tips

1. **Disk I/O**: Sử dụng SSD cho output directory
2. **CPU**: Release build nhanh hơn debug ~10x
3. **Memory**: Mỗi frame ~10-50KB, monitor với `htop`

## 🎥 Camera Settings

Để có kết quả tốt nhất:

### H.264 Settings (Required)
- ✅ Codec: H.264 / AVC
- ✅ Transport: RTP/UDP
- ✅ Profile: Baseline or Main
- ❌ KHÔNG dùng: MJPEG, H.265/HEVC

### Optional (Recommended)
- FPS: 15-30 fps
- Bitrate: 2-4 Mbps
- Resolution: 1920x1080 or lower
- I-frame interval: 30-60 frames

### Camera URL Examples

**Hikvision:**
```
rtsp://admin:password@192.168.1.100:554/Streaming/Channels/101
```

**Dahua:**
```
rtsp://admin:password@192.168.1.100:554/cam/realmonitor?channel=1&subtype=0
```

**Axis:**
```
rtsp://root:password@192.168.1.100:554/axis-media/media.amp
```

**ONVIF Generic:**
```
rtsp://user:pass@192.168.1.100:554/stream1
```

## 📝 Pipeline Details

```
rtspsrc location=RTSP_URL latency=0 protocols=udp ! appsink
```

- `rtspsrc`: RTSP client
- `latency=0`: minimum buffering
- `protocols=udp`: force UDP (không dùng TCP)
- `appsink`: receive raw buffers (không depay, không decode)

## 🔍 Troubleshooting

### "Failed to set pipeline to Playing"

Nguyên nhân:
1. Camera URL sai
2. Network không reach được camera
3. Camera yêu cầu authentication

Fix:
```bash
# Test với gstreamer trực tiếp
gst-launch-1.0 -v rtspsrc location=rtsp://YOUR_CAMERA ! fakesink

# Check với ffprobe
ffprobe -rtsp_transport udp rtsp://YOUR_CAMERA
```

### "RTP packet too short"

Nguyên nhân: Nhận được non-RTP data

Fix: Đảm bảo pipeline KHÔNG có rtph264depay

### No frames written

Check logs:
- Có nhận packets không? (`Packets received: N`)
- Có marker bit không? (`marker bit set`)
- Timestamp có đổi không?

## 📄 License

MIT

## 🙏 Credits

Built with:
- [GStreamer](https://gstreamer.freedesktop.org/)
- [Rust](https://www.rust-lang.org/)
- [FFmpeg](https://ffmpeg.org/)
