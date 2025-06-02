#![cfg(feature = "csv")]

use rust_decimal::{Decimal, dec};
use shapley::{DemandMatrix, NetworkShapleyBuilder, PrivateLinks, PublicLinks};

// Accept difference in decimal by 0.0001
const ERROR_TOLERANCE: Decimal = dec!(0.0001);

#[test]
fn test_csv_example_demand1() {
    let private_links =
        PrivateLinks::from_csv("tests/private_links.csv").expect("Failed to read private links");
    let public_links =
        PublicLinks::from_csv("tests/public_links.csv").expect("Failed to read public links");
    let demand1 = DemandMatrix::from_csv("tests/demand1.csv").expect("Failed to read demand1");

    let result1 = NetworkShapleyBuilder::new(private_links, public_links, demand1)
        .demand_multiplier(dec!(1.2))
        .build()
        .compute()
        .expect("Failed to compute network shapley values");

    println!("result1");
    println!("  Operator     Value  Percent");
    for (i, sv) in result1.iter().enumerate() {
        println!(
            "{} {:>8} {:>9.4} {:>8.4}",
            i, sv.operator, sv.value, sv.percent
        );
    }

    // Verify we have the expected operators
    assert_eq!(result1.len(), 8);
    let operators: Vec<&str> = result1.iter().map(|sv| sv.operator.as_str()).collect();
    assert!(operators.contains(&"Alpha"));
    assert!(operators.contains(&"Beta"));
    assert!(operators.contains(&"Delta"));
    assert!(operators.contains(&"Epsilon"));
    assert!(operators.contains(&"Gamma"));
    assert!(operators.contains(&"Kappa"));
    assert!(operators.contains(&"Theta"));
    assert!(operators.contains(&"Zeta"));

    // Verify percentages sum to 1 (with some tolerance for rounding)
    let total: rust_decimal::Decimal = result1.iter().map(|sv| sv.percent).sum();
    assert!((total - dec!(1.0)).abs() < dec!(0.001));

    // Verify all percentages are non-negative
    assert!(result1.iter().all(|sv| sv.percent >= dec!(0)));

    // Verify values match Python output (with some tolerance for floating point differences)
    for sv in &result1 {
        match sv.operator.as_str() {
            "Alpha" => {
                assert!((sv.value - dec!(36.0066)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.0205)).abs() < ERROR_TOLERANCE);
            }
            "Beta" => {
                assert!((sv.value - dec!(29.6241)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.0168)).abs() < ERROR_TOLERANCE);
            }
            "Delta" => {
                assert!((sv.value - dec!(48.4246)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.0275)).abs() < ERROR_TOLERANCE);
            }
            "Epsilon" => {
                assert!((sv.value - dec!(1.4942)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.0008)).abs() < ERROR_TOLERANCE);
            }
            "Gamma" => {
                assert!((sv.value - dec!(874.2342)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.4972)).abs() < ERROR_TOLERANCE);
            }
            "Kappa" => {
                assert!((sv.value - dec!(241.6948)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.1375)).abs() < ERROR_TOLERANCE);
            }
            "Theta" => {
                assert!((sv.value - dec!(526.7842)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.2996)).abs() < ERROR_TOLERANCE);
            }
            "Zeta" => {
                assert!((sv.value - dec!(0.0504)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.0000)).abs() < ERROR_TOLERANCE);
            }
            _ => panic!("Unexpected operator: {}", sv.operator),
        }
    }
}

#[test]
fn test_csv_example_demand2() {
    let private_links =
        PrivateLinks::from_csv("tests/private_links.csv").expect("Failed to read private links");
    let public_links =
        PublicLinks::from_csv("tests/public_links.csv").expect("Failed to read public links");
    let demand2 = DemandMatrix::from_csv("tests/demand2.csv").expect("Failed to read demand2");

    let result2 = NetworkShapleyBuilder::new(private_links, public_links, demand2)
        .demand_multiplier(dec!(1.2))
        .build()
        .compute()
        .expect("Failed to compute network shapley values");

    println!("result2");
    println!("  Operator     Value  Percent");
    for (i, sv) in result2.iter().enumerate() {
        println!(
            "{} {:>8} {:>9.4} {:>8.4}",
            i, sv.operator, sv.value, sv.percent
        );
    }

    // Verify we have the expected operators
    assert_eq!(result2.len(), 8);
    let operators: Vec<&str> = result2.iter().map(|sv| sv.operator.as_str()).collect();
    assert!(operators.contains(&"Alpha"));
    assert!(operators.contains(&"Beta"));
    assert!(operators.contains(&"Delta"));
    assert!(operators.contains(&"Epsilon"));
    assert!(operators.contains(&"Gamma"));
    assert!(operators.contains(&"Kappa"));
    assert!(operators.contains(&"Theta"));
    assert!(operators.contains(&"Zeta"));

    // Verify percentages sum to 1 (with some tolerance for rounding)
    let total: rust_decimal::Decimal = result2.iter().map(|sv| sv.percent).sum();
    assert!((total - dec!(1.0)).abs() < dec!(0.001));

    // Verify all percentages are non-negative
    assert!(result2.iter().all(|sv| sv.percent >= dec!(0)));

    // Verify values match Python output (with some tolerance for floating point differences)
    for sv in &result2 {
        match sv.operator.as_str() {
            "Alpha" => {
                assert!((sv.value - dec!(27.0097)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.0187)).abs() < ERROR_TOLERANCE);
            }
            "Beta" => {
                assert!((sv.value - dec!(298.6752)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.2066)).abs() < ERROR_TOLERANCE);
            }
            "Delta" => {
                assert!((sv.value - dec!(160.0689)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.1107)).abs() < ERROR_TOLERANCE);
            }
            "Epsilon" => {
                // Note: Python shows -0.0000, which is effectively 0
                assert!(sv.value.abs() < ERROR_TOLERANCE);
                assert!(sv.percent.abs() < ERROR_TOLERANCE);
            }
            "Gamma" => {
                assert!((sv.value - dec!(71.6711)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.0496)).abs() < ERROR_TOLERANCE);
            }
            "Kappa" => {
                assert!((sv.value - dec!(30.9147)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.0214)).abs() < ERROR_TOLERANCE);
            }
            "Theta" => {
                assert!((sv.value - dec!(439.1823)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.3038)).abs() < ERROR_TOLERANCE);
            }
            "Zeta" => {
                assert!((sv.value - dec!(417.8871)).abs() < dec!(0.01));
                assert!((sv.percent - dec!(0.2891)).abs() < ERROR_TOLERANCE);
            }
            _ => panic!("Unexpected operator: {}", sv.operator),
        }
    }
}
