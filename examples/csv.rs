#![cfg(feature = "csv")]

use rust_decimal::dec;
use shapley::{DemandMatrix, NetworkShapleyBuilder, PrivateLinks, PublicLinks};
use tabled::{Table, settings::Style};

fn main() {
    let private_links =
        PrivateLinks::from_csv("tests/private_links.csv").expect("Failed to read private links");
    let public_links =
        PublicLinks::from_csv("tests/public_links.csv").expect("Failed to read public links");
    let demand1 = DemandMatrix::from_csv("tests/demand1.csv").expect("Failed to read demand1");

    let result1 = NetworkShapleyBuilder::new(private_links.clone(), public_links.clone(), demand1)
        .demand_multiplier(dec!(1.2))
        .build()
        .compute()
        .expect("Failed to compute network shapley values");

    let t1 = Table::new(result1)
        .with(Style::psql().remove_horizontals())
        .to_string();
    println!("result1:\n{}", t1);

    let demand2 = DemandMatrix::from_csv("tests/demand2.csv").expect("Failed to read demand2");

    let result2 = NetworkShapleyBuilder::new(private_links, public_links, demand2)
        .demand_multiplier(dec!(1.2))
        .build()
        .compute()
        .expect("Failed to compute network shapley values");

    let t2 = Table::new(result2)
        .with(Style::psql().remove_horizontals())
        .to_string();
    println!("result2:\n{}", t2)
}
