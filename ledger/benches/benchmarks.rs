use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ea_lattice_ledger::*;

fn bench_generate_update(c: &mut Criterion) {
    c.bench_function("generate_update", |b| {
        let root = [0u8; 32];
        let id = [0xEAu8; 32];
        let blob = [0x77u8; MAX_BLOB];
        
        b.iter(|| {
            generate_update(black_box(id), black_box(1), black_box(blob), black_box(root))
        });
    });
}

fn bench_verify_update(c: &mut Criterion) {
    c.bench_function("verify_update", |b| {
        let root = [0u8; 32];
        let id = [0xEAu8; 32];
        let blob = [0x77u8; MAX_BLOB];
        let update = generate_update(id, 1, blob, root);
        
        b.iter(|| {
            verify_update(black_box(root), black_box(&update))
        });
    });
}

fn bench_square_mod_n(c: &mut Criterion) {
    c.bench_function("square_mod_n", |b| {
        let input = [0x42u8; 32];
        
        b.iter(|| {
            square_mod_n(black_box(&input))
        });
    });
}

criterion_group!(
    benches,
    bench_generate_update,
    bench_verify_update,
    bench_square_mod_n
);
criterion_main!(benches);
