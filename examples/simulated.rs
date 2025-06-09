#![cfg(feature = "csv")]

use rust_decimal::dec;
use shapley::{DemandMatrix, NetworkShapleyBuilder, PrivateLinks, PublicLinks};
use tabled::{Table, settings::Style};

fn main() {
    let private_links = PrivateLinks::from_csv("tests/simulated_private_links.csv")
        .expect("Failed to read private links");
    let public_links = PublicLinks::from_csv("tests/simulated_public_links.csv")
        .expect("Failed to read public links");
    let demand =
        DemandMatrix::from_csv("tests/simulated_demand.csv").expect("Failed to read demand1");

    let result = NetworkShapleyBuilder::new(private_links.clone(), public_links.clone(), demand)
        .operator_uptime(dec!(0.98))
        .hybrid_penalty(dec!(5))
        .demand_multiplier(dec!(1.2))
        .build()
        .compute()
        .expect("Failed to compute network shapley values");

    let table = Table::new(result)
        .with(Style::psql().remove_horizontals())
        .to_string();
    println!("{}", table)
}
