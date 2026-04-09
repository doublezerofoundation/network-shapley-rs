use std::fs::File;

use network_shapley::{
    error::Result,
    shapley::{ShapleyInput, ShapleyOutput},
    types::{Demand, Demands, Device, Devices, PrivateLink, PrivateLinks, PublicLink, PublicLinks},
};
use tabled::{builder::Builder as TableBuilder, settings::Style};

fn read_pvt_links(file_path: &str) -> Result<PrivateLinks> {
    let file = File::open(file_path).unwrap();
    let mut rdr = csv::Reader::from_reader(file);
    let mut links = vec![];
    for result in rdr.deserialize() {
        let pvt_link: PrivateLink = result.unwrap();
        links.push(pvt_link);
    }
    Ok(links)
}

fn read_devices(file_path: &str) -> Result<Devices> {
    let file = File::open(file_path).unwrap();
    let mut rdr = csv::Reader::from_reader(file);
    let mut devices = vec![];
    for result in rdr.deserialize() {
        let device: Device = result.unwrap();
        devices.push(device);
    }
    Ok(devices)
}

fn read_pub_links(file_path: &str) -> Result<PublicLinks> {
    let file = File::open(file_path).unwrap();
    let mut rdr = csv::Reader::from_reader(file);
    let mut links = vec![];
    for result in rdr.deserialize() {
        let link: PublicLink = result.unwrap();
        links.push(link);
    }
    Ok(links)
}

fn read_demands(file_path: &str) -> Result<Demands> {
    let file = File::open(file_path).unwrap();
    let mut rdr = csv::Reader::from_reader(file);
    let mut demands = vec![];
    for result in rdr.deserialize() {
        let demand: Demand = result.unwrap();
        demands.push(demand);
    }
    Ok(demands)
}

fn assert_shapley_value(
    shapley_output: &ShapleyOutput,
    operator: &str,
    expected_value: f64,
    expected_proportion: f64,
) {
    let sv = shapley_output.get(operator).unwrap();
    let value_f64 = sv.value;
    let proportion_f64 = sv.proportion;

    // Assert with tolerance of 0.01 for values and 0.0001 for proportions
    assert!(
        (value_f64 - expected_value).abs() < 0.01,
        "Value mismatch for {operator}: expected {expected_value}, got {value_f64}",
    );
    assert!(
        (proportion_f64 - expected_proportion).abs() < 0.0001,
        "Proportion mismatch for {operator}: expected {expected_proportion}, got {proportion_f64}",
    );
}

#[test]
fn test_csv_demand1() {
    let private_links = read_pvt_links("tests/private_links.csv").unwrap();
    let devices = read_devices("tests/devices.csv").unwrap();
    let public_links = read_pub_links("tests/public_links.csv").unwrap();
    let demand = read_demands("tests/demand1.csv").unwrap();

    let input = ShapleyInput {
        private_links: private_links.clone(),
        devices: devices.clone(),
        demands: demand,
        public_links: public_links.clone(),
        operator_uptime: 0.98,
        contiguity_bonus: 5.0,
        demand_multiplier: 1.2,
    };

    let result = input.compute().unwrap();
    let table = TableBuilder::from(result.clone())
        .build()
        .with(Style::psql().remove_horizontals())
        .to_string();
    println!("{table}");

    // Expected values (uptime penalty applied inside network-shapley-rs)
    assert_shapley_value(&result, "Alpha", 20.7043, 0.0224);
    assert_shapley_value(&result, "Beta", 10.6595, 0.0115);
    assert_shapley_value(&result, "Delta", 13.4308, 0.0145);
    assert_shapley_value(&result, "Epsilon", 0.0407, 0.0000);
    assert_shapley_value(&result, "Gamma", 385.4550, 0.4164);
    assert_shapley_value(&result, "Kappa", 0.0000, 0.0000);
    assert_shapley_value(&result, "Theta", 495.3964, 0.5351);
    assert_shapley_value(&result, "Zeta", 0.0445, 0.0000);
}

#[test]
fn test_csv_demand2() {
    let private_links = read_pvt_links("tests/private_links.csv").unwrap();
    let devices = read_devices("tests/devices.csv").unwrap();
    let public_links = read_pub_links("tests/public_links.csv").unwrap();
    let demand = read_demands("tests/demand2.csv").unwrap();

    let input = ShapleyInput {
        private_links: private_links.clone(),
        devices: devices.clone(),
        demands: demand,
        public_links: public_links.clone(),
        operator_uptime: 0.98,
        contiguity_bonus: 5.0,
        demand_multiplier: 1.2,
    };

    let result = input.compute().unwrap();
    let table = TableBuilder::from(result.clone())
        .build()
        .with(Style::psql().remove_horizontals())
        .to_string();
    println!("{table}");

    // Expected values (uptime penalty applied inside network-shapley-rs)
    assert_shapley_value(&result, "Alpha", 2.3309, 0.0019);
    assert_shapley_value(&result, "Beta", 168.2600, 0.1353);
    assert_shapley_value(&result, "Delta", 109.1948, 0.0878);
    assert_shapley_value(&result, "Epsilon", 96.5958, 0.0777);
    assert_shapley_value(&result, "Gamma", 24.3389, 0.0196);
    assert_shapley_value(&result, "Kappa", 10.6422, 0.0086);
    assert_shapley_value(&result, "Theta", 333.2760, 0.2680);
    assert_shapley_value(&result, "Zeta", 498.7059, 0.4011);
}
