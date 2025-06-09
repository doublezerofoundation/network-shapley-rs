use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use rust_decimal::dec;
use shapley::{
    Demand, DemandMatrix, LinkBuilder, NetworkShapleyBuilder, PrivateLinks, PublicLinks, lp,
};
use std::hint::black_box;

/// Generate a realistic test network with specified number of operators
/// Based on the example network structure to ensure validity
fn generate_valid_test_network(n_operators: usize) -> (PrivateLinks, PublicLinks, DemandMatrix) {
    // Define city codes and switch suffixes
    let cities = [
        "NYC", "LAX", "CHI", "DAL", "SEA", "BOS", "ATL", "DEN", "MIA", "PHX", "SFO", "DCA",
    ];
    let switch_suffix = "1";

    let mut private_links = Vec::new();
    let operator_names: Vec<String> = (0..n_operators)
        .map(|i| format!("Operator{}", i + 1))
        .collect();

    // Create a ring topology for private links to ensure connectivity
    // Each operator owns links in sequence
    for i in 0..n_operators {
        let from_idx = i % cities.len();
        let to_idx = (i + 1) % cities.len();

        let link = LinkBuilder::new(
            format!("{}{}", cities[from_idx], switch_suffix),
            format!("{}{}", cities[to_idx], switch_suffix),
        )
        .cost(dec!(40) + dec!(10) * rust_decimal::Decimal::from(i as i32))
        .bandwidth(dec!(10))
        .operator1(operator_names[i % n_operators].clone())
        .build();

        private_links.push(link);
    }

    // Add some cross-connections for operators to create more interesting coalitions
    if n_operators >= 3 {
        for i in 0..n_operators.min(cities.len() - 2) {
            let from_idx = i;
            let to_idx = (i + 2) % cities.len();

            let link = LinkBuilder::new(
                format!("{}{}", cities[from_idx], switch_suffix),
                format!("{}{}", cities[to_idx], switch_suffix),
            )
            .cost(dec!(60) + dec!(5) * rust_decimal::Decimal::from(i as i32))
            .bandwidth(dec!(8))
            .operator1(operator_names[(i + 1) % n_operators].clone())
            .uptime(dec!(0.98))
            .build();

            private_links.push(link);
        }
    }

    // Create public links - ensure complete coverage
    // We need to cover ALL cities that are used in private links
    let mut cities_used = std::collections::HashSet::new();
    for link in &private_links {
        // Extract city names from switches (remove suffix)
        if link.start.len() >= 3 {
            cities_used.insert(&link.start[..3]);
        }
        if link.end.len() >= 3 {
            cities_used.insert(&link.end[..3]);
        }
    }

    // Create public links as a full mesh between all used cities
    let mut public_links = Vec::new();
    let cities_vec: Vec<&str> = cities_used.into_iter().collect();

    for i in 0..cities_vec.len() {
        for j in (i + 1)..cities_vec.len() {
            let link = LinkBuilder::new(
                format!("{}{}", cities_vec[i], switch_suffix),
                format!("{}{}", cities_vec[j], switch_suffix),
            )
            .cost(dec!(70) + dec!(10) * rust_decimal::Decimal::from((j - i) as i32))
            .build();

            public_links.push(link);
        }
    }

    // Create demands - ensure we only reference cities that have switches
    let cities_with_switches: Vec<&str> = cities_vec
        .iter()
        .take(cities_vec.len().min(cities.len()))
        .copied()
        .collect();

    let mut demands = Vec::new();

    // Traffic type 1: from first city to multiple destinations
    if !cities_with_switches.is_empty() {
        let source_city = cities_with_switches[0];
        for city in cities_with_switches
            .iter()
            .take(cities_with_switches.len().min(4))
            .skip(1)
        {
            demands.push(Demand::new(
                source_city.to_string(),
                city.to_string(),
                dec!(5),
                1,
            ));
        }

        // If we have many operators, add a second traffic type with a different source
        if n_operators > 6 && cities_with_switches.len() > 2 {
            let second_source = cities_with_switches[cities_with_switches.len() - 1];
            for city in cities_with_switches
                .iter()
                .take(2.min(cities_with_switches.len() - 1))
            {
                // for i in 0..2.min(cities_with_switches.len() - 1) {
                if *city != second_source {
                    demands.push(Demand::new(
                        second_source.to_string(),
                        city.to_string(),
                        dec!(3),
                        2, // Different traffic type
                    ));
                }
            }
        }
    }

    (
        PrivateLinks::from_links(private_links),
        PublicLinks::from_links(public_links),
        DemandMatrix::from_demands(demands),
    )
}

/// Benchmark the complete network_shapley computation
fn benchmark_shapley_computation(c: &mut Criterion) {
    let mut group = c.benchmark_group("shapley_computation");

    // Configure sample size for different operator counts
    let configs = vec![(2, 100), (4, 100), (6, 50), (8, 20), (10, 10), (12, 10)];

    for (n_operators, sample_size) in configs {
        group.sample_size(sample_size);

        let (private_links, public_links, demand) = generate_valid_test_network(n_operators);

        group.bench_with_input(
            BenchmarkId::new("operators", n_operators),
            &n_operators,
            |b, _| {
                b.iter(|| {
                    NetworkShapleyBuilder::new(
                        black_box(private_links.clone()),
                        black_box(public_links.clone()),
                        black_box(demand.clone()),
                    )
                    .operator_uptime(black_box(dec!(0.98)))
                    .hybrid_penalty(black_box(dec!(5.0)))
                    .demand_multiplier(black_box(dec!(1.0)))
                    .build()
                    .compute()
                })
            },
        );
    }

    group.finish();
}

