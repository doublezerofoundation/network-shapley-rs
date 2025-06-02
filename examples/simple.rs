//! A minimal, self-contained demonstration of the network_shapley() function.
//!
//! Run:
//! cargo run --example example
//!
//! Output:
//!
//! Shapley results:
//!
//! Operator    Value  Percent
//!    Alpha  24.9704   0.0722
//!     Beta 171.9704   0.4972
//!    Gamma 148.9404   0.4306

use rust_decimal::dec;
use shapley::{
    Demand, DemandMatrix, LinkBuilder, NetworkShapleyBuilder, PrivateLinks, PublicLinks,
};

fn build_sample_inputs() -> (PrivateLinks, PublicLinks, DemandMatrix) {
    // Private links
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

    // Public links
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

    // Demand
    let demand = DemandMatrix::from_demands(vec![
        Demand::new("SIN".to_string(), "NYC".to_string(), dec!(5), 1),
        Demand::new("SIN".to_string(), "FRA".to_string(), dec!(5), 1),
    ]);

    (private_links, public_links, demand)
}

fn main() {
    let (private_links, public_links, demand) = build_sample_inputs();
    let result = NetworkShapleyBuilder::new(private_links, public_links, demand)
        .build()
        .compute();
    match result {
        Ok(shapley_values) => {
            println!("\nShapley results:\n");
            println!("{:>9}  {:>9}  {:>9}", "Operator", "Value", "Percent");
            for sv in shapley_values {
                println!("{:>9}  {:>9}  {:>9}", sv.operator, sv.value, sv.percent);
            }
        }
        Err(e) => {
            eprintln!("Error computing Shapley values: {}", e);
        }
    }
}
