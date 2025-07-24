use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use luanti_mts::{MapVector, Node, Schematic};

pub fn schematic_merge(c: &mut Criterion) {
    let schematic_sizes: Vec<u16> = (1..=8).map(|pow| 2_u16.pow(pow)).collect();
    let merge_point = MapVector::new(0, 0, 0).unwrap();

    let mut group = c.benchmark_group("Schematic::merge");

    for schematic_size in schematic_sizes {
        let mut schematic_1 =
            Schematic::new(MapVector::new(schematic_size, schematic_size, schematic_size).unwrap());

        let mut schematic_2 =
            Schematic::new(MapVector::new(schematic_size, schematic_size, schematic_size).unwrap());
        let content_index = schematic_2.register_content("default:cobble".to_string());
        schematic_2
            .fill(
                MapVector::new(0, 0, 0).unwrap(),
                schematic_2.dimensions,
                &Node::with_content_index(content_index),
            )
            .unwrap();

        group.throughput(criterion::Throughput::Elements(
            schematic_2.num_nodes() as u64
        ));
        group.bench_function(BenchmarkId::from_parameter(schematic_size), |b| {
            b.iter(|| schematic_1.merge(&schematic_2, merge_point))
        });
    }

    group.finish();
}

pub fn schematic_fill(c: &mut Criterion) {
    let schematic_sizes: Vec<u16> = (1..=8).map(|pow| 2_u16.pow(pow)).collect();
    let fill_from = MapVector::new(0, 0, 0).unwrap();

    let mut group = c.benchmark_group("Schematic::fill");

    for schematic_size in schematic_sizes {
        let mut schematic =
            Schematic::new(MapVector::new(schematic_size, schematic_size, schematic_size).unwrap());
        let content_index = schematic.register_content("default:cobble".to_string());
        let node = Node::with_content_index(content_index);

        group.throughput(criterion::Throughput::Elements(schematic.num_nodes() as u64));
        group.bench_function(BenchmarkId::from_parameter(schematic_size), |b| {
            b.iter(|| schematic.fill(fill_from, schematic.dimensions, &node))
        });
    }

    group.finish();
}

criterion_group!(benches, schematic_merge, schematic_fill);
criterion_main!(benches);
