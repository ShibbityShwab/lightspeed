/// Criterion benchmarks for the relay hot path (in-memory, no I/O).
///
/// The relay's per-packet CPU work is:
///   1. TunnelHeader::decode_with_payload  (every packet)
///   2. FecHeader::decode                  (FEC packets only)
///   3. FecEncoder::add_packet             (outbound FEC generation)
///   4. TunnelHeader::encode_with_payload  (response wrapping)
///
/// We isolate these steps so we know exactly where cycles are spent.
use bytes::{Bytes, BytesMut};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use lightspeed_protocol::{FecEncoder, FecHeader, TunnelHeader, FEC_HEADER_SIZE, HEADER_SIZE};
use std::net::{Ipv4Addr, SocketAddrV4};

fn make_src() -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345)
}
fn make_dst() -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777)
}

// ─────────────────────────────────────────────────────────────────────────────
// Benchmark 1: Full inbound v1 (non-FEC) packet decode
// ─────────────────────────────────────────────────────────────────────────────
fn bench_inbound_v1_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("relay/inbound_v1");

    for &payload_size in &[64usize, 256, 512, 1024] {
        // Pre-build a wire-format v1 packet: [TunnelHeader][payload]
        let header =
            TunnelHeader::new(1, 1_000_000, make_src(), make_dst()).with_session_token(0x42);
        let payload = vec![0xAAu8; payload_size];
        let wire_packet = header.encode_with_payload(&payload);

        group.throughput(Throughput::Bytes(wire_packet.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{payload_size}B")),
            &wire_packet,
            |b, pkt| {
                b.iter(|| {
                    let (_hdr, _payload) =
                        black_box(TunnelHeader::decode_with_payload(black_box(pkt)).unwrap());
                })
            },
        );
    }

    group.finish();
}

// ─────────────────────────────────────────────────────────────────────────────
// Benchmark 2: Full inbound v2 (FEC) packet decode (TunnelHeader + FecHeader)
// ─────────────────────────────────────────────────────────────────────────────
fn bench_inbound_v2_fec_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("relay/inbound_v2_fec");

    for &payload_size in &[64usize, 256, 512, 1024] {
        let header = TunnelHeader::new_fec(1, 1_000_000, make_src(), make_dst());
        let fec_hdr = FecHeader::data(0, 2, 4);
        let payload = vec![0xBBu8; payload_size];

        // Build: [TunnelHeader v2][FecHeader][payload]
        let mut buf = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + payload_size);
        buf.extend_from_slice(&header.encode());
        fec_hdr.encode(&mut buf);
        buf.extend_from_slice(&payload);
        let wire_packet: Bytes = buf.freeze();

        group.throughput(Throughput::Bytes(wire_packet.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{payload_size}B")),
            &wire_packet,
            |b, pkt| {
                b.iter(|| {
                    let (hdr, rest) =
                        black_box(TunnelHeader::decode_with_payload(black_box(pkt)).unwrap());
                    debug_assert!(hdr.has_fec());
                    let mut fec_slice: &[u8] = &rest[..FEC_HEADER_SIZE];
                    let _fec = black_box(FecHeader::decode(&mut fec_slice).unwrap());
                    let _game_payload = &rest[FEC_HEADER_SIZE..];
                })
            },
        );
    }

    group.finish();
}

// ─────────────────────────────────────────────────────────────────────────────
// Benchmark 3: Outbound v1 response — encode header + payload (relay → client)
// ─────────────────────────────────────────────────────────────────────────────
fn bench_outbound_v1_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("relay/outbound_v1");

    for &payload_size in &[64usize, 256, 512, 1024] {
        let payload = vec![0xCCu8; payload_size];
        let header = TunnelHeader::new(1, 1_000_000, make_dst(), make_src());

        group.throughput(Throughput::Bytes((HEADER_SIZE + payload_size) as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{payload_size}B")),
            &payload,
            |b, p| b.iter(|| black_box(header.encode_with_payload(black_box(p)))),
        );
    }

    group.finish();
}

// ─────────────────────────────────────────────────────────────────────────────
// Benchmark 4: Outbound FEC encode — the response listener's hot loop
// Simulates: encode header + FEC header + payload, feed into FEC encoder
// at K = 4 (default) with 256-byte game responses.
// ─────────────────────────────────────────────────────────────────────────────
fn bench_outbound_fec_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("relay/outbound_fec");

    for &k in &[2u8, 4, 8] {
        for &payload_size in &[64usize, 256, 512] {
            let payload = vec![0xDDu8; payload_size];

            group.throughput(Throughput::Bytes(
                (HEADER_SIZE + FEC_HEADER_SIZE + payload_size) as u64,
            ));
            group.bench_with_input(
                BenchmarkId::new(format!("K{k}"), format!("{payload_size}B")),
                &(k, &payload),
                |b, &(k, p)| {
                    b.iter_with_setup(
                        || FecEncoder::new(k),
                        |mut enc| {
                            let block_id = enc.block_id();
                            let index = enc.current_index();

                            // Build: [TunnelHeader v2][FecHeader][payload]
                            let response_hdr =
                                TunnelHeader::new_fec(1, 1_000_000, make_dst(), make_src());
                            let fec_hdr = FecHeader::data(block_id, index, k);

                            let mut buf =
                                BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + p.len());
                            buf.extend_from_slice(&response_hdr.encode());
                            fec_hdr.encode(&mut buf);
                            buf.extend_from_slice(p);

                            let parity = enc.add_packet(black_box(p));
                            black_box((buf.freeze(), parity))
                        },
                    )
                },
            );
        }
    }

    group.finish();
}

// ─────────────────────────────────────────────────────────────────────────────
// Benchmark 5: Full relay round-trip (decode inbound → encode outbound)
// This is the closest approximation to the actual per-packet CPU budget.
// ─────────────────────────────────────────────────────────────────────────────
fn bench_relay_round_trip_v1(c: &mut Criterion) {
    let mut group = c.benchmark_group("relay/round_trip_v1");

    for &payload_size in &[64usize, 256, 512, 1024] {
        // Build: inbound wire packet from client
        let inbound_header =
            TunnelHeader::new(1, 1_000_000, make_src(), make_dst()).with_session_token(0x42);
        let game_payload = vec![0xEEu8; payload_size];
        let inbound_wire = inbound_header.encode_with_payload(&game_payload);

        group.throughput(Throughput::Bytes(inbound_wire.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{payload_size}B")),
            &inbound_wire,
            |b, wire| {
                b.iter(|| {
                    // INBOUND: decode client packet
                    let (_hdr, payload) =
                        TunnelHeader::decode_with_payload(black_box(wire)).unwrap();

                    // OUTBOUND: wrap game server response back to client
                    let response_hdr = TunnelHeader::new(2, 2_000_000, make_dst(), make_src());
                    black_box(response_hdr.encode_with_payload(payload))
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    relay_benches,
    bench_inbound_v1_decode,
    bench_inbound_v2_fec_decode,
    bench_outbound_v1_encode,
    bench_outbound_fec_encode,
    bench_relay_round_trip_v1,
);
criterion_main!(relay_benches);
