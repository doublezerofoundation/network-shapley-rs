use rust_decimal::dec;
use shapley::{
    Demand, DemandMatrix, LinkBuilder, NetworkShapleyBuilder, PrivateLinks, PublicLinks,
};

#[test]
fn test_example_run_output() {
    // This test verifies the output matches the Python example_run.py
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

    let result = NetworkShapleyBuilder::new(private_links, public_links, demand)
        .build()
        .compute()
        .unwrap();
    println!("result: {:#?}", result);

    // Verify we have the correct operators
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].operator, "Alpha");
    assert_eq!(result[1].operator, "Beta");
    assert_eq!(result[2].operator, "Gamma");

    // Checks:
    // 1. Percentages sum to 1
    let total: rust_decimal::Decimal = result.iter().map(|sv| sv.percent).sum();
    assert_eq!(total, dec!(1.0));

    // 2. All percentages are non-negative
    assert!(result.iter().all(|sv| sv.percent >= dec!(0)));

    // 3. Beta and Gamma should have significant value (they provide the main paths)
    assert!(result[1].value > dec!(0)); // Beta
    assert!(result[2].value > dec!(0)); // Gamma

    // 4. We match with python exactly
    // Shapley results (from python):
    // Operator     Value       Percent
    // Alpha        24.9704     0.0722
    // Beta         171.9704    0.4972
    // Gamma        148.9404    0.4306
    assert!(result[0].value == dec!(24.9704)); // Alpha value
    assert!(result[0].percent == dec!(0.0722)); // Alpha percent
    assert!(result[1].value == dec!(171.9704)); // Beta value
    assert!(result[1].percent == dec!(0.4972)); // Beta percent
    assert!(result[2].value == dec!(148.9404)); // Gamma value
    assert!(result[2].percent == dec!(0.4306)); // Gamma percent
}