/// Benchmark individual components
fn benchmark_components(c: &mut Criterion) {
    let mut group = c.benchmark_group("shapley_components");

    // Use a moderate size for component benchmarks
    let n_operators = 6;
    let (private_links, public_links, demand) = generate_valid_test_network(n_operators);

    // Benchmark consolidate_map
    group.bench_function("consolidate_map", |b| {
        b.iter(|| {
            lp::consolidate_map(
                black_box(&private_links),
                black_box(&public_links),
                black_box(&demand),
                black_box(dec!(5.0)),
            )
        })
    });

    // Benchmark lp_primitives
    let link_map = lp::consolidate_map(&private_links, &public_links, &demand, dec!(5.0))
        .expect("Failed to consolidate map");

    group.bench_function("lp_primitives", |b| {
        b.iter(|| {
            lp::primitives(
                black_box(&link_map),
                black_box(&demand),
                black_box(dec!(1.0)),
            )
        })
    });

    // Benchmark coalition enumeration for different sizes
    use shapley::coalition_computation::{enumerate_operators, generate_coalition_bitmap};

    let operators = enumerate_operators(&private_links.links);

    group.bench_function("generate_coalition_bitmap", |b| {
        b.iter(|| generate_coalition_bitmap(black_box(operators.len())))
    });

    group.finish();
}

/// Benchmark scaling with network complexity (fixed operators, varying network size)
fn benchmark_network_complexity(c: &mut Criterion) {
    let mut group = c.benchmark_group("shapley_network_complexity");
    group.sample_size(20);

    let n_operators = 5; // Fixed number of operators

    // Vary the amount of traffic demands
    for n_demands in [2, 5, 10, 15].iter() {
        let (private_links, public_links, _) = generate_valid_test_network(n_operators);

        // Create varying amounts of demands
        let cities = vec!["NYC", "LAX", "CHI", "DAL", "SEA", "BOS", "ATL", "DEN"];
        let mut demands = Vec::new();

        // Create demands up to n_demands
        for i in 1..((*n_demands).min(cities.len())) {
            demands.push(Demand::new(
                cities[0].to_string(),
                cities[i].to_string(),
                dec!(5) + rust_decimal::Decimal::from(i as i32),
                1,
            ));
        }

        let demand_matrix = DemandMatrix::from_demands(demands);

        group.bench_with_input(BenchmarkId::new("demands", n_demands), n_demands, |b, _| {
            b.iter(|| {
                NetworkShapleyBuilder::new(
                    black_box(private_links.clone()),
                    black_box(public_links.clone()),
                    black_box(demand_matrix.clone()),
                )
                .operator_uptime(black_box(dec!(0.98)))
                .hybrid_penalty(black_box(dec!(5.0)))
                .demand_multiplier(black_box(dec!(1.0)))
                .build()
                .compute()
            })
        });
    }

    group.finish();
}

/// Benchmark the example case for reference
fn benchmark_example(c: &mut Criterion) {
    let mut group = c.benchmark_group("shapley_example");

    // Create the exact example from the code
    let private_links = PrivateLinks::from_links(vec![
        {
            LinkBuilder::new("FRA1".to_string(), "NYC1".to_string())
                .cost(dec!(40))
                .bandwidth(dec!(10))
                .operator1("Alpha".to_string())
                .build()
        },
        {
            LinkBuilder::new("FRA1".to_string(), "SIN1".to_string())
                .cost(dec!(50))
                .bandwidth(dec!(10))
                .operator1("Beta".to_string())
                .build()
        },
        {
            LinkBuilder::new("SIN1".to_string(), "NYC1".to_string())
                .cost(dec!(80))
                .bandwidth(dec!(10))
                .operator1("Gamma".to_string())
                .build()
        },
    ]);

    let public_links = PublicLinks::from_links(vec![
        {
            LinkBuilder::new("FRA1".to_string(), "NYC1".to_string())
                .cost(dec!(70))
                .build()
        },
        {
            LinkBuilder::new("FRA1".to_string(), "SIN1".to_string())
                .cost(dec!(80))
                .build()
        },
        {
            LinkBuilder::new("SIN1".to_string(), "NYC1".to_string())
                .cost(dec!(120))
                .build()
        },
    ]);

    let demand = DemandMatrix::from_demands(vec![
        Demand::new("SIN".to_string(), "NYC".to_string(), dec!(5), 1),
        Demand::new("SIN".to_string(), "FRA".to_string(), dec!(5), 1),
    ]);

    group.bench_function("reference_example", |b| {
        b.iter(|| {
            NetworkShapleyBuilder::new(
                black_box(private_links.clone()),
                black_box(public_links.clone()),
                black_box(demand.clone()),
            )
            .operator_uptime(black_box(dec!(0.98)))
            .hybrid_penalty(black_box(dec!(5.0)))
            .demand_multiplier(black_box(dec!(1.0)))
            .build()
            .compute()
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_shapley_computation,
    benchmark_components,
    benchmark_network_complexity,
    benchmark_example
);
criterion_main!(benches);
