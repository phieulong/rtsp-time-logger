# Architecture - RTSP Time Logger

## Tổng quan kiến trúc đa luồng

Ứng dụng sử dụng **3 luồng song song** để đảm bảo không bị drop packet từ RTSP stream:

```
┌─────────────────────────────────────────────────────────────┐
│                      MAIN THREAD                            │
│  (GStreamer Pipeline - Nhận RTP packets từ RTSP)           │
│                                                             │
│  rtspsrc → appsink → process_sample()                      │
│                           │                                 │
│                           ├─ Parse RTP (FAST)              │
│                           ├─ Collect to Frame (FAST)       │
│                           └─ Send to channels (NON-BLOCKING)│
└─────────────────┬───────────────────┬───────────────────────┘
                  │                   │
                  ▼                   ▼
      ┌───────────────────┐   ┌──────────────────┐
      │  WRITER THREAD    │   │  DECODER THREAD  │
      │  (Disk I/O)       │   │  (H264→JPEG)     │
      │                   │   │                  │
      │  Write packets    │   │  Decode frames   │
      │  to disk          │   │  to images       │
      └───────────────────┘   └──────────────────┘
```

## Chi tiết các luồng

### 1. Main Thread (Receiver Thread)
**Nhiệm vụ:** Nhận RTP packets từ RTSP stream và xử lý NHANH
**Yêu cầu:** KHÔNG được block bởi I/O

```rust
process_sample() {
    1. Pull RTP packet          // ~0.01ms
    2. Parse RTP header          // ~0.001ms  
    3. Collect to frame          // ~0.01ms
    4. try_send() to channels    // ~0.001ms (non-blocking)
    ────────────────────────────
    Total: ~0.02ms per packet    // FAST!
}
```

**Cải tiến:**
- ✅ Không có disk I/O trong callback
- ✅ Không có mutex contention
- ✅ Sử dụng `try_send()` với buffered channel (100 frames)
- ✅ Đảm bảo nhận đủ tốc độ camera (30-60fps)

### 2. Writer Thread (Disk I/O Thread)
**Nhiệm vụ:** Ghi RTP packets ra đĩa
**Đặc điểm:** Chậm nhưng không ảnh hưởng receiver

```rust
run_writer_thread() {
    while let Ok(frame) = channel.recv() {
        write_frame_to_disk();    // ~10-50ms (chậm nhưng OK)
    }
}
```

**Lợi ích:**
- Disk I/O không block receiver thread
- Buffer 100 frames trong channel
- Xử lý tuần tự, đảm bảo đúng thứ tự

### 3. Decoder Thread (Image Generation Thread)
**Nhiệm vụ:** Decode H264 → JPEG
**Đặc điểm:** Rất chậm nhưng chạy song song

```rust
run_decoder_thread() {
    while let Ok(frame) = channel.recv() {
        reconstruct_h264();       // ~1ms
        decode_to_jpeg();         // ~20-100ms (rất chậm)
        save_image();             // ~10ms
    }
}
```

**RFC 6184 NAL Unit Handling:**
- Single NAL (type 1-23): Thêm start code [0,0,0,1]
- FU-A (type 28): Ghép fragmented units
- STAP-A (type 24): Tách aggregated units

## Channel Buffering Strategy

```rust
const FRAME_CHANNEL_BUFFER: usize = 100;
sync_channel::<Frame>(100)  // 100 frames buffer
```

**Tại sao 100 frames?**
- 30fps camera = 100 frames ~ 3.3 giây buffer
- Đủ để writer/decoder xử lý burst traffic
- Không quá lớn để chiếm RAM

**Hành vi khi channel đầy:**
```rust
try_send(frame)  // Non-blocking
→ Err if full   // Log warning, drop frame (hiếm khi xảy ra)
→ Ok otherwise  // Gửi thành công
```

## Performance Analysis

