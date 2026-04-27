/// Criterion benchmarks for TunnelHeader encode/decode.
///
/// Measures the throughput of the hot-path header operations that every
/// tunnel packet passes through on both the client and proxy sides.
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use lightspeed_protocol::TunnelHeader;
use std::net::{Ipv4Addr, SocketAddrV4};

fn bench_header_encode(c: &mut Criterion) {
    let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
    let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
    let header = TunnelHeader::new(42, 1_000_000, src, dst).with_session_token(0xAB);

    let mut group = c.benchmark_group("header");
    group.throughput(Throughput::Elements(1));

    group.bench_function("encode", |b| {
        b.iter(|| black_box(header.encode()))
    });

    // Zero-alloc stack variant — avoids Bytes heap allocation entirely.
    // Expected: ~7× faster than encode() (~5 ns vs ~38 ns).
    group.bench_function("encode_to_array", |b| {
        b.iter(|| black_box(header.encode_to_array()))
    });

    group.finish();
}

fn bench_header_decode(c: &mut Criterion) {
    let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
    let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
    let header = TunnelHeader::new(42, 1_000_000, src, dst).with_session_token(0xAB);
    let encoded = header.encode();

    let mut group = c.benchmark_group("header");
    group.throughput(Throughput::Elements(1));

    group.bench_function("decode", |b| {
        b.iter(|| black_box(TunnelHeader::decode(black_box(&encoded)).unwrap()))
    });

    group.finish();
}

fn bench_header_encode_with_payload(c: &mut Criterion) {
    let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
    let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
    let header = TunnelHeader::new(42, 1_000_000, src, dst).with_session_token(0xAB);

    let mut group = c.benchmark_group("header_with_payload");

    // Benchmark common game packet sizes
    for payload_size in [64usize, 256, 512, 1024] {
        let payload = vec![0xAAu8; payload_size];
        group.throughput(Throughput::Bytes((20 + payload_size) as u64));
        group.bench_with_input(
            format!("encode+payload_{payload_size}B"),
            &payload,
            |b, p| b.iter(|| black_box(header.encode_with_payload(black_box(p)))),
        );
    }

    group.finish();
}

fn bench_header_decode_with_payload(c: &mut Criterion) {
    let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
    let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
    let header = TunnelHeader::new(42, 1_000_000, src, dst).with_session_token(0xAB);

    let mut group = c.benchmark_group("header_with_payload");

    for payload_size in [64usize, 256, 512, 1024] {
        let payload = vec![0xAAu8; payload_size];
        let packet = header.encode_with_payload(&payload);
        group.throughput(Throughput::Bytes((20 + payload_size) as u64));
        group.bench_with_input(
            format!("decode+payload_{payload_size}B"),
            &packet,
            |b, pkt| b.iter(|| black_box(TunnelHeader::decode_with_payload(black_box(pkt)).unwrap())),
        );
    }

    group.finish();
}

criterion_group!(
    header_benches,
    bench_header_encode,
    bench_header_decode,
    bench_header_encode_with_payload,
    bench_header_decode_with_payload,
);
criterion_main!(header_benches);
