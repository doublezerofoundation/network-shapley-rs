use network_shapley::{
    error::Result,
    shapley::{ShapleyInput, ShapleyValue},
    types::{Demand, Demands, Device, Devices, PrivateLink, PrivateLinks, PublicLink, PublicLinks},
};
use std::fs::File;
use tabled::{Table, settings::Style};

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

fn find_operator_value<'a>(
    values: &'a [ShapleyValue],
    operator: &'a str,
) -> Option<&'a ShapleyValue> {
    values.iter().find(|v| v.operator == operator)
}

fn assert_shapley_value(
    values: &[ShapleyValue],
    operator: &str,
    expected_value: f64,
    expected_proportion: f64,
) {
    let sv = find_operator_value(values, operator)
        .unwrap_or_else(|| panic!("Operator {} not found in results", operator));

    let value_f64 = sv.value;
    let proportion_f64 = sv.proportion;

    // Assert with tolerance of 0.01 for values and 0.0001 for proportions
    assert!(
        (value_f64 - expected_value).abs() < 0.01,
        "Value mismatch for {}: expected {}, got {}",
        operator,
        expected_value,
        value_f64
    );
    assert!(
        (proportion_f64 - expected_proportion).abs() < 0.0001,
        "Proportion mismatch for {}: expected {}, got {}",
        operator,
        expected_proportion,
        proportion_f64
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
    let table = Table::new(&result.values)
        .with(Style::psql().remove_horizontals())
        .to_string();
    println!("{table}");

    // Expected values from Python output
    assert_shapley_value(&result.values, "Alpha", 21.5370, 0.0208);
    assert_shapley_value(&result.values, "Beta", 10.6595, 0.0103);
    assert_shapley_value(&result.values, "Delta", 13.5257, 0.0131);
    assert_shapley_value(&result.values, "Epsilon", 0.0407, 0.0000);
    assert_shapley_value(&result.values, "Gamma", 487.1094, 0.4701);
    assert_shapley_value(&result.values, "Kappa", 0.0603, 0.0001);
    assert_shapley_value(&result.values, "Theta", 503.1153, 0.4855);
    assert_shapley_value(&result.values, "Zeta", 0.1393, 0.0001);
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
    let table = Table::new(&result.values)
        .with(Style::psql().remove_horizontals())
        .to_string();
    println!("{table}");

    // Expected values from Python output
    assert_shapley_value(&result.values, "Alpha", 2.0154, 0.0016);
    assert_shapley_value(&result.values, "Beta", 187.1198, 0.1501);
    assert_shapley_value(&result.values, "Delta", 111.6727, 0.0895);
    assert_shapley_value(&result.values, "Epsilon", 88.5022, 0.0709);
    assert_shapley_value(&result.values, "Gamma", 23.0343, 0.0184);
    assert_shapley_value(&result.values, "Kappa", 10.6421, 0.0085);
    assert_shapley_value(&result.values, "Theta", 333.5522, 0.2675);
    assert_shapley_value(&result.values, "Zeta", 490.0349, 0.3931);
}