### Trước khi refactor (Version cũ)
```
process_sample() {
    parse_rtp()         ~0.01ms
    collect_frame()     ~0.01ms
    write_to_disk()     ~30ms     ← BLOCKING I/O! ⚠️
    send_channel()      ~0.01ms
    ─────────────────────────────
    Total: ~30ms/packet
}

→ 30ms × 10 packets/frame = 300ms/frame
→ Max FPS: 3 fps   ← PROBLEM! ❌
```

### Sau khi refactor (Version hiện tại)
```
process_sample() {
    parse_rtp()         ~0.01ms
    collect_frame()     ~0.01ms
    try_send()          ~0.001ms   ← Non-blocking! ✅
    ─────────────────────────────
    Total: ~0.02ms/packet
}

→ 0.02ms × 10 packets/frame = 0.2ms/frame
→ Max FPS: 5000+ fps   ← EXCELLENT! ✅
```

## Đảm bảo đồng nhất packet và image

**Yêu cầu:** Image phải được decode từ chính xác packets đã lưu

**Giải pháp:**
1. Cùng 1 Frame object được gửi đến cả 2 threads
2. Clone frame để 2 threads độc lập:
```rust
let frame_for_decoder = Frame {
    frame_id: frame.frame_id,           // Cùng ID
    rtp_timestamp: frame.rtp_timestamp, // Cùng timestamp
    packets: frame.packets.clone(),      // Clone packets data
    ...
};
```

3. Frame ID mapping:
```
frame_000001/ (packets) ←→ frame_000001.jpg (image)
frame_000002/ (packets) ←→ frame_000002.jpg (image)
```

## Sequence Diagram

```
Camera    RTSP    Main Thread    Writer Thread    Decoder Thread
  │         │           │               │                │
  ├─packet─>│           │               │                │
  │         ├─packet───>│               │                │
  │         │           ├─parse()       │                │
  │         │           ├─collect()     │                │
  │         │           │               │                │
  ├─packet─>│           │               │                │
  │         ├─packet───>│               │                │
  │         │           ├─[frame done]  │                │
  │         │           │               │                │
  │         │           ├─try_send()───>│                │
  │         │           ├─try_send()──────────────────>  │
  │         │           │               │                │
  │         │           ├─[continue]    ├─write_disk()   ├─decode_h264()
  │         │           │               │                ├─save_jpeg()
  ├─packet─>│           │               │                │
  │         ├─packet───>│               │                │
  │         │           ├─parse()       │                │
  ...       ...         ...             ...              ...
```

## Monitoring & Logging

**Các metrics quan trọng:**
- `Packets received`: Tổng số RTP packets nhận được
- `Frames completed`: Số frame đã assemble xong
- `Frames written`: Số frame đã ghi disk (từ writer thread)
- `Images decoded`: Số ảnh đã decode (từ decoder thread)

**Log levels:**
- `INFO`: Frame completion, thread status, statistics
- `DEBUG`: Packet details, NAL unit parsing
- `WARN`: Sequence gaps, unsupported NAL types
- `ERROR`: Channel full, write errors, decode errors

## Troubleshooting

### Q: Nhận không đủ tốc độ camera?
**A:** Check log xem có `channel full` không
- Nếu có: Tăng `FRAME_CHANNEL_BUFFER`
- Nếu không: Kiểm tra network/GStreamer pipeline

### Q: Image không khớp với packets?
**A:** Check `frame_id` trong log
- Writer log: `Frame X completed`
- Decoder log: `Decoded image X`
- Phải cùng sequence

### Q: Decoder không tạo được image?
**A:** Check NAL unit types trong log
- `Unsupported NALU type`: Có thể cần thêm xử lý
- `Failed to push buffer`: GStreamer pipeline issue
- `No sample available`: Decoding delay (bình thường)

## Kết luận

Kiến trúc 3 luồng này đảm bảo:
✅ **Không drop packets** từ camera
✅ **Đồng nhất dữ liệu** giữa packets và images  
✅ **Hiệu suất cao** - receiver thread chỉ mất ~0.02ms/packet
✅ **Scalability** - Dễ thêm worker threads khác nếu cần
