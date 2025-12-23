use criterion::{criterion_group, criterion_main, Criterion};

fn benchmark_symbiote(c: &mut Criterion) {
    c.bench_function("symbiote_dummy", |b| {
        b.iter(|| {
            // Dummy benchmark
            1 + 1
        })
    });
}

criterion_group!(benches, benchmark_symbiote);
criterion_main!(benches);
