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

use rust_decimal_macros::dec;
use shapley::{Demand, DemandMatrix, Link, PrivateLinks, PublicLinks};

fn build_sample_inputs() -> (PrivateLinks, PublicLinks, DemandMatrix) {
    // Private links
    let private_links = PrivateLinks::from_links(vec![
        {
            let mut link = Link::new("FRA1".to_string(), "NYC1".to_string());
            link.cost = dec!(40);
            link.bandwidth = dec!(10);
            link.operator1 = "Alpha".to_string();
            link.operator2 = "0".to_string(); // Will be filled to "Alpha"
            link.uptime = dec!(1);
            link.shared = 0; // Will be assigned
            link
        },
        {
            let mut link = Link::new("FRA1".to_string(), "SIN1".to_string());
            link.cost = dec!(50);
            link.bandwidth = dec!(10);
            link.operator1 = "Beta".to_string();
            link.operator2 = "0".to_string(); // Will be filled to "Beta"
            link.uptime = dec!(1);
            link.shared = 0; // Will be assigned
            link
        },
        {
            let mut link = Link::new("SIN1".to_string(), "NYC1".to_string());
            link.cost = dec!(80);
            link.bandwidth = dec!(10);
            link.operator1 = "Gamma".to_string();
            link.operator2 = "0".to_string(); // Will be filled to "Gamma"
            link.uptime = dec!(1);
            link.shared = 0; // Will be assigned
            link
        },
    ]);

    // Public links
    let public_links = PublicLinks::from_links(vec![
        {
            let mut link = Link::new("FRA1".to_string(), "NYC1".to_string());
            link.cost = dec!(70);
            link
        },
        {
            let mut link = Link::new("FRA1".to_string(), "SIN1".to_string());
            link.cost = dec!(80);
            link
        },
        {
            let mut link = Link::new("SIN1".to_string(), "NYC1".to_string());
            link.cost = dec!(120);
            link
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

    let result = shapley::network_shapley(
        &private_links,
        &public_links,
        &demand,
        dec!(0.98),
        dec!(5.0),
        dec!(1.0),
    );

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
