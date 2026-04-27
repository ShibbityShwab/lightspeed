/// Criterion benchmarks for FEC encoder and decoder.
///
/// Measures XOR parity generation and single-packet recovery throughput
/// at block sizes K = 2, 4, 8, 16 with realistic game packet sizes.
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use lightspeed_protocol::{FecDecoder, FecEncoder, FecHeader};

// Typical game UDP payload sizes to benchmark
const PAYLOAD_SIZES: &[usize] = &[64, 256, 512, 1024];

fn bench_fec_encoder_add_packet(c: &mut Criterion) {
    let mut group = c.benchmark_group("fec_encoder/add_packet");

    for &k in &[2u8, 4, 8, 16] {
        for &payload_size in PAYLOAD_SIZES {
            let payload = vec![0xAAu8; payload_size];

            group.throughput(Throughput::Bytes(payload_size as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("K{k}"), format!("{payload_size}B")),
                &(k, &payload),
                |b, &(k, p)| {
                    b.iter_with_setup(
                        || FecEncoder::new(k),
                        |mut enc| {
                            // Add K-1 packets (don't complete the block — that would
                            // allocate a Bytes clone; we're measuring the XOR hot path)
                            for _ in 0..k - 1 {
                                black_box(enc.add_packet(black_box(p)));
                            }
                            // Add the final packet that completes the block
                            black_box(enc.add_packet(black_box(p)))
                        },
                    )
                },
            );
        }
    }

    group.finish();
}

fn bench_fec_encoder_full_block(c: &mut Criterion) {
    let mut group = c.benchmark_group("fec_encoder/full_block");

    for &k in &[2u8, 4, 8, 16] {
        let payload_size = 256usize; // representative game packet
        let payload = vec![0x55u8; payload_size];

        // Throughput = K * payload_size bytes processed per block
        group.throughput(Throughput::Bytes((k as usize * payload_size) as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("K{k}_256B")),
            &k,
            |b, &k| {
                b.iter_with_setup(
                    || FecEncoder::new(k),
                    |mut enc| {
                        let mut parity = None;
                        for _ in 0..k {
                            parity = enc.add_packet(black_box(&payload));
                        }
                        black_box(parity)
                    },
                )
            },
        );
    }

    group.finish();
}

fn bench_fec_decoder_receive_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("fec_decoder/receive_data");
    let payload_size = 256usize;

    for &k in &[2u8, 4, 8, 16] {
        let payload = vec![0xBBu8; payload_size];
        let data = bytes::Bytes::copy_from_slice(&payload);

        group.throughput(Throughput::Bytes(payload_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("K{k}")),
            &k,
            |b, &k| {
                b.iter_with_setup(
                    || FecDecoder::new(),
                    |mut dec| {
                        let fec = FecHeader::data(0, 0, k);
                        black_box(dec.receive_data(black_box(&fec), data.clone()))
                    },
                )
            },
        );
    }

    group.finish();
}

fn bench_fec_recovery(c: &mut Criterion) {
    let mut group = c.benchmark_group("fec_decoder/recovery");

    for &k in &[2u8, 4, 8, 16] {
        for &payload_size in PAYLOAD_SIZES {
            // Pre-generate encoder output (parity + data packets)
            let payload = vec![0xCCu8; payload_size];
            let mut enc = FecEncoder::new(k);
            let mut parity_bytes = None;
            for _ in 0..k {
                parity_bytes = enc.add_packet(&payload);
            }
            let parity = parity_bytes.unwrap();

            group.throughput(Throughput::Bytes(payload_size as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("K{k}"), format!("{payload_size}B")),
                &(k, &payload, &parity),
                |b, &(k, p, par)| {
                    b.iter_with_setup(
                        || {
                            // Set up a decoder that has received K-1 data packets
                            // (missing index 0) and is ready to receive parity
                            let mut dec = FecDecoder::new();
                            for i in 1..k {
                                let fec = FecHeader::data(0, i, k);
                                dec.receive_data(&fec, bytes::Bytes::copy_from_slice(p));
                            }
                            dec
                        },
                        |mut dec| {
                            let fec = FecHeader::parity(0, k);
                            black_box(dec.receive_parity(
                                black_box(&fec),
                                bytes::Bytes::copy_from_slice(par),
                            ))
                        },
                    )
                },
            );
        }
    }

    group.finish();
}

criterion_group!(
    fec_benches,
    bench_fec_encoder_add_packet,
    bench_fec_encoder_full_block,
    bench_fec_decoder_receive_data,
    bench_fec_recovery,
);
criterion_main!(fec_benches);
